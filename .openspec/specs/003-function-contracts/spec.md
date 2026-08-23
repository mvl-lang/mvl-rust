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
                                            (a parameter; each recursive call's descent is
                                            discharged through the native solver rust-refine
                                            also uses — ADR-0009)
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

Inside a function annotated `#[mvl::total]`, the tool MUST reject `.unwrap()`, `.expect(…)`, the `panic!`/`todo!`/`unimplemented!`/`unreachable!` macros, raw indexing (`xs[i]`), and division or modulo.

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

### Requirement 3: A directly recursive total function requires a provably decreasing `decreases` measure [MUST]

The tool MUST require `#[mvl::decreases(measure)]` on any `#[mvl::total]` function that directly calls itself.

**Amended by ADR-0009**, superseding this requirement's original presence-only wording. `measure` MUST be a bare identifier naming one of the function's own parameters. At every direct recursive call, the tool MUST build the entailment obligation `<call's argument> < <measure>` and discharge it via `mvl_rust_core::solver::native::discharge_entailment` (the same native `L1`–`L4` linear-arithmetic backend `rust-refine` uses for `requires`/`ensures`), supplying the function's own `#[mvl::requires(...)]` clauses as hypotheses. The tool MUST reject the declaration if `measure` is not a bare parameter identifier, and MUST reject each recursive call whose obligation discharges as `Violated` or `Runtime` (unproven). Only *direct* self-recursion is detected; mutual recursion between two functions is out of scope. This reaches exactly as far as the native solver's linear-arithmetic fragment does: subtraction of a positive literal or of a symbolically-bounded amount (via a `requires` hypothesis) is provable; division/modulo is outside the solver's linear system entirely and is never provable this way (ADR-0009 §2).

**Implementation:** `crates/rust-total/src/checks/termination.rs`

#### Scenario: Missing measure on a recursive total function is rejected

- GIVEN a `#[mvl::total]` function that calls itself and carries no `#[mvl::decreases]`
- WHEN the termination check runs
- THEN a `Level::Error` diagnostic MUST be reported
- AND the diagnostic SHOULD suggest adding a measure that strictly decreases

**Tests:** `crates/rust-total/tests/totality.rs::missing_decreases_on_recursive_total_function_is_rejected`

#### Scenario: A measure whose descent the solver proves is accepted

- GIVEN a `#[mvl::total]` recursive function carrying `#[mvl::decreases(n)]`
- AND every direct recursive call's argument for `n` discharges `<argument> < n` as `Proven`
- WHEN the termination check runs
- THEN no diagnostic MUST be reported

**Tests:** `crates/rust-total/tests/totality.rs::terminating_recursion_with_decreases_is_accepted`, `::non_recursive_total_function_needs_no_decreases`, `::a_symbolic_decrement_is_proved_given_a_requires_hypothesis`

#### Scenario: A measure that does not provably decrease is rejected

- GIVEN a `#[mvl::total]` recursive function carrying `#[mvl::decreases(n)]`
- AND at least one direct recursive call's argument for `n` discharges `<argument> < n` as `Violated` or `Runtime`
- WHEN the termination check runs
- THEN a `Level::Error` diagnostic MUST be reported for that call site

**Tests:** `crates/rust-total/tests/totality.rs::non_decreasing_measure_is_rejected`, `::division_is_never_provably_decreasing`, `::a_symbolic_decrement_without_a_positivity_hypothesis_is_rejected`

#### Scenario: A measure that isn't a bare parameter is rejected

- GIVEN a `#[mvl::total]` recursive function carrying `#[mvl::decreases(...)]` where the measure is not a single identifier naming a parameter (e.g. a computed expression)
- WHEN the termination check runs
- THEN a `Level::Error` diagnostic MUST be reported

**Tests:** `crates/rust-total/tests/totality.rs::non_parameter_measure_is_rejected`

#### Scenario: A measure shadowed in the function body is rejected

- GIVEN a `#[mvl::total]` recursive function carrying `#[mvl::decreases(n)]`
- AND `n` is rebound somewhere in the function body (a `let`, a closure parameter, a match arm, a for-loop pattern)
- WHEN the termination check runs
- THEN a `Level::Error` diagnostic MUST be reported, regardless of whether the recursive call's argument would otherwise discharge as `Proven`

**Tests:** `crates/rust-total/tests/totality.rs::a_shadowed_measure_is_rejected_even_though_it_looks_decreasing`

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

### Requirement 6: A `while`/`loop` in a total function requires a provably decreasing `loop_decreases!` measure [MUST]

**Added by ADR-0010**, closing a gap Requirement 3 never covered: direct recursion was checked, but a `while`/`loop` construct was not, at all.

The tool MUST require `mvl::loop_decreases!(measure)` as the first statement of any `while`/`loop` expression's body inside a `#[mvl::total]` function. Because a real attribute macro cannot legally attach to a `while`/`loop` expression on stable Rust (ADR-0010), `loop_decreases!` is a function-like macro invocation, not an attribute, unlike `#[mvl::decreases(measure)]` on a recursive function.

`measure` MUST be a bare identifier. The tool MUST reject the loop if `measure` is not a bare identifier, or if it is rebound anywhere in the loop body (same rule and rationale as Requirement 3's shadowing scenario). The tool MUST find every assignment to `measure` anywhere in the loop body (any nesting depth) and require exactly one; if more than one exists, or if the one that exists is not a direct, unconditional, top-level statement of the loop body, the tool MUST reject the loop. Given exactly one unconditional top-level assignment, the tool MUST build the entailment obligation `<measure's value after the assignment> < <measure>` and discharge it the same way Requirement 3 discharges a recursive call's descent — via `mvl_rust_core::solver::native::discharge_entailment`, with the function's own `#[mvl::requires(...)]` clauses as hypotheses — and MUST reject the loop if the result is `Violated` or `Runtime`.

Only `while` and `loop` expressions are detected. `for` loops, loops inside `impl` methods, and reasoning across multiple mutations of the same measure are out of scope.

**Implementation:** `crates/rust-total/src/checks/loop_termination.rs`

#### Scenario: A loop with no `loop_decreases!` marker is rejected

- GIVEN a `#[mvl::total]` function containing a `while`/`loop` expression with no `mvl::loop_decreases!(...)` as its body's first statement
- WHEN the loop-termination check runs
- THEN a `Level::Error` diagnostic MUST be reported

**Tests:** `crates/rust-total/tests/totality.rs::loop_missing_decreases_marker_is_rejected`

#### Scenario: A loop measure that isn't a bare identifier is rejected

- GIVEN a `#[mvl::total]` function containing a `while`/`loop` whose `mvl::loop_decreases!(...)` argument is not a single identifier (e.g. a computed expression)
- WHEN the loop-termination check runs
- THEN a `Level::Error` diagnostic MUST be reported

**Tests:** `crates/rust-total/tests/totality.rs::loop_non_identifier_measure_is_rejected`

#### Scenario: A loop whose measure provably decreases is accepted

- GIVEN a `#[mvl::total]` function containing a `while`/`loop` whose body starts with `mvl::loop_decreases!(n)`
- AND the body's only assignment to `n` is a direct, unconditional, top-level statement whose new value discharges `<new value> < n` as `Proven`
- WHEN the loop-termination check runs
- THEN no diagnostic MUST be reported

**Tests:** `crates/rust-total/tests/totality.rs::loop_with_literal_decrement_is_accepted`, `::loop_with_symbolic_decrement_is_proved_given_a_requires_hypothesis`, `::nested_loops_are_each_checked_independently`

#### Scenario: A loop whose measure does not provably decrease is rejected

- GIVEN a `#[mvl::total]` function containing a `while`/`loop` whose body starts with `mvl::loop_decreases!(n)`
- AND the body's only assignment to `n` discharges `<new value> < n` as `Violated` or `Runtime`
- WHEN the loop-termination check runs
- THEN a `Level::Error` diagnostic MUST be reported

**Tests:** `crates/rust-total/tests/totality.rs::loop_division_is_never_provably_decreasing`, `::loop_symbolic_decrement_without_a_positivity_hypothesis_is_rejected`

#### Scenario: A conditional or duplicated assignment to the measure is rejected

- GIVEN a `#[mvl::total]` function containing a `while`/`loop` whose body starts with `mvl::loop_decreases!(n)`
- AND the only assignment to `n` is nested inside an `if`/`match`/inner loop, OR `n` is assigned more than once anywhere in the body
- WHEN the loop-termination check runs
- THEN a `Level::Error` diagnostic MUST be reported

**Tests:** `crates/rust-total/tests/totality.rs::loop_conditional_mutation_is_rejected`, `::loop_multiple_mutations_are_rejected`

#### Scenario: A measure shadowed in the loop body is rejected

- GIVEN a `#[mvl::total]` function containing a `while`/`loop` whose body starts with `mvl::loop_decreases!(n)`
- AND `n` is rebound somewhere in the loop body
- WHEN the loop-termination check runs
- THEN a `Level::Error` diagnostic MUST be reported, regardless of whether the assignment found would otherwise discharge as `Proven`

**Tests:** `crates/rust-total/tests/totality.rs::loop_shadowed_measure_is_rejected`

### Requirement 7: Silently discarding a call's result in a `#[mvl::total]` body is rejected [MUST]

**Added by #117**, closing a gap Requirement 1 never covered: a hidden panic is one kind of hidden exit path, but silently discarding a fallible result before anyone inspects it is another — a `Result::Err` swallowed this way never surfaces at all, which is strictly worse than a panic that at least halts visibly.

Inside a function annotated `#[mvl::total]`, the tool MUST reject:
- `let _ = <call>;` where `<call>` is a function call, method call, `?`-expression, or `.await` expression (a bare `let _ = <identifier>;` discarding an already-bound value is not flagged — see Known Limitations)
- `drop(<call>)` / `std::mem::drop(<call>)` / `mem::drop(<call>)` where the argument is one of the same call shapes
- `.map(|_| ())` — a `.map` call whose sole argument is a closure with only wildcard parameters and a unit-literal body

Same syntactic-only limitation as Requirement 1: without type information the tool cannot tell a `Result`/`Option`-returning call from one returning `()` or a plain value, so this is deliberately over-inclusive.

**Implementation:** `crates/rust-total/src/checks/swallow.rs`

#### Scenario: `let _ = <call>` is rejected

- GIVEN a `#[mvl::total]` function containing `let _ = write_config();`
- WHEN the swallow check runs
- THEN a `Level::Error` diagnostic MUST be reported

**Tests:** `crates/rust-total/tests/totality.rs::let_underscore_call_is_rejected`

#### Scenario: `let _ = <bare identifier>` is not rejected

- GIVEN a `#[mvl::total]` function containing `let _ = x;` where `x` is an already-bound variable, not a call
- WHEN the swallow check runs
- THEN no diagnostic MUST be reported

**Tests:** `crates/rust-total/tests/totality.rs::let_underscore_bare_variable_is_not_rejected`

#### Scenario: `drop(<call>)` / `mem::drop(<call>)` is rejected

- GIVEN a `#[mvl::total]` function calling `drop(<call>)` or `std::mem::drop(<call>)` on a call's result
- WHEN the swallow check runs
- THEN a `Level::Error` diagnostic MUST be reported

**Tests:** `crates/rust-total/tests/totality.rs::drop_of_a_call_result_is_rejected`, `::mem_drop_of_a_call_result_is_rejected`

#### Scenario: `.map(|_| ())` is rejected but a real transform is not

- GIVEN a `#[mvl::total]` function calling `.map(|_| ())` on a `Result`/`Option`-shaped receiver
- WHEN the swallow check runs
- THEN a `Level::Error` diagnostic MUST be reported
- AND a `.map` call with a closure that actually uses its parameter (e.g. `.map(|x| x + 1)`) MUST NOT be flagged

**Tests:** `crates/rust-total/tests/totality.rs::map_discarding_closure_is_rejected`, `::map_with_real_transform_is_not_rejected`

---

## Known Limitations

- **`#[mvl::total]` is weaker than its name.** It means "contains no *syntactically obvious* panic construct and, if directly recursive, carries a `decreases` attribute". It does not mean panic-free and it does not mean terminating. Any downstream assurance claim reading `total` as a guarantee is over-reading it.
- **`#[mvl::total]` is not the same predicate as mvl's `total fn`, in either direction.** mvl's `total` is termination-only and opt-out (`partial` is the escape hatch); panic-freedom is a separate requirement (Req 10) checked via refinements. mvl-rust's `total` bundles panic-freedom checking into the same attribute and is opt-in (unannotated functions are unscanned). Concretely: `total fn divzero(a: Int, b: Int) -> Int { a / b }` is accepted by mvl (division-by-zero is a runtime panic, not a totality violation) and rejected by mvl-rust. As of ADR-0009, both mvl-rust and mvl now reject a `#[mvl::decreases(n)]`/`decreases n` naming a measure that doesn't provably decrease — mvl-rust's check reuses `rust-refine`'s own native `L1`–`L4` linear-arithmetic solver (no rustc types, but real arithmetic reasoning), which proves subtraction (including a symbolic amount bounded by a `requires` clause) but cannot represent division/modulo at all, where mvl's own solver can. So mvl-rust still rejects some measures mvl could prove decreasing, but the two no longer disagree on the *presence-only* case: neither accepts a measure it cannot show decreases. mvl's split — termination separate from runtime-error freedom — follows SPARK/Dafny/Idris/Lean 4; mvl-rust's bundling follows Turner's original 2004 formulation adapted to a language where `/` can trap. The two attributes remain not interchangeable, and `total` does not mean the same thing when read across the two projects, but "decreases means presence-only" is no longer one of the differences. ADR-0003 §"Relationship to MVL's `total fn`", ADR-0009.
- **Requirement 2's division rule produces false positives on float code**, by construction. Accepted on frequency grounds; fixing it needs types.
- **SARIF output (`--report=sarif`, #117) is minimal**: one generic `mvl-diagnostic` rule id for every diagnostic (no per-diagnostic category exists to key a real rule taxonomy on), and `provenance` is parsed back out of its `"file:line:col"` string form rather than carried as structured data — a provenance that doesn't parse in that shape falls back to line 1, column 1. Implementation: `crates/mvl-rust-core/src/assurance/sarif.rs`.
- **The `--check` CLI flag (Requirements 1, 3, 6, 7) only narrows which checks a given run reports** — it does not change what any individual check considers a violation. A clean `--check=panic` run says nothing about whether the same file would pass `--check=swallow`; treating a narrowed run as full compliance is a misuse of the flag, not something the tool can catch.
- **Requirement 7 is syntactic-only and over-inclusive**, same limitation as Requirement 1: `let _ = log_and_return_unit();` is flagged even though `()` cannot hold a swallowed error. `#[mvl::unchecked]` remains the escape hatch for a false positive. It also only recognizes the three documented shapes — e.g. `if let Ok(_) = fallible() {}` or storing a discarded call's result behind a `_ =` in a `match` arm are not detected.
- **Effects cannot serve as a purity signal for the solver.** Requirement 4's conflation of *absent* with *declared-empty* means the two are indistinguishable, which is why #45 cannot use `rust-effect` as the purity oracle spec 006's reflexivity rule needs. A tri-state signal would change this decision, not extend it.
- **Nor can an explicit `#[mvl::effect()]`** — Requirement 4's claim is verified only against same-file resolvable calls, so a function annotated pure that reaches an effect through a method or cross-file call is accepted in silence. It is an unverified assertion rather than an established fact, and nothing currently marks it as one. ADR-0008 §3–§4; pinned by `an_explicit_purity_claim_is_not_verified_against_unresolvable_calls`.
- **No cross-procedural effect inference.** A function calling an unresolvable callee may perform arbitrary effects while declaring none, with no diagnostic. Spec 002 narrows this by rejecting `dyn Trait` and unreviewed macros but does not close it.
- **Injected runtime assertions would falsify `#[mvl::total]`.** See spec 007 Requirement 5; the collision is introduced by this port and has no upstream answer.
- **`for` loops aren't covered by Requirement 6.** Only `while` and `loop` are visited. A `for i in 0..n` loop terminates by construction in ordinary use and likely needs a different treatment (recognizing bounded-range iteration) rather than a `loop_decreases!` marker — a real gap, deferred rather than forced into this shape (ADR-0010 Consequences).
- **Requirement 6 cannot compose multiple mutations of the same measure.** A loop that decrements a measure in one branch and increments it in another is rejected outright even when a human could argue it terminates on average. Conservative by design, not a scoped shape-list gap — closing it needs real per-path reasoning.
- **Requirement 6's marker macro is unenforced if simply omitted-and-not-caught by a reviewer** in the sense that nothing about ordinary Rust requires a loop to carry it — the same is true of `#[mvl::total]` itself not being required on any function. This is opt-in by design (ADR-0001), not a defect specific to loops.

---

## Traceability

| Layer | Artefact |
|---|---|
| **Intent** | #6 (`rust-total`), #9 (`rust-effect`, v1 scope), #45 (purity signal — blocked by Req 4, closed as won't-fix per ADR-0008 §6), #82 (loop termination gap), #117 (Requirement 7, silent-swallow checking) |
| **Specification** | this document |
| **Decision** | ADR-0003; ADR-0001 §1 (attribute carrier), §5 (greenfield rule); ADR-0009 (Requirement 3 amendment); ADR-0010 (Requirement 6) |
| **Program** | `crates/rust-total/src/checks/`, `crates/rust-effect/src/checks.rs` |
| **Evidence** | `crates/rust-total/tests/totality.rs` (48 tests), `crates/rust-effect/src/checks.rs::tests` (9 tests), per-tool `tests/verification_mode.rs`, `examples/rust-total-demo/`, `examples/rust-effect-demo/` |
