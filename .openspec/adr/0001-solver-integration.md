---
status: Accepted
date: 2026-07-27
---

# ADR-0001: Solver Integration Story with `mvl-lang/mvl`

## Context

`rust-refine` (spec Requirement 3, issue #8) discharges refinement obligations through a layered dispatcher (`L1` trivial → `L2` intervals → `L3` path enumeration → `L4` Cooper QE → `L5` SMT → runtime fallback), mirroring the one the MVL compiler itself uses. Before `rust-refine` can be implemented, we need to decide where that dispatcher's implementation actually lives.

An earlier draft of this ADR picked shelling out to `mvl solve --json` (reusing `mvl-lang/mvl`'s own solver as a subprocess). That was wrong, for two reasons surfaced in review:

1. **It isn't independent verification.** The epic's own stated goal is *"MVL's semantics proven language-independent by a completely independent implementation."* If `rust-refine` just calls the same solver `mvl-lang/mvl` uses — whether by shelling out to it (Option 1, below) or linking it as a library (Option 2) — then `mvl-rust` is the same solver with a Rust-flavored UI on top, not a second implementation. A bug or gap in that one solver would silently pass "independent" verification in `mvl-rust` too, since there's nothing independent to disagree with it. Cross-validation only works if the two implementations are actually separate — that's the entire mechanism by which it can catch a divergence.
2. **It risks corrupting `mvl-lang/mvl`'s own codebase.** Rust has a larger language surface than MVL. If `rust-refine`'s obligations need the shared solver to understand Rust-specific constructs MVL itself has no concept of, there's direct pressure to grow `mvl-lang/mvl`'s solver — a codebase that should stay scoped to MVL's own, smaller language — to accommodate a bolt-on host language it was never designed for. Keeping the two solvers separate keeps that pressure from ever reaching `mvl-lang/mvl` in the first place.

## Options considered

1. **Shell out** to `mvl solve --json` per obligation.
   - Ties `mvl-rust`'s correctness to `mvl-lang/mvl`'s actual solver — not independent (see above). Requires the `mvl` binary on `PATH`. IPC overhead per obligation.

2. **Link the solver as a Rust library** (`libmvl_solver`).
   - Same independence problem as Option 1, just without the IPC cost. Still the same solver underneath; still requires `mvl-lang/mvl` to expose it, and still couples `mvl-rust`'s obligation-discharge correctness to a single shared implementation.

3. **Reimplement** the dispatcher inside `mvl-rust-core`, fully self-contained.
   - No cross-repo dependency, no shared binary/crate to break independence.
   - Real, substantial work — a full `L1`–`L5` dispatcher (culminating in an actual SMT solver at `L5`) is a large undertaking on its own.

## Decision

**Option 3.** `mvl-rust-core` implements its own obligation dispatcher, with no dependency — build-time, runtime, or logical — on `mvl-lang/mvl`'s solver. This is the only option consistent with the project's own independent-implementation premise.

**v0.1 scope, to keep this tractable:** implement **`L1` (trivial syntactic checks) and `L2` (interval arithmetic) natively**, in Rust, with no external solver dependency — both are well-understood, tractable techniques on their own. Any obligation that would need `L3` (path enumeration), `L4` (Cooper quantifier elimination), or `L5` (SMT) falls through to a **runtime check** instead, matching spec Requirement 3's own "uncloseable obligation surfaces as runtime check" scenario. `L3`–`L5` are deferred to their own future tickets, not attempted as part of `rust-refine` v0.1 or this ADR.

> **Scope since delivered** (the paragraph above records the v0.1 decision, not current state):
> `L3` landed as bounded-quantifier expansion in #31, and `L4` as Fourier–Motzkin
> elimination in #35 — note that is *not* the Cooper quantifier elimination named
> above, an upstream naming inaccuracy tracked as [`mvl-lang/mvl`#2022](https://github.com/mvl-lang/mvl/issues/2022).
> #38 (ADR-0002) added call-site obligations against a hypothesis context, so the
> dispatcher is no longer coherence-only. `L5` (SMT) remains deferred to #37.

`mvl_rust_core::solver::SolverBackend` (`crates/mvl-rust-core/src/solver.rs`, from #3) is still the right abstraction point — `rust-refine` (#8) depends on the trait, not a concrete backend. What changes is the default/only backend: a native `L1`+`L2` implementation replaces `ShellOutSolver` as what actually ships. `ShellOutSolver` itself is removed, not kept as a fallback option — there is no scenario in this decision where shelling out to `mvl solve` is the right answer, so keeping it around as dead-but-compiling code would misrepresent the architecture to the next reader.

## Consequences

- `rust-refine` v0.1 can only prove obligations that reduce to `L1`/`L2` reasoning (e.g. `x >= 0 && x < 100`, `x == 5`); anything needing path-sensitivity, quantifiers, or full SMT gets a runtime check instead of a compile-time proof, until `L3`–`L5` land as later work. (Since delivered: `L3` in #31, `L4` in #35 — see the scope note above. Only `L5` still falls through.)
- No external tool dependency (`mvl` binary, `PATH` requirements) for any crate using `rust-refine` — a meaningful simplification over the shell-out story.
- `mvl-lang/mvl`'s own codebase is never asked to accommodate `mvl-rust`'s needs.
- `crates/mvl-rust-core/src/solver.rs` needs rework: `ShellOutSolver` removed, a native interval/trivial-check backend added in its place.
- The tracking ticket requesting `mvl-lang/mvl` expose a solver crate ([`mvl-lang/mvl`#2007](https://github.com/mvl-lang/mvl/issues/2007), filed against the earlier, incorrect draft of this decision) is no longer wanted and should be closed with an explanation, not left open.

## Links

- `mvl-lang/mvl-rust`#7 (this decision)
- `mvl-lang/mvl-rust`#8 (`rust-refine`, blocked on this)
- [`mvl-lang/mvl`#2007](https://github.com/mvl-lang/mvl/issues/2007) (superseded — closing, see above)
