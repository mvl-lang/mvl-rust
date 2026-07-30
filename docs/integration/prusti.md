# Coexisting with Prusti

[Prusti](https://viperproject.github.io/prusti-dev/) is a deductive verifier
built on Rust's MIR and the Viper verification infrastructure (Silicon/
Carbon), using separation logic to reason natively about ownership and
borrowing alongside functional contracts (`#[requires]`, `#[ensures]`,
`#[pure]`, `#[invariant]`).

## What overlaps, what doesn't

- **Real naming overlap to watch for**, same shape as Creusot: Prusti's
  contract attributes are also literally named `requires`/`ensures`.
  `mvl-rust` stays collision-free by always being fully qualified
  (`#[mvl::requires(...)]`) — keep Prusti's own attributes fully qualified
  too (per its current import convention) on any function using both.
- **Ownership-aware vs. attribute-syntactic.** Prusti's biggest advantage
  over `rust-refine` is that its proofs understand Rust's borrow checker
  natively — it can reason about mutable references, loop invariants over
  `&mut`, and structural invariants on types. `rust-refine`'s native layers
  have no borrow-checker awareness at all; they reason over the plain
  boolean/arithmetic expression a predicate is, syntactically, via `syn`.
  A contract that fundamentally depends on aliasing/mutation behavior is a
  Prusti (or Creusot) question, not one `rust-refine` can answer today.
  Contracts over integer arithmetic and comparisons on plain values are
  exactly `rust-refine`'s native fragment, and closing there costs nothing.
- **Verifier-backed vs. dependency-free.** Prusti needs a real Viper/Z3
  toolchain and a verification pass with its own runtime; `rust-refine`'s
  native layers (`L1`-`L4`) have zero external dependency and run as part
  of an ordinary `cargo build`. `rust-refine`'s optional `L5` does add an
  actual Z3 dependency, but stays feature-gated and default-off for exactly
  this reason.

## A workable split

- Default to `mvl-rust` for contracts over plain values and integer
  arithmetic — no extra toolchain, proves at compile time for the fragment
  it covers, falls back to a runtime check for the rest.
- Reach for Prusti specifically where a contract needs to reason about
  mutable references, aliasing, or structural invariants across a data
  structure's lifetime — the class of properties separation logic is built
  for and `rust-refine` has no model of at all.
- Run Prusti as its own CI job (it needs its own Viper toolchain install),
  separate from the fast `cargo mvl check` step.
