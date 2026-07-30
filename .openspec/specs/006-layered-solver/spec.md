# 006 — The Layered Solver

**Domain:** Decision procedures
**Version:** 0.1.0
**Status:** Fully implemented (L1–L5; L5 feature-gated and narrowed in scope — see Requirement 8)
**Date:** 2026-07-29
**Decided by:** ADR-0006 §1–§3

## Overview

Spec 005 settles *what question* is asked at each program point. This spec settles *what answers the layer stack can produce*. What happens to the ones it cannot close is spec 007.

The stack mirrors the reference implementation's, and the rationale is cost plus trust: most predicates are simple, so routing everything through an SMT solver is correct but wasteful, and the deeper layers are the only ones needing an external solver in the trust boundary.

| Layer | Decision procedure | Complete for its class? |
|---|---|---|
| L1 | Constant folding + structural reflexivity | for exact syntactic shapes |
| L2 | Per-variable integer interval containment | yes, for its fragment |
| L3 | Bounded-quantifier expansion (an attribution, not a procedure) | n/a |
| L4 | Fourier–Motzkin elimination + single-variable divisibility | **no** — not even for QF-LIA |
| L5 | Z3 SMT, `Int`/QF-NIA only, feature-gated (#37) | complete for QF-NIA within the 1s timeout; `unknown`/timeout/feature-off falls to `Runtime`, never a proof |

### Philosophy

- **Layer names describe what runs, not what was aspired to.** L4 is named `L4` for wire compatibility but **is not Cooper's algorithm** — see Requirement 4.
- **Sound direction only.** A layer that cannot decide MUST decline rather than guess. A bail-out MUST NOT be readable as a proof.
- **Priced against evidence.** The reference's own evaluation discharges 0.6% of obligations at L3 and 2.3% at L4, against 19.5% at L5. Deferrals below follow that data.

---

## RFC 2119 Keywords

The key words "MUST", "MUST NOT", "SHOULD", "SHOULD NOT", and "MAY" in this document are to be interpreted as described in [RFC 2119](https://datatracker.ietf.org/doc/html/rfc2119).

---

## Requirements

### Requirement 1: L1 decides constant folding and structural identity [MUST]

The solver MUST decide a comparison between two integer literals by evaluation, and a comparison whose operands are structurally identical by reflexivity — `Eq`/`Le`/`Ge` true, `Ne`/`Lt`/`Gt` false.

Structural identity MUST see through grouping at every operand depth, and MUST recurse through unary negation. It MUST NOT be extended to commutativity, associativity or distributivity, which belong to L4's normal form.

**Implementation:** `crates/mvl-rust-core/src/solver/native.rs`

#### Scenario: Substitution's one-sided grouping still closes

- GIVEN a return-site goal `(a + b) == a + b`, the shape substitution produces
- WHEN the obligation is discharged
- THEN the outcome MUST be `Proven` at L1

**Tests:** `crates/mvl-rust-core/tests/entailment.rs::reflexivity_sees_through_one_sided_parenthesization`

#### Scenario: A non-linear identity is reachable only by L1

- GIVEN a goal `(a * b) == a * b`
- WHEN the obligation is discharged
- THEN the outcome MUST be `Proven` at L1, since the linear fragment cannot represent the term

**Tests:** `crates/mvl-rust-core/tests/entailment.rs::a_non_linear_identity_is_reachable_only_by_l1`

### Requirement 2: L1 reflexivity is gated on call-free terms [MUST]

Structural identity MUST NOT be applied to a term containing a call, at any depth.

Rationale: reflexivity concludes "same tree ⇒ same value", which is invalid for an impure term, and it fails in **both** directions — `span(gen(), gen())` against `requires(lo <= hi)` would be `Proven`, dropping a check that can genuinely fail, and `requires(a != b)` would be `Violated`, a compile error on a valid call.

**Implementation:** `crates/mvl-rust-core/src/solver/native.rs`

#### Scenario: A buried call suppresses reflexivity

- GIVEN a goal `a + f() == a + f()`
- WHEN the obligation is discharged
- THEN the outcome MUST be `Runtime`, not `Proven`

**Tests:** `crates/mvl-rust-core/tests/entailment.rs::reflexivity_does_not_fire_on_a_term_containing_a_call`

### Requirement 3: L2 decides interval containment per variable [MUST]

The solver MUST maintain a closed integer interval per variable from hypotheses of the form `var OP literal`, and MUST prove a goal clause when the known interval is contained in the goal's interval.

An empty intersection MUST yield `Violated`. `!=` MUST NOT produce a bound, since it does not describe one contiguous interval. Cross-variable relations MUST NOT be treated as bounds.

**Implementation:** `crates/mvl-rust-core/src/solver/native.rs`

#### Scenario: A wider hypothesis does not entail a narrower goal

- GIVEN a hypothesis `x > 0` and a goal `x > 5`
- WHEN the obligation is discharged
- THEN the outcome MUST NOT be `Proven`

**Tests:** `crates/mvl-rust-core/tests/entailment.rs::a_hypothesis_bound_wider_than_the_goal_bound_is_not_entailed`

### Requirement 4: L4 is Fourier–Motzkin elimination, not Cooper's algorithm [MUST]

The solver MUST decide a conjunction of linear integer inequalities by Fourier–Motzkin elimination, with strict inequalities tightened over the integers (`t < 0` ⟺ `t + 1 ≤ 0`), plus a divisibility test for single-variable equalities.

The layer MUST retain the label `L4` for wire compatibility with upstream tooling. Documentation MUST describe the technique as Fourier–Motzkin and MUST NOT claim Cooper's quantifier elimination, which is implemented nowhere: the constraint representation has no divisibility atom, so Cooper's central atom is unrepresentable.

Complexity guards MUST bail rather than run unbounded: more than 5 free variables, coefficient magnitude above 10⁶, or more than 128 derived constraints.

**Implementation:** `crates/mvl-rust-core/src/solver/native.rs`

#### Scenario: A rearranged equality closes only at L4

- GIVEN a goal `(a + b) == b + a`
- WHEN the obligation is discharged
- THEN the outcome MUST be `Proven` at L4, since no tree comparison matches the rearrangement

**Tests:** `crates/mvl-rust-core/tests/entailment.rs::a_rearranged_equality_is_reachable_only_by_l4`

#### Scenario: A non-linear term is declined, not guessed

- GIVEN a goal over `x * y` with both factors variable
- WHEN L4 inspects it
- THEN the layer MUST decline
- AND the obligation MUST fall through to `Runtime` **when L5 is unavailable or also declines** — L4's own decline is unconditional; what happens *next* is Requirement 8's

**Tests:** `crates/rust-refine/tests/call_sites.rs::nonlinear_argument_falls_through_to_runtime` (default features, no z3), `::a_genuine_nonlinear_entailment_proves_at_l5_with_z3` (--features z3 — the same fixture, now closed at L5)

### Requirement 5: A satisfiable Fourier–Motzkin verdict must not yield `Proven` [MUST]

Fourier–Motzkin decides satisfiability over the **rationals**. `FM-UNSAT ⟹ ℤ-UNSAT`, so concluding `Proven` from an unsatisfiable verdict is sound. The converse is not: a system may be rationally satisfiable with no integer solution.

The solver MUST NOT conclude `Proven` from a satisfiable verdict, and MUST NOT allow a complexity bail-out to be read as satisfiable.

**Implementation:** `crates/mvl-rust-core/src/solver/native.rs`

#### Scenario: A rationally-satisfiable, integer-unsatisfiable predicate is not proven

- GIVEN a predicate `2 * x >= 1 && 2 * x <= 1`, satisfiable at `x = ½` and unsatisfiable over ℤ
- WHEN the coherence obligation is discharged
- THEN the outcome MUST NOT be `Proven`

**Tests:** `crates/mvl-rust-core/src/solver/native.rs::a_rationally_satisfiable_integer_unsatisfiable_predicate_is_not_proven`, `::a_parity_contradiction_is_not_proven`

### Requirement 6: An equality goal is split into two refutations [MUST]

Since `t = 0` holds exactly when `t ≤ 0` and `t ≥ 0` both do, an equality goal MUST be refuted as two independent queries, both of which MUST come back unsatisfiable for an outcome of `Proven`. Negation MUST stay inside the linear fragment rather than introducing disjunction support.

**Implementation:** `crates/mvl-rust-core/src/solver/native.rs`

#### Scenario: An equality goal needs the context to pin both sides

- GIVEN hypotheses `y >= x` and `y <= x` and a goal `y == x`
- WHEN the obligation is discharged
- THEN the outcome MUST be `Proven`

**Tests:** `crates/mvl-rust-core/tests/entailment.rs::an_equality_goal_is_entailed_only_when_the_context_pins_it`

### Requirement 7: L3 expands bounded quantifiers and re-dispatches [MUST]

A `forall`/`exists` over a literal range MUST be discharged by substituting each integer in the range and re-dispatching through the same entry point, so nesting and Γ need no special handling.

The expansion MUST be capped on the **product** of quantifier widths, not each width independently. An empty range MUST make `forall` vacuously proven and `exists` violated. Every instance of a `forall` MUST be proven; one witness suffices for `exists`.

**Implementation:** `crates/mvl-rust-core/src/solver/native.rs`

#### Scenario: Nested quantifiers are capped on their product

- GIVEN two nested quantifiers whose widths are individually within the cap but whose product exceeds it
- WHEN the obligation is discharged
- THEN the expansion MUST be declined rather than run

**Tests:** `crates/mvl-rust-core/tests/entailment.rs::nested_quantifiers_are_capped_on_their_product`, `::nested_quantifiers_within_the_cap_still_expand`

### Requirement 8: L5 delegates to Z3, feature-gated [MUST]

The solver MUST delegate to an SMT solver when the native layers are exhausted. The layer MUST be feature-gated with no build dependency when disabled, MUST return a runtime outcome immediately when the feature is off, and MUST treat `unknown` or a timeout as a runtime outcome rather than a proof.

**Landed by #37, narrowed twice from the ticket's own original scope** (both re-scopes are in the issue's own comment history, re-verified rather than taken on faith):

- **`Int`/QF-NIA only**, not the reference's four sorts (string/bitwise/float/int). `crate::attrs::Predicate`'s grammar has no string, bitwise, or float surface at all — three of the reference's four encoding paths (`impl_z3_str`+`regex_z3.rs`, `impl_z3_bv`, `impl_z3_real`) have nothing to encode against and are a follow-up gated on the grammar growing that surface, not on this requirement.
- **Proof direction only.** `L5` answers "does Γ entail every still-unresolved goal clause" (`Γ ∧ ¬goal` UNSAT), never "is the goal violated". Real MVL's own model-extraction / counterexample-vs-witness classification for the `Violated` direction is out of scope — `L1`–`L4` already own disproof, and a nonlinear counterexample is not yet a demonstrated need.
- **One of the two motivating reproducers the issue verified turned out stale before implementation started.** Equality-goal entailment (`x == 4 && y == x + 1 ⇒ y == 5`) was cited as needing Z3; re-verified against the current backend, it now closes at `L4` via equality-splitting (`#43`, which postdates the issue's own research). The reproducer that held up: genuine nonlinearity (`a > 2 && b > 2 ⇒ a * b > 4`), which `linterm_from_expr` refuses by construction regardless of system size. `L4`'s own complexity-guard bailouts (Requirement 4's five-variable/coefficient/constraint-count caps) are covered by the same mechanism, with no dedicated logic — Z3 doesn't need to know *why* `L1`–`L4` gave up.

**Implementation:** `crates/mvl-rust-core/src/solver/smt.rs`, wired into `crates/mvl-rust-core/src/solver/native.rs::entail_expr`

#### Scenario: The feature being disabled is not a failure

- GIVEN the Z3 feature is not enabled
- WHEN an obligation reaches L5
- THEN a runtime outcome MUST be returned immediately without panicking

**Tests:** `crates/rust-refine/tests/call_sites.rs::nonlinear_argument_falls_through_to_runtime` (default features)

#### Scenario: A timeout is not a proof

- GIVEN Z3 cannot decide an obligation within the timeout
- WHEN the query returns
- THEN the outcome MUST be a runtime check, never `Proven`

Not independently tested with a real timeout — that would mean constructing a query slow enough to hit the 1s budget deterministically, which is exactly the kind of flaky, machine-dependent test worth avoiding. The guarantee is structural: `Config::set_timeout_msec` is set unconditionally before every query, and the `SatResult` match only returns `Proven` on `Unsat`; `Sat` and `Unknown` (which is what a timeout produces) both fall through identically.

#### Scenario: A genuine nonlinear entailment proves at L5

- GIVEN `#[mvl::requires(n > 1)] fn double(n) ...` called from a caller whose Γ has `x > 1 && y > 1` and whose argument is `x * y`
- WHEN the `z3` feature is enabled and the obligation is discharged
- THEN the outcome MUST be `Proven { layer: Layer::L5 }`

**Tests:** `crates/mvl-rust-core/src/solver/smt.rs::nonlinear_entailment_proves`, `crates/rust-refine/tests/call_sites.rs::a_genuine_nonlinear_entailment_proves_at_l5_with_z3` (--features z3)

#### Scenario: An unencodable goal clause does not panic, it falls through

- GIVEN a goal outside the encodable fragment (a function call, a string/bitwise/float operation the grammar cannot even express)
- WHEN `L5` attempts to encode it
- THEN encoding MUST return `None` rather than panicking, and the obligation MUST fall through as if `L5` had declined

**Tests:** `crates/mvl-rust-core/src/solver/smt.rs::an_unencodable_clause_falls_through_rather_than_panicking`

#### Scenario: An unencodable hypothesis is dropped, not fatal to the query

- GIVEN a hypothesis outside the encodable fragment alongside others that are within it, and a goal the encodable hypotheses alone already entail
- WHEN `L5` attempts to encode Γ
- THEN the unencodable hypothesis MUST be dropped rather than failing the whole query — the same "fewer facts only make proving harder, never wrongly easier" reasoning Requirement 6 already applies to `L1`-`L4`'s hypothesis handling — and the goal MUST still prove on what remains

**Tests:** `crates/mvl-rust-core/src/solver/smt.rs::an_unencodable_hypothesis_is_dropped_not_bailed_on`

---

## Known Limitations

- **`L5` sits behind `discharge_entailment`, not the `SolverBackend` trait.** The trait's obligation type carries a re-parsed string predicate with no Γ field, so `L5` needed the same Γ-shaped interface `L4` already uses (`discharge_entailment(&[Expr], &Predicate)`, #38) rather than a reframing of the trait.
- **`L5` covers entailment only, not the declaration-site coherence question** (`discharge_predicate`). Real MVL's own motivating cases for Z3 are all call-site hypothesis chains; a coherence-only nonlinear satisfiability question was never the reproducer this landed for, and extending it there is unscoped follow-up, not a gap in this requirement.
- **String/bitwise/float encoding paths don't exist**, because the grammar has nothing for them to encode. `crate::attrs::Predicate` is comparison/boolean expressions plus bounded quantifiers over integers only — the moment (if ever) that grammar grows string, bitwise, or float surface, this is where that work resumes, mirroring real MVL's `impl_z3_str`/`impl_z3_bv`/`impl_z3_real`.
- **`L5` proves, it does not disprove.** A `Sat` result means "not entailed", not "definitely violated" — real MVL's model-extraction / counterexample-vs-symbolic-witness classification for that direction is unimplemented. A nonlinear obligation that is genuinely violated still falls to `Runtime`, the same as before this requirement landed, rather than becoming a compile-time error.
- **Local development against a homebrew-installed Z3 needs two env vars `z3-sys`'s build script doesn't discover on its own**: `Z3_SYS_Z3_HEADER=$(brew --prefix z3)/include/z3.h` (bindgen doesn't consult `pkg-config` for the header) and `RUSTFLAGS="-L $(brew --prefix z3)/lib"` (the linker needs an explicit search path). Neither is needed in CI, where `apt`-installed `libz3-dev` lands in standard system paths bindgen and the linker already search.
- **Requirement 5 is satisfied since #49**, at a cost in precision. `Satisfiable` from Fourier–Motzkin no longer yields `Proven`, so an integer-unsatisfiable predicate falls to a runtime check rather than being reported proven. The cost: `2 * x == 6` *is* exactly decidable over ℤ by the divisibility check (`2 | 6` ⟹ `x = 3`), but `check_satisfiability` collapses that exact verdict and a merely-rational one into a single `Satisfiable`, so it fell out with the unsound case. Distinguishing them would recover it.
- **L4 cannot handle a conjunctive goal in the reference implementation**, because `¬(A ∧ B)` is a disjunction. Requirement 6's per-clause split is this implementation's answer; the reference has no equivalent.
- **L3 here is not L3 there.** The reference's L3 is symbolic path enumeration over pure function bodies. This implementation has no path enumeration at all. Deferred on the 0.6% hit rate; the prose must stop claiming otherwise (#55).
- **Real Cooper's algorithm is deferred.** It would close the ℚ/ℤ gap and the parity cases natively, but it is a rewrite rather than an extension, and the reference discharges four of 174 obligations at this layer.
- **No atom normalisation.** Compound atoms (`s.field`, `xs.len()`) are not lifted to opaque variables, so goals over them never reach the arithmetic layers — where the reference proves them at L2.
- **L1 reflexivity is unsound for floats.** `x == x` is false for NaN, and there is no type information to exclude an `f64`. Sound within the unbounded-ℤ scope; admitting float-typed terms needs a real type signal first.
- **Lifting the call-free gate needs four things, not one.** #45 frames a tri-state purity signal as the prerequisite; ADR-0008 records that it is one of four, alongside a *checkable* purity claim (an explicit `#[mvl::effect()]` is not one), the type signal above, and a notion of determinism that MVL's effect vocabulary does not currently express. Deliberately unscheduled: the gate is imprecise rather than unsound, which ADR-0001 §5 accepts.
- **Call-freedom is a syntactic approximation of purity** (Requirement 2). #45 tracks the real signal; spec 003 Requirement 4 records why the effect system cannot currently supply it.

---

## Traceability

| Layer | Artefact |
|---|---|
| **Intent** | #7 (native dispatcher), #31 (L3), #35 (L4), #43/#44 (equality goals and the call-free gate), #37 (L5), #45 (purity signal), #49 (FM soundness) |
| **Specification** | this document; spec 005 (where obligations come from), spec 007 (residual enforcement) |
| **Decision** | ADR-0006 §1–§3; ADR-0005 §4 (negation and dropped hypotheses) |
| **Program** | `crates/mvl-rust-core/src/solver/native.rs`, `crates/mvl-rust-core/src/solver/mod.rs`, `crates/mvl-rust-core/src/solver/smt.rs` (L5, feature-gated, #37) |
| **Evidence** | `crates/mvl-rust-core/tests/entailment.rs` (36 tests), `crates/mvl-rust-core/src/solver/native.rs::tests`, `crates/rust-refine/tests/call_sites.rs` (six ported upstream L5 fixtures closing at L4 with no SMT solver, plus the L5-with-`z3`-feature scenario), `crates/mvl-rust-core/src/solver/smt.rs::real::tests` (#37) |
| **Upstream reference** | `mvl-lang/mvl` spec `018-refinement-solver`; [`mvl-lang/mvl`#2022](https://github.com/mvl-lang/mvl/issues/2022) (L4 naming) |
