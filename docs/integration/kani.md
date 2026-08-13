# Integrating with Kani

> Tracked: [#80](https://github.com/mvl-lang/mvl-rust/issues/80)

[Kani](https://model-checking.github.io/kani/) is a bit-precise model
checker for Rust: it unwinds loops, symbolically executes a `#[kani::proof]`
harness, and checks memory safety, arithmetic overflow, and user assertions
against a bounded model — backed by CBMC, not an SMT-over-unbounded-integers
solver.

## What overlaps, what doesn't

- **Little attribute overlap.** Kani's own annotations (`#[kani::proof]`,
  `#[kani::requires]`/`#[kani::ensures]` in newer releases, `kani::any()`
  for symbolic values) live under the `kani::` namespace, the same
  fully-qualified convention `mvl-rust` uses under `mvl::` — so the two sets
  of attributes don't collide syntactically. Check the Kani version you're
  on for its current contract-attribute names before assuming exact parity
  with what's described here; that surface has evolved across releases.
- **Different guarantee, different cost.** `rust-refine`'s native layers
  prove over unbounded mathematical integers, cheaply, at compile time, with
  no execution — but they only reach the fragment a Fourier-Motzkin/interval
  solver (or, with the optional `z3` feature, QF-NIA) can represent. Kani
  can check properties `rust-refine` can't touch at all (memory safety,
  bit-precise overflow, `unsafe` code) but does so by bounded model
  checking, which costs real CI time and needs an explicit unwind bound
- **`rust-limit` and Kani pull in opposite directions on `unsafe`.**
  `rust-limit` rejects `unsafe` outright, because the rest of `mvl-rust`'s
  proofs assume it's absent. Kani's proofs are most valuable exactly where
  `unsafe` shows up. If a codebase has both mvl-rust-checked modules and a
  Kani-verified `unsafe` core, keep them in separate files/modules —
  `rust-limit` only needs to pass on the former.

## A workable split

- Use `mvl-rust` (`rust-total`, `rust-refine`) on the ordinary-Rust business
  logic: `rust-refine`'s pre/postconditions over integer arithmetic are
  proved natively and cheaply on every build, and `rust-total` adds a cheap
  syntactic panic-risk scan plus a decreases-measure provability check on
  top, reusing that same native solver — provable for subtraction, never for
  division/modulo (outside the solver's linear-arithmetic system entirely),
  not a general termination proof (see `rust-total`'s Known Limitations,
  ADR-0009).
- Reserve Kani harnesses for the `unsafe` core, FFI boundaries, or anywhere
  you need a genuine bounded-model-checking guarantee (memory safety,
  bit-level correctness) that no attribute-based static check can give you.
- Run both in CI as separate jobs — Kani's runtime is usually much larger
  than `mvl-rust`'s (bounded model checking vs. a fast native solver stack),
  so gating on it separately keeps the fast checks fast.

## Example: module-split pattern

A crate with both mvl-rust-verified business logic and a Kani-verified unsafe
core:

```
src/
├── lib.rs              # re-exports, #[mvl::*] on public API
├── business_logic.rs   # safe Rust, rust-limit passes, rust-refine checks
└── unsafe_core.rs      # #[kani::proof] harnesses, rust-limit exempted
```

**`src/business_logic.rs`** — the safe, mvl-rust-checked module:

```rust
use crate::unsafe_core::fast_copy;

#[mvl::requires(src.len() == dst.len())]
#[mvl::ensures(result == src.len())]
pub fn copy_all(src: &[u8], dst: &mut [u8]) -> usize {
    // Safe wrapper; the unsafe core is verified separately
    unsafe { fast_copy(src.as_ptr(), dst.as_mut_ptr(), src.len()) }
    src.len()
}
```

**`src/unsafe_core.rs`** — the unsafe, Kani-verified module:

```rust
/// # Safety
/// - `src` and `dst` must be valid for `len` bytes
/// - `src` and `dst` must not overlap
#[cfg_attr(kani, kani::modifies(dst))]
pub unsafe fn fast_copy(src: *const u8, dst: *mut u8, len: usize) {
    std::ptr::copy_nonoverlapping(src, dst, len);
}

#[cfg(kani)]
mod proofs {
    use super::*;

    #[kani::proof]
    #[kani::unwind(17)]  // 16 bytes + 1
    fn fast_copy_no_ub() {
        let src: [u8; 16] = kani::any();
        let mut dst: [u8; 16] = [0; 16];
        let len: usize = kani::any();
        kani::assume(len <= 16);

        unsafe { fast_copy(src.as_ptr(), dst.as_mut_ptr(), len) };

        // Kani checks: no UB, no out-of-bounds, no overlap violation
        assert_eq!(&dst[..len], &src[..len]);
    }
}
```

**`rust-limit.toml`** — exempt the unsafe module:

```toml
[rust-limit]
exempt_modules = ["unsafe_core"]
```

Run both:

```bash
cargo mvl check          # rust-limit + rust-refine on business_logic
cargo kani               # bounded model checking on unsafe_core
```

## CI configuration

Add Kani as a separate, optional job. It's slow (~minutes) and shouldn't
block the fast mvl-rust checks (~seconds):

```yaml
# .github/workflows/ci.yml (addition)
kani:
  name: kani (unsafe core)
  runs-on: ubuntu-latest
  # Optional: only run on explicit trigger or main branch
  if: github.event_name == 'workflow_dispatch' || github.ref == 'refs/heads/main'
  steps:
    - uses: actions/checkout@v4
    - name: Install Kani
      run: |
        cargo install --locked kani-verifier
        cargo kani setup
    - name: Run Kani proofs
      run: cargo kani --workspace
```

## Candidate unsafe packages

Common Rust crates with `unsafe` cores that benefit from Kani verification
alongside mvl-rust on their safe API:

| Crate | Unsafe surface | Kani value |
|-------|----------------|------------|
| `std::collections` | Vec growth, HashMap probing | Memory safety, capacity overflow |
| `crossbeam` | Lock-free queues, epoch GC | Atomic ordering, ABA problem |
| `parking_lot` | Mutex internals, futex | Deadlock freedom (bounded), memory safety |
| `bytes` | Zero-copy slicing, `Bytes::split` | Aliasing safety, refcount overflow |
| `smallvec` | Inline/heap transition | Capacity overflow, uninitialized reads |
| `arrayvec` | Fixed-capacity push | Bounds checking, panic paths |

For these, the pattern is:
1. `rust-refine` on the safe public API (preconditions, postconditions)
2. Kani on the unsafe internals (memory safety, bit-precise overflow)
3. `rust-limit` exempts the internal modules

## Attribute bridge (future)

Both `mvl::requires` and `kani::requires` express preconditions. A future
integration could:

1. **Generate Kani contracts from MVL contracts** — same predicate, different
   enforcement (static proof vs. bounded model checking)
2. **Cross-validate** — if `rust-refine` proves `x > 0` and Kani finds a
   counterexample at the boundary, surface the conflict

Not implemented; tracked for Paper 6 (Verified Rust).
