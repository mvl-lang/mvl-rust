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
