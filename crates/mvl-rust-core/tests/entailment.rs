//! `discharge_entailment` (#38): the call-site question, `Γ ⊢ goal`, as
//! opposed to `discharge_predicate`'s declaration-site coherence question.
//!
//! These exercise the solver directly, on the constraint-level cases that
//! are hard to reach through `rust-refine`'s source-level fixtures --
//! notably which hypotheses may be dropped, and where negation stops.

use std::collections::HashMap;

use mvl_rust_core::attrs::Predicate;
use mvl_rust_core::solver::native::{discharge_entailment, discharge_predicate, substitute_exprs};
use mvl_rust_core::solver::{DischargeResult, Layer};

fn gamma(clauses: &[&str]) -> Vec<syn::Expr> {
    clauses
        .iter()
        .map(|c| syn::parse_str(c).expect("hypothesis parses"))
        .collect()
}

fn goal(text: &str) -> Predicate {
    syn::parse_str(text).expect("goal parses")
}

fn entail(hypotheses: &[&str], g: &str) -> DischargeResult {
    discharge_entailment(&gamma(hypotheses), &goal(g))
}

fn assert_proven_at(result: DischargeResult, expected: Layer) {
    match result {
        DischargeResult::Proven { layer } => assert_eq!(layer, expected),
        other => panic!("expected Proven at {expected:?}, got {other:?}"),
    }
}

#[test]
fn a_tautological_goal_needs_no_hypotheses() {
    assert_proven_at(entail(&[], "5 > 0"), Layer::L1);
}

#[test]
fn an_unconstrained_goal_is_not_entailed_by_an_empty_context() {
    // The key difference from `discharge_predicate`, which calls this same
    // predicate `Proven` because it is satisfiable. Entailment asks whether
    // it *always* holds, and with nothing known it does not.
    assert_eq!(entail(&[], "x > 0"), DischargeResult::Runtime);
    assert_proven_at(discharge_predicate(&goal("x > 0")), Layer::L2);
}

#[test]
fn a_hypothesis_bound_inside_the_goal_bound_is_entailed_at_l2() {
    assert_proven_at(entail(&["x > 10"], "x > 5"), Layer::L2);
}

#[test]
fn a_hypothesis_bound_wider_than_the_goal_bound_is_not_entailed() {
    // `x > 5` permits `x = 6`, which does not satisfy `x > 10`.
    assert_eq!(entail(&["x > 5"], "x > 10"), DischargeResult::Runtime);
}

#[test]
fn a_goal_contradicting_the_hypotheses_is_violated() {
    assert!(matches!(
        entail(&["x > 10"], "x < 5"),
        DischargeResult::Violated { .. }
    ));
}

#[test]
fn contradictory_hypotheses_entail_anything() {
    // The program point is unreachable, so `Γ ∧ ¬goal` is unsatisfiable for
    // want of a satisfiable Γ. Real MVL's Z3 query goes unsat the same way.
    assert_proven_at(entail(&["x > 10", "x < 5"], "x == 42"), Layer::L2);
}

#[test]
fn every_clause_of_a_conjunctive_goal_must_be_entailed() {
    // The first clause follows from Γ, the second does not -- so the
    // conjunction as a whole does not.
    assert_eq!(
        entail(&["x > 10"], "x > 5 && x < 20"),
        DischargeResult::Runtime
    );
    assert_proven_at(entail(&["x > 10", "x < 15"], "x > 5 && x < 20"), Layer::L2);
}

#[test]
fn cross_variable_hypotheses_are_reached_by_l4_not_l2() {
    // `y > x` is not a constant bound, so L2 cannot represent it at all;
    // Fourier-Motzkin over `Γ ∪ {¬goal}` closes it.
    assert_proven_at(entail(&["x > 10", "y > x"], "y > 5"), Layer::L4);
}

#[test]
fn an_undecidable_hypothesis_is_dropped_rather_than_fatal() {
    // An opaque call carries no usable fact. Dropping it costs precision
    // (this stays `Runtime`) but must not make the query fail or, worse,
    // wrongly succeed.
    assert_eq!(entail(&["is_valid(x)"], "x > 0"), DischargeResult::Runtime);
    // ...and it must not suppress a fact that *is* usable alongside it.
    assert_proven_at(entail(&["is_valid(x)", "x > 10"], "x > 0"), Layer::L2);
}

#[test]
fn an_equality_hypothesis_reaches_fourier_motzkin() {
    // `x == 5` has to participate in the L4 phase for this to close: the
    // goal is not a constant bound, so L2 can't use the equality's point
    // interval. Real MVL's own `is_unsat` drops equalities from that phase;
    // this path deliberately doesn't.
    assert_proven_at(entail(&["x == 5", "y >= 0"], "y > x - 10"), Layer::L4);
}

#[test]
fn an_equality_goal_is_entailed_only_when_the_context_pins_it() {
    // `¬(t = 0)` has no single-inequality form, so L4 can't negate an
    // equality goal -- L2's point interval is what closes this one.
    assert_proven_at(entail(&["x == 5"], "x == 5"), Layer::L2);
    assert_eq!(entail(&["x >= 5"], "x == 5"), DischargeResult::Runtime);
}

#[test]
fn a_quantified_goal_expands_at_l3_against_the_same_context() {
    assert_proven_at(
        entail(&["n > 100"], "forall i in [0..3] . i < n"),
        Layer::L3,
    );
    assert_eq!(
        entail(&[], "forall i in [0..3] . i < n"),
        DischargeResult::Runtime
    );
}

#[test]
fn an_existential_goal_needs_one_witness() {
    assert_proven_at(
        entail(&["n > 100"], "exists i in [0..3] . i < n"),
        Layer::L3,
    );
}

// ── Call-site substitution ────────────────────────────────────────────────

fn bindings(pairs: &[(&str, &str)]) -> HashMap<String, syn::Expr> {
    pairs
        .iter()
        .map(|(name, expr)| {
            (
                (*name).to_string(),
                syn::parse_str(expr).expect("argument parses"),
            )
        })
        .collect()
}

#[test]
fn parameters_are_replaced_by_the_actual_arguments() {
    let substituted = substitute_exprs(&goal("n > 0"), &bindings(&[("n", "x - 1")]));
    // Parenthesized so the spliced argument keeps its own precedence.
    assert_eq!(substituted.render(), "(x - 1) > 0");
}

#[test]
fn substitution_reaches_inside_a_quantifier_body() {
    let substituted = substitute_exprs(
        &goal("forall i in [0..3] . i < n"),
        &bindings(&[("n", "k")]),
    );
    assert_eq!(substituted.render(), "forall i in [0..3] . i < (k)");
}

#[test]
fn a_quantifiers_bound_variable_shadows_a_binding_of_the_same_name() {
    // The `i` inside the body is the quantifier's, not the parameter's, so
    // it must survive substitution untouched.
    let substituted = substitute_exprs(
        &goal("forall i in [0..3] . i < n"),
        &bindings(&[("i", "99")]),
    );
    assert_eq!(substituted.render(), "forall i in [0..3] . i < n");
}

// ── Where an unsatisfiable Γ is detected, and where it isn't ──────────────

#[test]
fn a_linear_only_contradiction_in_gamma_still_entails_an_equality_goal() {
    // `contradictory_hypotheses_entail_anything` covers the case the
    // per-variable interval check catches. This Γ contradicts itself only
    // across variables, so only the linear system sees it -- and the goal is
    // an equality, which has no single-inequality negation, so the L4
    // entailment pass cannot reach the same conclusion on its own. Without
    // an explicit check this reported `Violated`: a hard compile error on
    // unreachable code.
    let result = entail(&["x + y <= 0", "x >= 5", "y >= 5"], "x == 3");
    assert!(
        matches!(result, DischargeResult::Proven { .. }),
        "an unsatisfiable Γ entails anything, got {result:?}"
    );
}

#[test]
fn a_satisfiable_gamma_that_rules_the_goal_out_is_still_violated() {
    // The guard above must not swallow real violations: Γ here is perfectly
    // satisfiable, and simply admits no value where the goal holds.
    let result = entail(&["x >= 5"], "x < 0");
    assert!(
        matches!(result, DischargeResult::Violated { .. }),
        "expected Violated, got {result:?}"
    );
}

#[test]
fn an_equality_goal_ruled_out_by_gamma_is_violated() {
    // Was `Runtime` until #43: `system_constraints` expanded an equality
    // *hypothesis* into its two inequalities so Fourier-Motzkin could use
    // it, but the `Γ ∧ goal` check fed goal clauses in raw and the ported
    // L4 drops equalities, so `x >= 5 ∧ x == 3` was never seen as the
    // contradiction it is. Both sides now get the same expansion.
    let result = entail(&["x >= 5"], "x == 3");
    assert!(
        matches!(result, DischargeResult::Violated { .. }),
        "expected Violated, got {result:?}"
    );
}

#[test]
fn an_equality_goal_merely_unproven_by_gamma_stays_runtime() {
    // The boundary the two-inequality split must respect: `x == 7` is
    // satisfiable under `x >= 5` but not entailed by it, so it is neither
    // `Proven` nor `Violated`. Requiring *both* halves of the negation to
    // be unsat is what keeps this from becoming a false `Proven`.
    let result = entail(&["x >= 5"], "x == 7");
    assert!(
        matches!(result, DischargeResult::Runtime),
        "expected Runtime, got {result:?}"
    );
}

// ── Bounded expansion is capped on the product, not per quantifier ────────

#[test]
fn nested_quantifiers_are_capped_on_their_product() {
    // Two individually-legal 1000-wide ranges expand to 1_000_000 instances.
    // The per-quantifier check passes both; only the product catches it.
    // Left uncapped this took ~46s for a single call site.
    let pred = goal("forall i in [0..999] . forall j in [0..999] . i + j < n");
    assert!(matches!(
        discharge_predicate(&pred),
        DischargeResult::Runtime
    ));
    assert!(matches!(
        discharge_entailment(&gamma(&["n > 10"]), &pred),
        DischargeResult::Runtime
    ));
}

#[test]
fn nested_quantifiers_within_the_cap_still_expand() {
    // 20 * 20 = 400 instances, comfortably under the cap -- the fix must not
    // turn every nested quantifier into an unconditional `Runtime`.
    let pred = goal("forall i in [0..19] . forall j in [0..19] . i + j < n");
    assert_proven_at(discharge_entailment(&gamma(&["n > 100"]), &pred), Layer::L3);
}

// ── Equality goals (#43) ──────────────────────────────────────────────────
//
// An equality goal used to be unprovable outside the narrow case where Γ
// pinned the variable to a point interval -- not even a syntactic identity.
// Two independent mechanisms close it, and neither subsumes the other:
// L1 reflexivity reaches non-linear terms the L4 split cannot represent,
// and the L4 split reaches rearranged terms L1's structural match misses.

#[test]
fn a_reflexive_goal_is_proven_at_l1() {
    assert_proven_at(entail(&[], "a == a"), Layer::L1);
    assert_proven_at(entail(&[], "a <= a"), Layer::L1);
    assert_proven_at(entail(&[], "a >= a"), Layer::L1);
}

#[test]
fn an_irreflexive_goal_is_violated_at_l1() {
    for text in ["a != a", "a < a", "a > a"] {
        let result = entail(&[], text);
        assert!(
            matches!(result, DischargeResult::Violated { .. }),
            "`{text}` can never hold, got {result:?}"
        );
    }
}

#[test]
fn reflexivity_sees_through_one_sided_parenthesization() {
    // The shape call-site substitution actually produces: `substitute_exprs`
    // parenthesizes every argument it splices in, so a return-site
    // obligation for `ensures(result == a + b)` with body `a + b` arrives
    // grouped on one side only. A plain `==` on `syn::Expr` would miss it.
    assert_proven_at(entail(&[], "(a + b) == a + b"), Layer::L1);
    assert_proven_at(entail(&[], "((a)) == a"), Layer::L1);
}

#[test]
fn a_non_linear_identity_is_reachable_only_by_l1() {
    // `a * b` is outside the linear fragment, so `constraints_from_clause`
    // rejects it and the L4 split never sees it -- yet
    // `#[mvl::ensures(result == a * b)] fn mul(a, b) { a * b }` is an
    // ordinary postcondition. Structural reflexivity is what discharges it.
    assert_proven_at(entail(&[], "(a * b) == a * b"), Layer::L1);
}

#[test]
fn a_rearranged_equality_is_reachable_only_by_l4() {
    // Structurally different, arithmetically identical -- the mirror image
    // of the case above, and why L1 reflexivity is not sufficient alone.
    assert_proven_at(entail(&[], "(a + b) == b + a"), Layer::L4);
    assert_proven_at(entail(&[], "(a + a) == 2 * a"), Layer::L4);
}

#[test]
fn an_equality_goal_entailed_by_gamma_is_proven() {
    // Two-sided constant bounds intersect to a point interval, so L2 closes
    // this one without needing the split at all.
    assert_proven_at(entail(&["x >= 7", "x <= 7"], "x == 7"), Layer::L2);
}

#[test]
fn a_cross_variable_equality_goal_needs_the_l4_split() {
    // `y >= x` is not a constant bound, so L2 cannot represent either the
    // hypotheses or the goal. Only running both halves of the negation
    // through Fourier-Motzkin closes it.
    assert_proven_at(entail(&["y >= x", "y <= x"], "y == x"), Layer::L4);
    // ...and one-sided is correctly not enough.
    assert_eq!(entail(&["y >= x"], "y == x"), DischargeResult::Runtime);
}

#[test]
fn reflexivity_is_structural_and_does_not_check_purity() {
    // Deliberate, and a known deviation: `mvl-lang/mvl`'s `preds_equivalent`
    // is also purely structural, but its `RefExpr` grammar cannot express a
    // call in the first place, so the question never arises there. Ours can.
    // If `f` is impure this conclusion is wrong. Recorded in ADR-0002's
    // scope list; the fix is a purity signal from `rust-effect`, not a
    // change here. Pinned so the deviation stays deliberate.
    assert_proven_at(entail(&[], "f() == f()"), Layer::L1);
}
