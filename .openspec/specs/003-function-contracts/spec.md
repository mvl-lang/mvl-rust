# 003 — Function Contracts: `total` and `effect`

**Domain:** Totality / effect propagation
**Version:** 0.1.0
**Status:** Implemented
**Date:** 2026-07-29
**Decided by:** ADR-0003

## Overview

Two tools use the attribute carrier in its simplest form: an attribute on a function declares a property, and the tool checks that function's body against it. No hypothesis context, no solver, no cross-procedural state.

```
#[mvl::total]               → rust-total:  claims this body cannot panic and terminates
                                            (checked, not proved — see Known Limitations)
#[mvl::decreases(measure)]  → rust-total:  names the measure for a recursive total function
                                            (presence checked, not that it decreases)
#[mvl::effect(Log, Clock)]  → rust-effect: this body performs at most these effects
```

This is the **baseline shape** the other annotation tools deviate from — spec 005 needs a solver and a hypothesis context; spec 004 puts its information in types instead. Establishing the simple pattern first makes those deviations legible as deviations.

### Philosophy

- **Checked, not assumed.** Unlike a refinement postcondition (spec 005), a declaration here never becomes a premise another proof rests on. A failing `#[mvl::total]` claim is an error; it is never a fact. This is why neither tool has an analogue of spec 005's Γ-soundness problem.
- **The false-positive rate is a design input, not a compromise.** Where a syntactic-only check would flag nearly all code, the check is omitted rather than shipped noisy. A tool that cries wolf on every addition is not stricter — it is ignored.
- **Silence over guessing.** Both tools emit only `Level::Error`, so a false diagnostic fails the build on correct code. A missing diagnostic is preferable.

---

## RFC 2119 Keywords

The key words "MUST", "MUST NOT", "SHOULD", "SHOULD NOT", and "MAY" in this document are to be interpreted as described in [RFC 2119](https://datatracker.ietf.org/doc/html/rfc2119).

---

## Requirements

### Requirement 1: Panic-risk constructs in a `#[mvl::total]` body are rejected [MUST]

Inside a function annotated `#[mvl::total]`, the tool MUST reject `.unwrap()`, `.expect(…)`, the `panic!`/`todo!`/`unimplemented!` macros, raw indexing (`xs[i]`), and division or modulo.

The tool MUST NOT scan functions that carry no `#[mvl::total]` annotation.

**Implementation:** `crates/rust-total/src/checks/panic_freedom.rs`

#### Scenario: An unannotated function is not scanned

- GIVEN a function containing `.unwrap()` and no `#[mvl::total]` attribute
- WHEN `cargo mvl-total` runs
- THEN no diagnostic MUST be reported — totality is opt-in

**Tests:** `crates/rust-total/tests/totality.rs::non_total_functions_are_not_scanned_at_all`

#### Scenario: A compliant total function produces no diagnostics

- GIVEN a `#[mvl::total]` function whose body contains none of the rejected constructs
- WHEN the panic-freedom check runs
- THEN no diagnostics MUST be reported

**Tests:** `crates/rust-total/tests/totality.rs::compliant_total_function_has_no_diagnostics`

### Requirement 2: General arithmetic overflow is deliberately not checked [MUST NOT]

The tool MUST NOT flag `+`, `-` or `*` for overflow risk.

Rationale: without type information, flagging every arithmetic operator would flag nearly all numeric code, making the tool useless. Division and modulo are kept in scope despite the same syntactic-only limitation — a float divisor cannot panic and the tool cannot tell floats from integers — because `/` and `%` are far rarer, so the false-positive rate is tolerable. **The dividing line is frequency, not principle**, and is recorded as such.

**Implementation:** `crates/rust-total/src/checks/panic_freedom.rs`

#### Scenario: Arithmetic in a total function is accepted

- GIVEN a `#[mvl::total]` function whose body performs `a + b * c`
- WHEN the panic-freedom check runs
- THEN no diagnostic MUST be reported, even though the expression may overflow at runtime

**Tests:** `crates/rust-total/tests/totality.rs::binary_arithmetic_in_a_total_function_is_accepted`

### Requirement 3: A directly recursive total function requires a `decreases` measure [MUST]

The tool MUST require `#[mvl::decreases(measure)]` on any `#[mvl::total]` function that directly calls itself.

The tool MUST check the attribute's **presence only**. It MUST NOT be read as proving the measure decreases. Only *direct* self-recursion is detected; mutual recursion between two functions is out of scope.

**Implementation:** `crates/rust-total/src/checks/termination.rs`

#### Scenario: Missing measure on a recursive total function is rejected

- GIVEN a `#[mvl::total]` function that calls itself and carries no `#[mvl::decreases]`
- WHEN the termination check runs
- THEN a `Level::Error` diagnostic MUST be reported
- AND the diagnostic SHOULD suggest adding a measure that strictly decreases

**Tests:** `crates/rust-total/tests/totality.rs::missing_decreases_on_recursive_total_function_is_rejected`

#### Scenario: Presence of a measure satisfies the check

- GIVEN a `#[mvl::total]` recursive function carrying `#[mvl::decreases(n)]`
- WHEN the termination check runs
- THEN no diagnostic MUST be reported
- AND the tool MUST NOT be read as having proven termination

**Tests:** `crates/rust-total/tests/totality.rs::terminating_recursion_with_decreases_is_accepted`, `::non_recursive_total_function_needs_no_decreases`

### Requirement 4: A caller must declare every effect its callees declare [MUST]

The tool MUST reject a call from a function whose declared effect set does not include every effect declared by the callee. Absence of `#[mvl::effect(…)]` MUST be treated identically to an explicit `#[mvl::effect()]` — the empty set — so that not declaring an effect is a positive claim of purity.

Self-recursive calls MUST always be accepted.

**Implementation:** `crates/rust-effect/src/checks.rs`

#### Scenario: A pure function calling an effectful one is rejected

- GIVEN `fn caller()` with no effect attribute calling `#[mvl::effect(Log)] fn callee()`
- WHEN the effect check runs
- THEN a `Level::Error` diagnostic MUST be reported at the call site

**Tests:** `crates/rust-effect/src/checks.rs::pure_calling_effectful_is_an_error`

#### Scenario: An explicitly empty effect set is purity

- GIVEN a function annotated `#[mvl::effect()]` that calls an effectful function
- WHEN the effect check runs
- THEN the call MUST be rejected identically to the unannotated case

**Tests:** `crates/rust-effect/src/checks.rs::explicit_empty_effect_attr_is_pure`

### Requirement 5: Effect matching is flat and same-file [MUST]

Effect sets MUST be compared as flat, exact sets. The tool MUST NOT implement a subsumption hierarchy, effect polymorphism, effect variables, or handler discharge.

Call resolution MUST be same-file free functions only. A call to anything else MUST be silently skipped rather than flagged in either direction.

**Implementation:** `crates/rust-effect/src/checks.rs`

#### Scenario: An unresolvable callee is skipped in both directions

- GIVEN a pure function calling a method or a function defined in another file
- WHEN the effect check runs
- THEN no diagnostic MUST be reported
- AND the caller MUST NOT be credited with having declared any effect either

**Tests:** `crates/rust-effect/src/checks.rs::call_to_unresolvable_function_is_silently_skipped`

---

## Known Limitations

- **`#[mvl::total]` is weaker than its name.** It means "contains no *syntactically obvious* panic construct and, if directly recursive, carries a `decreases` attribute". It does not mean panic-free and it does not mean terminating. Any downstream assurance claim reading `total` as a guarantee is over-reading it.
- **`#[mvl::total]` is not the same predicate as mvl's `total fn`, in either direction.** mvl's `total` is termination-only and opt-out (`partial` is the escape hatch); panic-freedom is a separate requirement (Req 10) checked via refinements. mvl-rust's `total` bundles panic-freedom checking into the same attribute and is opt-in (unannotated functions are unscanned), and its recursion check is presence-only rather than a proved measure. Concretely: `total fn divzero(a: Int, b: Int) -> Int { a / b }` is accepted by mvl (division-by-zero is a runtime panic, not a totality violation) and rejected by mvl-rust; a `#[mvl::decreases(n)]` naming a non-decreasing measure is accepted by mvl-rust (presence-only) and rejected by mvl (SMT-proved). Neither position is wrong — mvl's split follows SPARK/Dafny/Idris/Lean 4 in separating termination from runtime-error freedom, mvl-rust's bundling follows Turner's original 2004 formulation adapted to a language where `/` can trap — but the two attributes are not interchangeable and `total` does not mean the same thing when read across the two projects. ADR-0003 §"Relationship to MVL's `total fn`".
- **Requirement 2's division rule produces false positives on float code**, by construction. Accepted on frequency grounds; fixing it needs types.
- **Effects cannot serve as a purity signal for the solver.** Requirement 4's conflation of *absent* with *declared-empty* means the two are indistinguishable, which is why #45 cannot use `rust-effect` as the purity oracle spec 006's reflexivity rule needs. A tri-state signal would change this decision, not extend it.
- **Nor can an explicit `#[mvl::effect()]`** — Requirement 4's claim is verified only against same-file resolvable calls, so a function annotated pure that reaches an effect through a method or cross-file call is accepted in silence. It is an unverified assertion rather than an established fact, and nothing currently marks it as one. ADR-0008 §3–§4; pinned by `an_explicit_purity_claim_is_not_verified_against_unresolvable_calls`.
- **No cross-procedural effect inference.** A function calling an unresolvable callee may perform arbitrary effects while declaring none, with no diagnostic. Spec 002 narrows this by rejecting `dyn Trait` and unreviewed macros but does not close it.
- **`#[mvl::partial]` is parsed and unclaimed** (#54). The natural reading is the dual of `total` — an explicit opt-out — which would also supply the tri-state distinction #45 wants.
- **Injected runtime assertions would falsify `#[mvl::total]`.** See spec 007 Requirement 5; the collision is introduced by this port and has no upstream answer.

---

## Traceability

| Layer | Artefact |
|---|---|
| **Intent** | #6 (`rust-total`), #9 (`rust-effect`, v1 scope), #45 (purity signal — blocked by Req 4), #54 (`partial` unclaimed) |
| **Specification** | this document |
| **Decision** | ADR-0003; ADR-0001 §1 (attribute carrier), §5 (greenfield rule) |
| **Program** | `crates/rust-total/src/checks/`, `crates/rust-effect/src/checks.rs` |
| **Evidence** | `crates/rust-total/tests/totality.rs` (13 tests), `crates/rust-effect/src/checks.rs::tests` (8 tests), per-tool `tests/verification_mode.rs`, `examples/rust-total-demo/`, `examples/rust-effect-demo/` |
