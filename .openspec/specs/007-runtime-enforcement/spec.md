# 007 — Runtime Enforcement of Residual Obligations

**Domain:** Enforcement / soundness of Γ
**Version:** 0.1.0
**Status:** Fully implemented — Requirements 1–2 landed in #47; 3–5 landed in #53; Requirement 2's "or enforced" clause and Requirement 6 landed in #69
**Date:** 2026-07-29
**Decided by:** ADR-0006 §4–§5

## Overview

Spec 006 settles what the layer stack can prove. This spec settles what happens to what it cannot.

Today the answer is: **nothing**. The `mvl::` attributes are inert pass-throughs, `rust-refine` is an out-of-band lint with no codegen path, and an obligation it cannot discharge is reported as a note that does not fail the build. Worse, the diagnostic text says `"inserting a runtime check"` while inserting nothing — and spec 005's postcondition propagation then treats such an obligation as an established fact.

That combination is the direct cause of #47. This spec is the formalisation of the fix.

**The reference implementation does not inject on residual — it always enforces.** An assertion is emitted for every runtime-checkable contract clause regardless of proof outcome; the static solver is an *early-error layer on top of universal runtime enforcement*, not a filter deciding what to emit. That is what makes its hypothesis propagation sound. This spec adopts the property, not the mechanism, since a lint cannot reach codegen.

### Philosophy

- **`Runtime` means unenforced, everywhere the tool speaks** — in Γ, in diagnostics, and in the assurance report. Until enforcement exists, nothing may claim otherwise.

  **Amended by #69.** Once enforcement exists (#53), a `Runtime` discharge outcome is no longer synonymous with "unenforced" — a call/return-site obligation the static solver leaves `Runtime` may still be backed by a real `assert!`. The stronger, still-accurate principle: **an obligation may claim no more than it has actually established, static or enforced**, and `Warrant` (#69) is what lets the tool say precisely which one, everywhere it speaks, instead of collapsing both into "unverified".
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

**Implementation:** `crates/rust-refine/src/checks.rs`

#### Scenario: A runtime outcome is described honestly

- GIVEN an obligation whose outcome is a runtime check
- WHEN the diagnostic is rendered
- THEN it MUST NOT claim a runtime assertion was inserted
- AND it MUST name the hypotheses available at that point

**Tests:** `crates/rust-refine/tests/call_sites.rs::a_runtime_outcome_does_not_claim_a_check_was_inserted`

### Requirement 2: An unenforced postcondition must not enter the hypothesis context [MUST]

A callee's `ensures` MUST NOT be propagated into a caller's Γ unless the callee's own return-site obligation was discharged, or the postcondition is enforced under Requirement 3.

This is the invariant stated in ADR-0006 §5:

> A fact is admitted to Γ only if it has been established, or is an obligation some other program point is required to discharge.

**Implementation:** `crates/rust-refine/src/checks.rs`

#### Scenario: A runtime-only postcondition proves nothing downstream

- GIVEN `#[mvl::ensures(result > 100)] fn suspicious(b: i64) -> i64 { b & 15 }`, whose return-site obligation cannot be discharged, carrying `#[mvl::unchecked]`
- AND a caller binding `let y = suspicious(b);` then calling `#[mvl::requires(v > 50)] fn needs_big(v: i64)`
- WHEN the call-site obligation is discharged
- THEN it MUST NOT be reported as `Proven`
- AND `result > 100` MUST NOT appear in the caller's Γ

**Tests:** `crates/rust-refine/tests/call_sites.rs::an_unenforced_postcondition_does_not_enter_gamma`, `::an_established_postcondition_still_enters_gamma`, `::a_violated_postcondition_does_not_enter_gamma_either`

#### Scenario: The "or enforced" clause — an undischarged-but-enforced postcondition now propagates

**Landed by #69.** The same fixture as above, minus `#[mvl::unchecked]`: `suspicious` carries `#[mvl::ensures]` and is not opted out, so `mvl-macros` (#53) really does assert it on every return, regardless of what the static solver concluded about that return site. ADR-0006 §5's soundness argument for enforcement is unconditional — "either the postcondition holds, or the process aborted" — so this is not a precision improvement bolted on afterward, it is the condition Requirement 2 was always written to permit ("or the postcondition is enforced under Requirement 3") and that #53 alone could not yet satisfy, since nothing propagated on enforcement grounds until this gate relaxed.

- GIVEN the same `suspicious`/`needs_big`/`caller` fixture, without `#[mvl::unchecked]`
- WHEN the call-site obligation is discharged
- THEN it MUST be usable to close the caller's obligation
- AND the outcome MUST be reported per Requirement 6 — resting on `suspicious`'s enforcement, not a proof outright

This applies **regardless of the callee's own return-site outcome** — `Proven`, `Runtime`, or even a demonstrated `Violated` are all safe to propagate from once the function is enforced, since the same "abort instead of a bad value" backstop covers all three; only `#[mvl::unchecked]` forfeits it.

**Tests:** `crates/rust-refine/tests/call_sites.rs::an_enforced_but_undischarged_postcondition_now_enters_gamma`, `::a_violated_but_enforced_postcondition_still_propagates_soundly`

### Requirement 3: Contract attributes enforce their predicates at runtime [MUST]

`#[mvl::requires(p)]` MUST expand to a check of `p` on entry. `#[mvl::ensures(p)]` MUST expand to a wrapping of the whole function body that binds the produced value and checks `p` before returning it.

The check MUST use an unconditional assertion, not a debug-only one, and MUST NOT be elidable by build profile. A check that compiles out under release would void the assumption Requirement 2 permits.

Wrapping the whole body — rather than only the trailing expression — MUST cover explicit `return` paths.

**Implementation:** `crates/mvl-macros/src/inject.rs::inject_requires`, `::inject_ensures`

#### Scenario: An explicit return is checked

- GIVEN `#[mvl::ensures(result > 100)] fn f(x: i64) -> i64 { if x > 5 { return x } 200 }` called with `x = 7`
- WHEN the expanded function runs
- THEN the postcondition MUST be checked on the `return x` path
- AND the program MUST abort rather than return a value violating the contract

**Tests:** `crates/mvl/tests/enforcement.rs::a_violating_explicit_return_aborts`, `crates/mvl-macros/src/inject.rs::tests::explicit_return_is_instrumented`

**Known gap: the `?` operator is not an instrumented return point.** `?`'s early return is invisible to `syn` — there is no `Expr::Return` node to rewrite, only an `Expr::Try` wrapping the fallible expression. A function that returns early via `foo()?` produces a value `ensures` never checks. This is not a new soundness hole: `rust-refine`'s own static checker (`crates/rust-refine/src/checks.rs::visit_expr_return`) has the same blind spot, so Γ never assumed that path was covered either. It is, however, a real runtime-enforcement gap, tracked rather than silently left — see Known Limitations below.

**Tests:** `crates/mvl/tests/enforcement.rs::a_violating_early_return_via_try_operator_does_not_abort` (pins the current gap so a future change to this behavior is deliberate, not silent)

#### Scenario: Enforcement is not elided in release

- GIVEN a crate built in release mode
- WHEN a contract predicate is violated at runtime
- THEN the check MUST still fire

Not re-verified by actually building in release mode — that would mean spawning a real `cargo build --release`, which belongs with an integration-style check rather than a unit test. The guarantee is structural instead: `inject.rs` emits a bare `assert!` with no `cfg(debug_assertions)` gate anywhere in the module, which the tests below pin directly. A profile-conditional check would surface as a change to what those tests assert, not as a test passing in one profile and failing in another.

**Tests:** `crates/mvl-macros/src/inject.rs::tests::requires_uses_assert_not_debug_assert`, `::ensures_uses_assert_not_debug_assert`

### Requirement 4: Every predicate the grammar can express must be runtime-evaluable, and none may be silently excluded [MUST]

**Revised by #53.** This requirement originally assumed a bounded quantifier cannot be evaluated at runtime and must be excluded from both enforcement and Γ, mirroring upstream's `is_runtime_checkable`. That assumption does not hold for this grammar, and #53 found this out by implementing the exclusion's alternative rather than the exclusion itself.

`Predicate::Forall`/`Exists` bounds are **literal integers**, checked at parse time (`crates/mvl-rust-core/src/attrs/predicate.rs`) — there is nothing to evaluate to get them, so they can be emitted directly into a runtime loop: `forall i in [lo..hi] . body` lowers to `(lo..=hi).all(|i| body)`, `exists` to `.any(...)`. Verified against the real compliant demo's `require_dense_fleet`, whose `requires` is exactly this shape over a same-file call.

There is also no ghost-state or pre-state (`old(...)`) construct anywhere in the `Predicate` grammar — confirmed by inspection, not merely assumed — so the second half of the original concern names a case that cannot arise from any predicate this parser produces. Should the grammar later grow one, *that* addition would need this requirement re-opened; today it has nothing to apply to.

The consequence: **every predicate in the grammar is runtime-evaluable**, so Requirement 2's "or enforced" permission is not conditioned on a partition between checkable and unchecked predicate shapes — there is only one shape, and it is always checkable.

**Implementation:** `crates/mvl-macros/src/inject.rs::predicate_to_bool`

#### Scenario: A quantified postcondition is checked, not assumed away

- GIVEN `#[mvl::ensures(forall i in [0..10] . result > i)]`
- WHEN the attribute expands
- THEN a runtime check MUST be emitted covering every value in the range
- AND a violation at any value in the range MUST cause the check to fail

**Tests:** `crates/mvl/tests/enforcement.rs::quantified_postcondition_is_checked_at_runtime`, `crates/mvl/tests/enforcement.rs::quantified_postcondition_catches_a_single_bad_value`

### Requirement 5: A function may opt out of enforcement, and opting out excludes it from Γ [MUST]

The tool MUST provide a per-function opt-out from runtime enforcement.

**The `#[mvl::total]` collision this requirement was written to resolve did not need the opt-out, and #53 found that out by building the opt-out anyway and then not needing it for the case that motivated it.** The rationale below describes the *original* framing; the paragraph after it describes what actually shipped and why the two differ.

Original rationale: an injected assertion makes a `#[mvl::total]` function panicking, and spec 003's totality checker asserts it is not. The two tools would otherwise make contradictory claims about the same function, and only by allow-list accident would the contradiction go unreported.

**What #53 actually did:** ADR-0003 §2's amendment redefines `#[mvl::total]`'s panic-freedom claim as scoped to *accidental* crash sources — `.unwrap()`, raw indexing, a bare `panic!` — and a contract assert is a documented check, not one of those. So a `#[mvl::total]` function carrying `#[mvl::requires]`/`#[mvl::ensures]` is not a contradiction to resolve per-function; it never conflicts, for any such function, with no opt-out needed. `rust-total`'s panic-freedom checker takes no action on either attribute, pinned by `requires_and_ensures_on_a_total_function_are_not_flagged` (`rust-total`).

The opt-out attribute (`#[mvl::unchecked]`) still exists, and is still worth having — but for a narrower reason than the original rationale: an author who wants a *specific* function's `requires`/`ensures` to keep the pre-#53, fully pass-through behavior (no enforcement at all, regardless of `#[mvl::total]`), rather than for resolving a conflict that turned out not to exist.

**Implementation:** `crates/mvl-macros/src/lib.rs::unchecked`

#### Scenario: A total function's contract does not conflict with its panic-freedom claim

- GIVEN a `#[mvl::total]` function carrying `#[mvl::requires]`/`#[mvl::ensures]`, with no `#[mvl::unchecked]`
- WHEN both tools run
- THEN `rust-total` MUST NOT flag the function for the assert its contract attributes inject
- AND `#[mvl::total]`'s panic-freedom claim MUST be understood as scoped to accidental crash sources, not contract checks

**Tests:** `crates/rust-total/tests/totality.rs::requires_and_ensures_on_a_total_function_are_not_flagged`, `crates/mvl/tests/enforcement.rs::a_total_function_with_a_satisfied_contract_runs_normally`, `::a_total_function_still_enforces_its_contract`

#### Scenario: An opted-out function is not enforced, in either attribute order

- GIVEN a function carrying `#[mvl::unchecked]` above or below `#[mvl::ensures]`
- WHEN the attributes expand
- THEN no runtime check MUST be emitted for that function's `requires`/`ensures`, regardless of which order the attributes were written in

**Tests:** `crates/mvl/tests/enforcement.rs::unchecked_suppresses_enforcement_regardless_of_attribute_order`

### Requirement 6: A proof resting on an enforced premise must not be reported as a proof [MUST]

Where an obligation is discharged against a premise that is runtime-enforced rather than statically established, the reported provenance MUST distinguish it from an obligation proven outright.

**Landed by #69** via a new wire-facing discriminator, [`Warrant`](../../../crates/mvl-rust-core/src/solver/mod.rs) (`ObligationRecord.warrant`, assurance schema `1.2`) — a third axis alongside `kind` (which question) and `layer` (did static reasoning close it): `Proof` (a real, untainted proof), `Enforcement { premises }` (rests on the named functions' runtime enforcement), or `None` (neither). `ObligationRecord::is_proof()` requires `Warrant::Proof` in addition to its existing checks, so nothing downstream that already calls `is_proof()` needs to change to get this protection.

**Exactness, not a conservative over-approximation.** `rust-refine` re-discharges the goal against only the untainted hypotheses first: if that alone still proves it, every enforced-not-proven fact present in Γ was a red herring and the outcome is a real `Proof` — a proof that merely *coexists* with an unrelated enforced fact is not swept into `Enforcement`. Where enforcement genuinely is needed, leave-one-out against the full hypothesis set finds every *individually* necessary premise exactly (sound by monotonicity of interval/Fourier–Motzkin reasoning); the one documented gap is two independently-sufficient enforced premises used as alternatives rather than a conjunction, where the reported set is sufficient but not guaranteed globally smallest — see `crates/rust-refine/src/checks.rs::FoundObligation::warrant_for_proof`'s doc comment.

**Implementation:** `crates/mvl-rust-core/src/solver/mod.rs::Warrant`, `crates/mvl-rust-core/src/assurance/schema.rs::ObligationRecord`, `crates/rust-refine/src/checks.rs::FoundObligation::warrant`

#### Scenario: An enforced premise taints the outcome it supports

- GIVEN a call-site obligation closed using a postcondition that is enforced rather than proven
- WHEN the outcome is reported
- THEN it MUST NOT be presented identically to an obligation closed from statically established facts
- AND the report MUST name exactly which function's enforcement it rests on

**Tests:** `crates/rust-refine/tests/call_sites.rs::an_enforced_but_undischarged_postcondition_now_enters_gamma`, `::a_violated_but_enforced_postcondition_still_propagates_soundly`, `::two_jointly_necessary_enforced_premises_are_both_named`

#### Scenario: A proof that never used an enforced premise stays a proof

- GIVEN Γ contains an enforced-not-proven fact from one call, and a separate call-site obligation is discharged using only other, genuinely established hypotheses
- WHEN the outcome is reported
- THEN it MUST be `Warrant::Proof`, not `Enforcement`, even though an enforced fact was present elsewhere in Γ

**Tests:** `crates/rust-refine/tests/call_sites.rs::a_red_herring_enforced_hypothesis_does_not_taint_an_unrelated_proof`, `::an_established_postcondition_still_enters_gamma`

---

## Known Limitations

- **All scenarios in this spec are now evidenced (#53, #69).**
- **Requirement 3 changes the runtime behaviour of existing annotated code.** It contradicts the facade crate's documented "unaffected by whether this crate is even a dependency", breaks the passthrough test by design, and amends ADR-0001 §2.
- **Enforcement becomes dependent on the `mvl` crate being a dependency.** Dropping it produces an unresolved-attribute compile error — fail-loud, therefore acceptable.
- **An abort replaces a silent wrong answer.** For the target domains that is the right trade, but it is a stated decision. There is no profile in which a check Γ depends on may be elided.
- **Declaration-site obligations have no runtime analogue.** Coherence asks whether a predicate is satisfiable; there is no program point to assert at. Those stay static-only, which is adequate — a self-contradictory `requires` is already an error.
- **Requirements 1 and 2 recover soundness without any injection**, by declining to propagate what is not established, at a cost in precision. They are sequenced first for that reason.
- **The reference implementation's own enforcement has at least seven holes** — explicit `return` paths, inline parameter refinements, return-type refinements, trait-impl methods, instrumented builds, one backend entirely, and predicates that fail to lower. Requirement 3 is therefore implementing an intent, not porting a mechanism, and can exceed it.
- **The `?` operator is an uninstrumented return point.** `ensures` covers the tail expression and every explicit `return`, but a `?`-driven early return produces a value with no check. Consistent with `rust-refine`'s own static checker (also blind to `?`), so no unsound Γ claim results — but it is a real, silent enforcement gap for any function using `?`. Pinned by `crates/mvl/tests/enforcement.rs::a_violating_early_return_via_try_operator_does_not_abort` rather than left to drift unnoticed. Closing it is future work, not scoped to #53/#69.
- **`Warrant::Enforcement`'s `premises` is not guaranteed globally minimal.** When two enforced-not-proven facts are independently-sufficient alternatives rather than a conjunction (either alone would close the goal), `rust-refine` reports a sufficient witness set built by adding candidates back in scan order, not the smallest possible one. The yes/no question ("does this rest on enforcement at all") is exact regardless; only the *exact membership* of `premises` in this specific redundant-alternative case is best-effort. See `crates/rust-refine/src/checks.rs::FoundObligation::warrant_for_proof`'s doc comment.

---

## Traceability

| Layer | Artefact |
|---|---|
| **Intent** | #47 (Γ invariant and the honesty fixes — Reqs 1–2), #53 (proc-macro enforcement — Reqs 3–5), #69 (relaxed propagation + taint — Req 2's enforced clause, Req 6 — landed), #48 (return-point doc invariant) |
| **Specification** | this document; spec 005 (the Γ it protects), spec 006 (what produces residuals), spec 008 (how outcomes are reported) |
| **Decision** | ADR-0006 §4 (mechanism, and why source rewriting was rejected), §5 (the invariant and its five conditions); ADR-0001 §2 (amended by Req 3); ADR-0003 §2 (the totality collision Req 5's shipped resolution rests on) |
| **Program** | `crates/mvl-macros/src/lib.rs`, `crates/mvl-macros/src/inject.rs` (Reqs 3–5); `crates/rust-refine/src/checks.rs::FnFacts`, `::ClosureKind`, `::return_site_closure`, `::CallSiteScan::propagate_postcondition`, `::FoundObligation::warrant` (Req 2's enforced clause, #69); `crates/mvl-rust-core/src/solver/mod.rs::Warrant`, `crates/mvl-rust-core/src/assurance/schema.rs::ObligationRecord` (Req 6, #69); `crates/mvl-rust-core/src/attrs.rs::MvlAttr::Unchecked` (lets `rust-refine` see `#[mvl::unchecked]` at all, #69) |
| **Evidence** | `crates/mvl-macros/src/inject.rs::tests`, `crates/mvl/tests/enforcement.rs`, `crates/rust-total/tests/totality.rs` (Reqs 3–5); `crates/rust-refine/tests/call_sites.rs` (the Γ soundness + `Warrant` sections), `crates/mvl-rust-core/tests/schema_stability.rs` (schema `1.2`) (Req 2's enforced clause + Req 6, #69) |
