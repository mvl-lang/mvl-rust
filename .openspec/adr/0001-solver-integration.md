---
status: Accepted
date: 2026-07-27
---

# ADR-0001: Solver Integration Story with `mvl-lang/mvl`

## Context

`rust-refine` (spec Requirement 3, issue #8) discharges refinement obligations through the same layered dispatcher (`L1` trivial → `L2` intervals → `L3` path enumeration → `L4` Cooper QE → `L5` SMT → runtime fallback) the MVL compiler itself uses. That dispatcher lives inside `mvl-lang/mvl`. Before `rust-refine` can be implemented, we need to decide how `mvl-rust` reaches it — this decision was explicitly called out as a blocker for #8 and is being made before starting that ticket.

## Options considered

1. **Shell out** to `mvl solve --json` per obligation.
   - Simplest to implement. Already prototyped: `mvl_rust_core::solver::ShellOutSolver` (`crates/mvl-rust-core/src/solver.rs`, built in #3) implements exactly this shape — a `SolverBackend` trait plus a default implementation that writes an `Obligation` as JSON on stdin and reads a `DischargeResult` as JSON from stdout.
   - IPC overhead per obligation.
   - Requires the `mvl` binary on `PATH` at build/check time for any crate using `rust-refine` — a new external tool dependency.

2. **Link the solver as a Rust library** (`libmvl_solver`).
   - Fastest at runtime — no IPC, no external binary dependency.
   - Requires `mvl-lang/mvl` to refactor its compiler internals to expose the solver as a published, independent crate. Nontrivial upstream work; `mvl-rust` doesn't control that timeline.

3. **Reimplement** the L1–L5 dispatcher inside `mvl-rust-core`.
   - Self-contained — no cross-repo dependency at all.
   - Duplicates real, nontrivial logic (SMT integration, Cooper QE, interval arithmetic) and risks semantic drift between what `mvl` considers provable and what `mvl-rust` does. This directly undermines the epic's own stated goal — *"MVL's semantics proven language-independent by a completely independent implementation"* — since an independent **reimplementation of the same solver** isn't independent verification, it's two copies of one thing that can silently diverge.

## Decision

Start with **Option 1** (shell out). Migrate to **Option 2** once `mvl-lang/mvl` exposes a solver crate. Do not pursue Option 3 — reimplementing risks exactly the semantic drift the cross-validation pitch exists to avoid.

The shell-out interface is already implemented: `mvl_rust_core::solver::{SolverBackend, ShellOutSolver, Obligation, DischargeResult, Layer}` (`crates/mvl-rust-core/src/solver.rs`). `rust-refine` (#8) should depend on the `SolverBackend` **trait**, not directly on `ShellOutSolver` — migrating to Option 2 later means implementing a new backend (e.g. `LinkedSolver`) and switching which one `rust-refine` constructs, with no change to `rust-refine`'s own obligation-discharge logic.

## Migration trigger

Migrate from Option 1 to Option 2 when **both** of these hold:

- `mvl-lang/mvl` publishes a stable, versioned solver crate exposing the L1–L5 dispatcher's `solve` entry point directly (not just the `mvl solve --json` CLI).
- That crate's obligation/result wire types are stable enough that `mvl-rust-core`'s own `Obligation`/`DischargeResult`/`Layer` types (already schema-aligned with the assurance-JSON design, #13) can be mapped to/from it without a breaking change to `rust-refine`'s public API.

Filed as a tracking ticket in `mvl-lang/mvl` (linked below) so this gets revisited when that lands, rather than left as an implicit assumption nobody checks.

## Consequences

- `rust-refine` v0.1 (#8) can proceed now without waiting on any upstream compiler refactor.
- Every `rust-refine`-enabled crate needs the `mvl` binary on `PATH` at build/check time — worth calling out explicitly in `rust-refine`'s own docs when #8 ships.
- The exact wire format for `mvl solve --json` (the input/output JSON shape) is **not yet locked** with `mvl-lang/mvl` — `crates/mvl-rust-core/src/solver.rs`'s current `Obligation`/`DischargeResult` types are `mvl-rust`'s own first-cut guess, not a contract `mvl-lang/mvl` has agreed to. This ADR does not resolve that; #8 (or a follow-up ticket) needs to confirm the real `mvl solve --json` contract with the `mvl-lang/mvl` side before relying on it beyond local testing (today's tests in `crates/mvl-rust-core/tests/solver.rs` run against a fake stand-in script, not the real `mvl` binary).

## Links

- `mvl-lang/mvl-rust`#7 (this decision)
- `mvl-lang/mvl-rust`#8 (`rust-refine`, blocked on this)
- [`mvl-lang/mvl`#2007](https://github.com/mvl-lang/mvl/issues/2007) (tracking ticket requesting solver-crate exposure)
