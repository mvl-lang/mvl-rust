# 007 — Runtime Enforcement of Residual Obligations

**Domain:** Enforcement / soundness of Γ
**Version:** 0.1.0
**Status:** Not implemented — every requirement below is `(planned)`
**Date:** 2026-07-29
**Decided by:** ADR-0006 §4–§5

## Overview

Spec 006 settles what the layer stack can prove. This spec settles what happens to what it cannot.

Today the answer is: **nothing**. The `mvl::` attributes are inert pass-throughs, `rust-refine` is an out-of-band lint with no codegen path, and an obligation it cannot discharge is reported as a note that does not fail the build. Worse, the diagnostic text says `"inserting a runtime check"` while inserting nothing — and spec 005's postcondition propagation then treats such an obligation as an established fact.

That combination is the direct cause of #47. This spec is the formalisation of the fix.

**The reference implementation does not inject on residual — it always enforces.** An assertion is emitted for every runtime-checkable contract clause regardless of proof outcome; the static solver is an *early-error layer on top of universal runtime enforcement*, not a filter deciding what to emit. That is what makes its hypothesis propagation sound. This spec adopts the property, not the mechanism, since a lint cannot reach codegen.

### Philosophy

- **`Runtime` means unenforced, everywhere the tool speaks** — in Γ, in diagnostics, and in the assurance report. Until enforcement exists, nothing may claim otherwise.
- **Enforcement is callee-side.** A check at the callee covers every caller, including ones a same-file scan cannot resolve. No call-site scheme has that property.
- **Injection buys soundness, not the right to call it a proof.** An obligation closed against a runtime-enforced premise is not `Proven`.

---

## RFC 2119 Keywords

The key words "MUST", "MUST NOT", "SHOULD", "SHOULD NOT", and "MAY" in this document are to be interpreted as described in [RFC 2119](https://datatracker.ietf.org/doc/html/rfc2119).

---

## Requirements

### Requirement 1: An undischarged obligation must not be reported as enforced [MUST]

A diagnostic for an obligation whose outcome is a runtime check MUST NOT assert that a check has been inserted. It MUST report the obligation as undischarged and name what was known at that point.

This requirement is independent of every other in this spec and MUST be satisfiable without any injection.

**Implementation:** `crates/rust-refine/src/checks.rs` (planned — #47)

**Tests:** `crates/rust-refine/tests/call_sites.rs` (planned — #47)

#### Scenario: A runtime outcome is described honestly

- GIVEN an obligation whose outcome is a runtime check
- WHEN the diagnostic is rendered
- THEN it MUST NOT claim a runtime assertion was inserted
- AND it MUST name the hypotheses available at that point

### Requirement 2: An unenforced postcondition must not enter the hypothesis context [MUST]

A callee's `ensures` MUST NOT be propagated into a caller's Γ unless the callee's own return-site obligation was discharged, or the postcondition is enforced under Requirement 3.

This is the invariant stated in ADR-0006 §5:

> A fact is admitted to Γ only if it has been established, or is an obligation some other program point is required to discharge.

**Implementation:** `crates/rust-refine/src/checks.rs` (planned — #47)

**Tests:** `crates/rust-refine/tests/call_sites.rs` (planned — #47)

#### Scenario: A runtime-only postcondition proves nothing downstream

- GIVEN `#[mvl::ensures(result > 100)] fn suspicious(b: i64) -> i64 { b & 15 }`, whose return-site obligation cannot be discharged
- AND a caller binding `let y = suspicious(b);` then calling `#[mvl::requires(v > 50)] fn needs_big(v: i64)`
- WHEN the call-site obligation is discharged
- THEN it MUST NOT be reported as `Proven`
- AND `result > 100` MUST NOT appear in the caller's Γ

### Requirement 3: Contract attributes enforce their predicates at runtime [MUST]

`#[mvl::requires(p)]` MUST expand to a check of `p` on entry. `#[mvl::ensures(p)]` MUST expand to a wrapping of the whole function body that binds the produced value and checks `p` before returning it.

The check MUST use an unconditional assertion, not a debug-only one, and MUST NOT be elidable by build profile. A check that compiles out under release would void the assumption Requirement 2 permits.

Wrapping the whole body — rather than only the trailing expression — MUST cover explicit `return` paths.

**Implementation:** `crates/mvl-macros/src/lib.rs` (planned — #53)

**Tests:** `crates/mvl/tests/passthrough.rs` (planned — #53)

#### Scenario: An explicit return is checked

- GIVEN `#[mvl::ensures(result > 100)] fn f(x: i64) -> i64 { if x > 5 { return x } 200 }` called with `x = 7`
- WHEN the expanded function runs
- THEN the postcondition MUST be checked on the `return x` path
- AND the program MUST abort rather than return a value violating the contract

#### Scenario: Enforcement is not elided in release

- GIVEN a crate built in release mode
- WHEN a contract predicate is violated at runtime
- THEN the check MUST still fire

### Requirement 4: Predicates that cannot be evaluated at runtime must be excluded from both enforcement and Γ [MUST]

A predicate that cannot be evaluated in the callee's post-state — a bounded quantifier, ghost state, or a reference to a pre-state value that was not captured — MUST NOT be expanded into a runtime check.

Such a predicate MUST also be excluded from Γ, since Requirement 2's permission depends on enforcement existing.

**Implementation:** `crates/mvl-macros/src/lib.rs` (planned — #53)

**Tests:** `crates/mvl-rust-core/tests/attrs.rs` (planned — #53)

#### Scenario: A quantified postcondition is neither checked nor assumed

- GIVEN `#[mvl::ensures(forall i in [0..10] . result > i)]`
- WHEN the attribute expands
- THEN no runtime check MUST be emitted for it
- AND the postcondition MUST NOT be propagated into any caller's Γ

### Requirement 5: A function may opt out of enforcement, and opting out excludes it from Γ [MUST]

The tool MUST provide a per-function opt-out from runtime enforcement.

Rationale: an injected assertion makes a `#[mvl::total]` function panicking, and spec 003's totality checker asserts it is not. The two tools would otherwise make contradictory claims about the same function, and only by allow-list accident would the contradiction go unreported.

A function that opts out MUST be excluded from Requirement 2's propagation permission — it fails the condition that every function whose postcondition can enter Γ is enforced.

**Implementation:** `crates/mvl-macros/src/lib.rs` (planned — #53)

**Tests:** `crates/rust-total/tests/totality.rs` (planned — #53)

#### Scenario: A total function with a residual obligation does not silently become panicking

- GIVEN a `#[mvl::total]` function carrying an `#[mvl::ensures]` whose obligation is undischarged
- WHEN both tools run
- THEN either the function MUST opt out of enforcement, or the conflict MUST be reported
- AND `#[mvl::total]` MUST NOT be reported as satisfied for a body containing an injected assertion

### Requirement 6: A proof resting on an enforced premise must not be reported as a proof [MUST]

Where an obligation is discharged against a premise that is runtime-enforced rather than statically established, the reported provenance MUST distinguish it from an obligation proven outright.

**Implementation:** `crates/mvl-rust-core/src/assurance/schema.rs` (planned — #53)

**Tests:** `crates/mvl-rust-core/tests/schema_stability.rs` (planned — #53)

#### Scenario: An enforced premise taints the outcome it supports

- GIVEN a call-site obligation closed using a postcondition that is enforced rather than proven
- WHEN the outcome is reported
- THEN it MUST NOT be presented identically to an obligation closed from statically established facts

---

## Known Limitations

- **Nothing here is implemented.** Every requirement is `(planned)` and excluded from the assurance totals by design — they describe intended architecture, not current behaviour.
- **Requirement 3 changes the runtime behaviour of existing annotated code.** It contradicts the facade crate's documented "unaffected by whether this crate is even a dependency", breaks the passthrough test by design, and amends ADR-0001 §2.
- **Enforcement becomes dependent on the `mvl` crate being a dependency.** Dropping it produces an unresolved-attribute compile error — fail-loud, therefore acceptable.
- **An abort replaces a silent wrong answer.** For the target domains that is the right trade, but it is a stated decision. There is no profile in which a check Γ depends on may be elided.
- **Declaration-site obligations have no runtime analogue.** Coherence asks whether a predicate is satisfiable; there is no program point to assert at. Those stay static-only, which is adequate — a self-contradictory `requires` is already an error.
- **Requirements 1 and 2 recover soundness without any injection**, by declining to propagate what is not established, at a cost in precision. They are sequenced first for that reason.
- **The reference implementation's own enforcement has at least seven holes** — explicit `return` paths, inline parameter refinements, return-type refinements, trait-impl methods, instrumented builds, one backend entirely, and predicates that fail to lower. Requirement 3 is therefore implementing an intent, not porting a mechanism, and can exceed it.

---

## Traceability

| Layer | Artefact |
|---|---|
| **Intent** | #47 (Γ invariant and the honesty fixes — Reqs 1–2), #53 (proc-macro enforcement — Reqs 3–6), #48 (return-point doc invariant) |
| **Specification** | this document; spec 005 (the Γ it protects), spec 006 (what produces residuals), spec 008 (how outcomes are reported) |
| **Decision** | ADR-0006 §4 (mechanism, and why source rewriting was rejected), §5 (the invariant and its five conditions); ADR-0001 §2 (amended by Req 3); ADR-0003 (the totality collision Req 5 resolves) |
| **Program** | `crates/mvl-macros/src/lib.rs` (currently inert pass-throughs), `crates/rust-refine/src/checks.rs` |
| **Evidence** | none yet — this spec's scenarios are the acceptance criteria for #47 and #53 |
