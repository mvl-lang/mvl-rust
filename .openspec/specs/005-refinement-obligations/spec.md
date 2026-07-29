# 005 — Refinement Obligations and the Hypothesis Context

**Domain:** Refinement obligation discovery
**Version:** 0.1.0
**Status:** Implemented
**Date:** 2026-07-29
**Decided by:** ADR-0005

## Overview

`rust-refine` is the one annotation tool that cannot check a declaration against a body locally. Spec 003's tools ask "does this body do something forbidden?" — a question about syntax. A refinement asks "does this predicate hold for every value that can reach this point?" — a question about *values*.

This spec covers **where obligations arise and what is known at each point**. The decision procedure that discharges them is spec 006; what happens to the ones it cannot close is spec 007.

Obligations arise at three program points, asking two different questions:

| Site | Question | Entry point |
|---|---|---|
| Declaration | is the predicate coherent? | `discharge_predicate` |
| Call site | does Γ entail the callee's precondition with arguments substituted? | `discharge_entailment` |
| Return site | does Γ entail this function's postcondition with `result` bound to the returned expression? | `discharge_entailment` |

### Philosophy

- **Coherence is not a weaker entailment.** A declaration site has no arguments to reason about, so coherence is the only question available there — and a self-contradictory `requires` is a real defect worth its own error.
- **Hypotheses may be dropped; goal clauses may not.** Fewer facts make both `Γ ∧ ¬goal` and `Γ ∧ goal` easier to satisfy, so dropping a hypothesis costs precision in *both* directions and correctness in neither.
- **Γ must not contain what nothing established.** Stated as an invariant in ADR-0006 §5 and audited per fact source in Requirement 4. Source 3 was violated until #47; #50 tracks three remaining construction paths.

---

## RFC 2119 Keywords

The key words "MUST", "MUST NOT", "SHOULD", "SHOULD NOT", and "MAY" in this document are to be interpreted as described in [RFC 2119](https://datatracker.ietf.org/doc/html/rfc2119).

---

## Requirements

### Requirement 1: Declaration sites are checked for coherence [MUST]

Every `#[mvl::requires(p)]` and `#[mvl::ensures(p)]` MUST produce an obligation asking whether `p` is satisfiable. A predicate that is unsatisfiable MUST be reported as `Level::Error`.

This obligation MUST be distinguishable, in reporting, from an entailment proof — see spec 008 Requirement 3.

**Implementation:** `crates/rust-refine/src/checks.rs`

#### Scenario: A self-contradictory precondition is rejected

- GIVEN `#[mvl::requires(x >= 10 && x < 5)]`
- WHEN the declaration-site obligation is discharged
- THEN the outcome MUST be `Violated` and reported as `Level::Error`

**Tests:** `crates/mvl-rust-core/src/solver/native.rs::contradictory_interval_is_violated`

### Requirement 2: Call sites are discharged against the caller's hypothesis context [MUST]

For a call `g(args)` in `f`'s body where `g` declares `#[mvl::requires(p)]`, the tool MUST produce an obligation whose goal is `p` with `g`'s parameters replaced by the actual arguments, discharged against `Γ_f` as it stands at that call.

Call resolution MUST be same-file free functions only. A call to anything else MUST produce no obligation rather than a guessed one.

**Implementation:** `crates/rust-refine/src/checks.rs`

#### Scenario: An argument satisfying the callee's precondition proves

- GIVEN `#[mvl::requires(v > 0)] fn need_pos(v: i64)` called as `need_pos(5)`
- WHEN the call-site obligation is discharged
- THEN the outcome MUST be `Proven`

**Tests:** `crates/rust-refine/tests/call_sites.rs::a_caller_precondition_entails_the_callees`

#### Scenario: An unresolvable callee yields no obligation

- GIVEN a call to a method, a cross-file function, or a name shadowed by a local binding
- WHEN the scan runs
- THEN no call-site obligation MUST be produced
- AND no diagnostic MUST be emitted in either direction

**Tests:** `crates/rust-refine/tests/call_sites.rs::a_call_to_an_unresolvable_function_produces_no_obligation`, `::a_call_inside_a_macro_invocation_is_invisible`

### Requirement 3: Return sites must establish the function's own postcondition [MUST]

Every point at which a function's body produces its value MUST produce an obligation whose goal is the function's `ensures` with `result` bound to the returned expression, discharged against Γ at that point.

Return points MUST be recognised structurally: a trailing expression, an explicit `return`, and through `if`/`else` arms, `match` arms, and plain or `unsafe` blocks in tail position. A construct not on that list MUST NOT yield a *false* obligation.

An explicit `return` inside a closure or `async` block MUST NOT be attributed to the enclosing function.

A diverging body (`panic!`, `todo!`, `unimplemented!`, `unreachable!`) produces no `result` and MUST yield no obligation.

**Implementation:** `crates/rust-refine/src/checks.rs`

#### Scenario: A body contradicting its own postcondition is rejected

- GIVEN `#[mvl::ensures(result > 0)] fn f(a: i64) -> i64 { -1 }`
- WHEN the return-site obligation is discharged
- THEN the outcome MUST be `Violated` and reported as `Level::Error`

**Tests:** `crates/rust-refine/tests/call_sites.rs::a_body_contradicting_its_own_ensures_is_violated`

#### Scenario: A closure's return belongs to the closure

- GIVEN `#[mvl::ensures(result > 0)] fn f(a: i64) -> i64 { let g = || { return -1; }; 7 }`
- WHEN the scan runs
- THEN exactly one return-site obligation MUST be produced, for the tail `7`
- AND the closure's `-1` MUST NOT be reported as a violating return of `f`

**Tests:** `crates/rust-refine/tests/call_sites.rs::an_explicit_return_inside_a_closure_is_not_the_enclosing_functions_return`, `::a_closure_body_is_not_the_enclosing_functions_return_point`

#### Scenario: Each tail `match` arm is its own return point

- GIVEN `#[mvl::ensures(result > 0)] fn f(a: i64) -> i64 { match a { 0 => -3, _ => 2 } }`
- WHEN the scan runs
- THEN two obligations MUST be produced, one per arm
- AND the `-3` arm MUST be `Violated`

**Tests:** `crates/rust-refine/tests/call_sites.rs::each_tail_match_arm_is_its_own_return_point`

### Requirement 4: Γ accumulates parameter refinements, branch narrowing, and propagated postconditions [MUST]

The hypothesis context MUST contain:

1. the enclosing function's own `requires` clauses;
2. branch narrowing — inside `if c { … }` the condition `c` holds, in the `else` arm its negation does, and a `while` body carries its condition;
3. postcondition propagation — after `let y = g(x);`, `g`'s `ensures` with `result := y` and `g`'s parameters bound to the actual arguments, **and only when every one of `g`'s return sites discharged to `Proven`** (#47).

Γ MUST be block-scoped, so a fact never outlives the block that established it.

Source 3's condition is what makes propagation an assumption rather than a guess. A postcondition reaching `Runtime` or `Violated` MUST NOT be propagated: `rust-refine` inserts no runtime check (spec 007), so nothing anywhere enforces it.

Closure MUST be computed conservatively. The implementation uses a pre-pass that itself propagates nothing, which under-credits rather than over-credits — a return site that would only close using a propagated fact is not counted, so that function's postcondition does not propagate in turn. Imprecise, never unsound (ADR-0001 §5), and it avoids a circularity: closure would otherwise depend on the map being built.

#### Audit of Γ's three fact sources against the invariant

ADR-0006 §5 states it: *a fact is admitted to Γ only if it has been established, or is an obligation some other program point is required to discharge.* Verdict per source:

| Source | Verdict | Why |
|---|---|---|
| 1. The function's own `requires` | **holds** | A precondition is an obligation every *call site* must discharge, so assuming it inside the body is the modular-verification bargain, not an unbacked claim. |
| 2. Branch narrowing | **holds** | `c` inside `if c { … }` is established by the program's own control flow. Nothing needs to discharge it. |
| 3. Postcondition propagation | **holds since #47**; violated before | The callee's return-site obligation establishes it, and that is now checked before the fact is admitted. |

Recorded including the sources that turned out fine. #47's diagnosis was that the invariant had never been written down, so it got rediscovered one violation at a time — a verdict per source is what stops that.

A quantified `requires` is a usable *goal* but MUST NOT enter Γ as a hypothesis.

A name rebound by **any** construct MUST lose Γ's facts about it for the scope of the rebinding — `let`, assignment, compound assignment, a `&mut` borrow, and also a `for` pattern, a closure parameter, a `match` arm pattern, and an `if let`/`while let` binding. Shadowing MUST be scoped: the fact returns when the binding goes out of scope, since a blanket invalidation would disable call-site checking for any function that shadows a parameter name.

A loop body MUST retire, on entry, every name it assigns anywhere within itself. The walk is a single in-order pass, so a mutation after a call would otherwise leave the call proven from a fact false on every iteration but the first.

**Implementation:** `crates/rust-refine/src/checks.rs`

#### Scenario: A return inside a narrowed branch uses that branch's context

- GIVEN `#[mvl::ensures(result > 0)] fn f(a: i64) -> i64 { if a > 10 { a } else { 1 } }`
- WHEN both return-site obligations are discharged
- THEN both MUST be `Proven`
- AND the `a` arm MUST be proven from the branch condition `a > 10`, not from a literal

**Tests:** `crates/rust-refine/tests/call_sites.rs::a_return_inside_a_narrowed_branch_uses_that_branchs_gamma`

#### Scenario: Contradictory hypotheses entail anything

- GIVEN a Γ that is itself unsatisfiable, i.e. an unreachable program point
- WHEN any goal is discharged against it
- THEN the outcome MUST be `Proven`

**Tests:** `crates/mvl-rust-core/tests/entailment.rs::contradictory_hypotheses_entail_anything`

### Requirement 5: Γ must be invalidated when a name's value changes [MUST]

Rebinding (`let x = …`), assignment (`x = …`, `x += …`), and a mutable borrow (`&mut x`) MUST retire every Γ clause mentioning that name.

This MUST be blunt rather than dataflow-precise: the tool cannot see whether a callee writes through a `&mut`, so it MUST assume the worst. Keeping a fact about a value that is no longer there is unsound, not merely imprecise.

**Implementation:** `crates/rust-refine/src/checks.rs`

#### Scenario: Rebinding retires the hypothesis

- GIVEN `#[mvl::requires(x > 10)] fn f(x: i64) { let x = -1; need_pos(x); }`
- WHEN the call-site obligation is discharged
- THEN the hypothesis `x > 10` MUST NOT be available
- AND the outcome MUST NOT be `Proven`

**Tests:** `crates/rust-refine/tests/call_sites.rs::a_shadowing_let_invalidates_the_hypotheses_about_that_name`

### Requirement 6: Undecidable hypotheses are dropped, goal clauses are not [MUST]

A hypothesis outside the decidable fragment — an opaque call, a non-linear term, a bitwise operation — MUST be skipped rather than failing the query. Every *goal* clause MUST be decided for an outcome of `Proven`.

**Implementation:** `crates/mvl-rust-core/src/solver/native.rs`

#### Scenario: Every clause of a conjunctive goal must close

- GIVEN a goal `a > 0 && b > 0` where Γ entails only `a > 0`
- WHEN the obligation is discharged
- THEN the outcome MUST NOT be `Proven`

**Tests:** `crates/mvl-rust-core/tests/entailment.rs::every_clause_of_a_conjunctive_goal_must_be_entailed`

---

## Known Limitations

- **Coverage is 3 program points; the reference implementation generates obligations at 15.** Not covered: loop invariants, invariant preservation, `decreases` bounds, struct construction, enum-variant construction, actor spawn, refined-type-alias coercion, `let` with a declared refined type, indexing bounds.
- **Methods in `impl` blocks are entirely invisible** — attributes and body (spec 001). Most idiomatic Rust is therefore unanalysed.
- **`match`-arm patterns do not narrow Γ.** An arm *is* a return point (Requirement 3) but contributes no hypothesis. Imprecise, never unsound.
- **`?` is not a return point.** Its early `Err(…)` is not `result`-shaped under a type-free view. An unmodelled tail expression is substituted whole and falls to a runtime check rather than being skipped — #48.
- **Arithmetic is over unbounded ℤ with no overflow modelling.** `x >= i64::MAX ⊢ x + i64::MAX > 0` is `Proven` though the Rust expression overflows.
- **The four known Γ-construction violations are fixed** — `Runtime` propagation (#47) and, in #50, pattern-binding shadowing (`for`, closure params, `match` arms, `if let`/`while let`), loop-carried mutation, and arity-mismatch parameter capture. Requirement 4's audit table is what should make a fifth predicted rather than discovered.
- **Shadowing and loop retirement cost precision, deliberately.** A name shadowed in a scope loses its facts for that scope and regains them after; a name assigned anywhere in a loop body loses them for the whole body, even if the assignment follows its last use. Both under-credit rather than over-credit (ADR-0001 §5). A real fixpoint would recover the second; nothing needs it yet.
- **Obligation ids collide**, so assurance leaves are not addressable (#51).

---

## Traceability

| Layer | Artefact |
|---|---|
| **Intent** | #8 (`rust-refine` v0.1), #38 (call-site obligations), #42 (return-site obligations), #40 (Γ invalidation), #47 (Γ invariant), #50 (Γ violations), #48 (return-point doc), #51 (obligation ids) |
| **Specification** | this document; spec 006 (the decision procedure), spec 007 (residual enforcement) |
| **Decision** | ADR-0005; ADR-0006 §5 (the Γ invariant) |
| **Program** | `crates/rust-refine/src/checks.rs`, `crates/mvl-rust-core/src/solver/native.rs` |
| **Evidence** | `crates/rust-refine/tests/call_sites.rs` (51 tests), `crates/mvl-rust-core/tests/entailment.rs` (36 tests), `examples/rust-refine-demo/{compliant,violating}/` |
