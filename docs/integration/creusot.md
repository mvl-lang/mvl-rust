# Coexisting with Creusot

[Creusot](https://github.com/creusot-rs/creusot) is a deductive verifier for
Rust: it translates a function and its contracts into WhyML and discharges
the resulting proof obligations through Why3, against SMT backends
(typically Z3, CVC, or Alt-Ergo). Contracts are written in Creusot's own
specification logic (Pearlite) via macros from the `creusot_contracts` crate.

## What overlaps, what doesn't

- **Real naming overlap to watch for.** Creusot's contract macros are
  literally named `requires`/`ensures` (e.g. `creusot_contracts::requires`),
  the same words `mvl-rust`'s `mvl::requires`/`mvl::ensures` use. `mvl-rust`
  is always invoked fully-qualified (`#[mvl::requires(...)]`), so there's no
  collision as long as a function doesn't also `use creusot_contracts::*;`
  and write a bare `#[requires(...)]` expecting it to mean the `mvl` one —
  keep both fully qualified on any function carrying contracts from both
  tools, and check `creusot_contracts`' current import conventions.
- **Different depth, different scope.** Creusot proves full functional
  correctness against an arbitrarily expressive specification logic —
  strictly more than `rust-refine`'s native layers can reach, at the cost of
  needing Why3 tooling, more verification time, and specifications written
  in Pearlite rather than plain Rust boolean expressions. `rust-refine`'s
  proofs are a much narrower fragment (comparisons and linear/QF-NIA
  arithmetic over integers) but come for free — no extra toolchain, no
  proof time, native to every `cargo build`.
- **Same underlying idea, different cost curve.** Both tools are
  attribute-driven pre/postcondition checking. The practical difference is
  where each pays its cost: `mvl-rust` pays nothing extra when a proof
  closes natively and falls back to a runtime check when it doesn't;
  Creusot pays a real (often substantial) verification-time cost for a
  strictly stronger guarantee.

## A workable split

- Use `mvl-rust`'s `rust-refine` as the fast, always-on layer for the
  contracts most call sites actually need (integer bounds, simple
  arithmetic invariants) — it costs nothing to leave on.
- Reserve Creusot for the specific functions where full functional
  correctness is the actual goal (a core algorithm, a security-critical
  invariant) and the Why3 toolchain/verification-time cost is justified.
- If a function needs both, keep every contract attribute fully qualified
  (`#[mvl::requires(...)]` alongside `#[creusot_contracts::requires(...)]`
  or its current qualified path) rather than importing either namespace
  unqualified.
