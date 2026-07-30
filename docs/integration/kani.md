# Coexisting with Kani

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
  checking, which costs real CI time and needs an explicit unwind bound.
- **`rust-limit` and Kani pull in opposite directions on `unsafe`.**
  `rust-limit` rejects `unsafe` outright, because the rest of `mvl-rust`'s
  proofs assume it's absent. Kani's proofs are most valuable exactly where
  `unsafe` shows up. If a codebase has both mvl-rust-checked modules and a
  Kani-verified `unsafe` core, keep them in separate files/modules —
  `rust-limit` only needs to pass on the former.

## A workable split

- Use `mvl-rust` (`rust-total`, `rust-refine`) on the ordinary-Rust business
  logic: panic-freedom, termination, and pre/postconditions over integer
  arithmetic, proved natively and cheaply on every build.
- Reserve Kani harnesses for the `unsafe` core, FFI boundaries, or anywhere
  you need a genuine bounded-model-checking guarantee (memory safety,
  bit-level correctness) that no attribute-based static check can give you.
- Run both in CI as separate jobs — Kani's runtime is usually much larger
  than `mvl-rust`'s (bounded model checking vs. a fast native solver stack),
  so gating on it separately keeps the fast checks fast.
