# Adopting mvl-rust in an existing codebase

Most of `mvl-rust`'s tools are opt-in per function or per attribute.
`rust-limit` and `rust-total` are the two exceptions — both run whole-file
(see below for each) — so plan for those two to require a one-time pass
over an existing file rather than a purely incremental one. A recommended
order:

## 1. Start with `rust-limit`, and expect friction once

```bash
cargo mvl limit src/**/*.rs
```

This is the one whole-file check — it runs whether or not you've added any
attributes. If your codebase uses `unsafe`, `dyn Trait`, or explicit
lifetimes beyond `'static`/`'_` anywhere, you'll see violations immediately.

!!! note "This is expected, and it's fine to not fix all of it"
    `rust-limit` isn't saying your code is wrong — it's saying that code is
    outside the subset the *other* four tools can reason about. If a module
    genuinely needs `unsafe` (an FFI boundary, a hand-rolled data structure),
    leave it alone and don't add refinement/effect/IFC attributes to
    functions in that module. `rust-limit` doesn't have a suppression
    mechanism today — treat a violation as "this function isn't a candidate
    for the other tools yet," not as a required fix.

## 2. Add `#[mvl::total]` to your leaf functions first — and `#[mvl::partial]` to the rest

`rust-total` is whole-file, like `rust-limit`: every `fn` item and `impl`
method in a scanned file must carry exactly one of `#[mvl::total]` or
`#[mvl::partial]` (ADR-0012) — there's no silently-unchecked third state.
So the practical first pass over a file is two steps together, not one:

1. Mark your pure, non-recursive, panic-free leaf functions `#[mvl::total]`
   — `rust-total` needs nothing else to check them. Recursive functions
   need a `#[mvl::decreases(measure)]` alongside; if you can't name a
   strictly-decreasing measure, that's often a sign the function's
   termination argument is more subtle than it looks, not that the tool is
   wrong.
2. Mark everything else in that file `#[mvl::partial]` — the explicit,
   permanent "not making a totality claim here" declaration. This is not a
   migration shim to remove later; it's the same two-state shape upstream
   `mvl` itself uses (`total`/`partial`), and a function can move from
   `partial` to `total` at any point once you're ready to check it.

You don't have to run `rust-total` on every file in the crate at once —
scope it to one file or module at a time, same as `rust-limit`.

## 3. Add `#[mvl::requires]`/`#[mvl::ensures]` where you already have doc-comment invariants

If a function's doc comment already says "panics if `n` is negative" or
"the caller must ensure X", that's a `#[mvl::requires]` you were already
relying on informally. Adding it:

- Documents the same thing in a form the compiler checks.
- Gets you a compile-time proof at every call site `rust-refine`'s layered
  solver can close (most linear-arithmetic call sites do, natively, no
  external dependency).
- Falls through to a runtime `assert!` for anything it can't prove — so
  adding the attribute never *weakens* what you had, even in the worst case.

This is also the point where `#[mvl::unchecked]` earns its keep: if a
function's contract genuinely can't be enforced today (e.g. it depends on
external state `rust-refine` has no visibility into), mark it explicitly
rather than leaving a precondition that silently never fires.

## 4. Add `#[mvl::effect(...)]` only where effect leakage has bitten you before

`rust-effect`'s v1 scope is same-file, resolvable calls only — it won't
catch an effect that enters through a trait object or a different module.
It's most valuable in files where "does this pure-looking function actually
do I/O" has been a real source of confusion, not as a blanket policy from
day one.

## 5. Add `#[mvl::label]`/`#[mvl::relabel]` at your actual trust boundaries

IFC labeling pays off at the specific functions where tainted/secret data
crosses into or out of trusted code — an ingestion function, a
declassification/logging function. Labeling everything that touches a
`String` is not the goal; labeling the two or three functions where a real
security review would ask "wait, is this checked?" is.

## Wiring into CI

Once you're happy with what's checked, add the Gate tools as a required
step:

```yaml
- run: cargo install cargo-mvl
- run: cargo mvl check src/**/*.rs
```

and, separately, emit assurance evidence without gating the merge on it:

```yaml
- run: cargo mvl assurance src/**/*.rs > assurance.json
- uses: actions/upload-artifact@v4
  with:
    name: mvl-assurance
    path: assurance.json
```
