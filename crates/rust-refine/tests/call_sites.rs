//! Call-site obligation checking against a hypothesis context Γ (#38).
//!
//! The first section is a **port of `mvl-lang/mvl`'s own Layer 5 fixtures**
//! (`tests/solver/layer5/`), which exist there precisely because its L1-L4
//! cannot discharge them: its L2 needs constant bounds (`self > x` is not
//! constant) and its L4 handles linear-expression *arguments* rather than
//! bare variables whose hypotheses reference other variables. Every one is
//! marked in its source with the query real MVL sends to Z3.
//!
//! They are ported here as the acceptance corpus for Γ, and they double as
//! cross-validation: two independently written solvers, the same
//! obligations. Where the layer attribution differs, that difference is
//! asserted explicitly below rather than smoothed over -- see
//! `chained_hypotheses_close_at_l4_without_an_smt_solver`.

use mvl_rust_core::diagnostics::Level;
use mvl_rust_core::solver::{DischargeResult, Layer, Warrant};
use rust_refine::checks::{check_source, find_obligations, ObligationKind};

/// Every call-site obligation in `source`, as `(callee, outcome)`.
fn call_sites(source: &str) -> Vec<(String, DischargeResult)> {
    find_obligations(source)
        .expect("fixture parses")
        .iter()
        .filter_map(|found| match &found.kind {
            ObligationKind::CallSite { callee } => Some((callee.clone(), found.discharge())),
            _ => None,
        })
        .collect()
}

/// The single call-site outcome in `source` -- asserts there's exactly one,
/// so a fixture that silently stops producing an obligation fails loudly
/// instead of vacuously passing.
fn only_call_site(source: &str) -> DischargeResult {
    let sites = call_sites(source);
    assert_eq!(
        sites.len(),
        1,
        "expected exactly one call-site obligation, got {sites:?}"
    );
    sites.into_iter().next().unwrap().1
}

/// Like [`only_call_site`], but also returns what backs the outcome (#69) --
/// for tests asserting the *provenance* the wire schema reports, not just
/// whether the goal discharged.
fn only_call_site_warrant(source: &str) -> (DischargeResult, Warrant) {
    let found = find_obligations(source).expect("fixture parses");
    let sites: Vec<_> = found
        .iter()
        .filter(|f| matches!(f.kind, ObligationKind::CallSite { .. }))
        .collect();
    assert_eq!(
        sites.len(),
        1,
        "expected exactly one call-site obligation, got {sites:?}"
    );
    let site = sites[0];
    (site.discharge(), site.warrant())
}

fn assert_proven_at(result: &DischargeResult, expected: Layer) {
    match result {
        DischargeResult::Proven { layer } => assert_eq!(
            *layer, expected,
            "proven, but at {layer:?} rather than {expected:?}"
        ),
        other => panic!("expected Proven at {expected:?}, got {other:?}"),
    }
}

// ── Ported from mvl-lang/mvl tests/solver/layer5/ ─────────────────────────

/// `01_chained_hypotheses.mvl`, whose own comment records real MVL's query:
/// `x > 10 ∧ y > x ∧ ¬(y > 5)` → UNSAT → Proven.
///
/// Real MVL needs **L5/Z3** for this. Our entailment path runs
/// Fourier-Motzkin over `Γ ∪ {¬goal}` directly, so it closes at **L4 with
/// no SMT solver at all**. That divergence is the point of having two
/// implementations, and it is asserted, not tolerated: if this ever starts
/// reporting L5 (or stops proving), the assertion says so.
#[test]
fn chained_hypotheses_close_at_l4_without_an_smt_solver() {
    let result = only_call_site(
        "#[mvl::requires(n > 5)]\n\
         fn require_gt5(n: i32) -> i32 { n }\n\
         #[mvl::requires(x > 10 && y > x)]\n\
         fn chain_via_gt10(x: i32, y: i32) -> i32 { require_gt5(y) }",
    );
    assert_proven_at(&result, Layer::L4);
}

/// `01_chained_hypotheses.mvl`'s second case: `x > 0, y > x ⊢ y > 0`.
#[test]
fn chain_via_positive() {
    let result = only_call_site(
        "#[mvl::requires(n > 0)]\n\
         fn require_positive(n: i32) -> i32 { n }\n\
         #[mvl::requires(x > 0 && y > x)]\n\
         fn chain(x: i32, y: i32) -> i32 { require_positive(y) }",
    );
    assert_proven_at(&result, Layer::L4);
}

/// `03_ge_and_gt_chain.mvl`: mixed `>=`/`>` in the chain, which is where an
/// off-by-one in the strict-inequality tightening (`t < 0` ↔ `t+1 ≤ 0`)
/// would show up.
#[test]
fn ge_and_gt_chain() {
    let result = only_call_site(
        "#[mvl::requires(n >= 5)]\n\
         fn require_ge5(n: i32) -> i32 { n }\n\
         #[mvl::requires(x >= 5 && y > x)]\n\
         fn chain(x: i32, y: i32) -> i32 { require_ge5(y) }",
    );
    assert_proven_at(&result, Layer::L4);
}

/// `04_four_variable_chain.mvl`: a four-link chain, which also exercises
/// the ported free-variable complexity guard (bails above 5 variables).
#[test]
fn four_variable_chain() {
    let result = only_call_site(
        "#[mvl::requires(n > 0)]\n\
         fn require_positive(n: i32) -> i32 { n }\n\
         #[mvl::requires(a > 0 && b > a && c > b && d > c)]\n\
         fn chain(a: i32, b: i32, c: i32, d: i32) -> i32 { require_positive(d) }",
    );
    assert_proven_at(&result, Layer::L4);
}

/// `06_sandwich_bounds.mvl`: bounded on both sides, and the goal is a
/// two-clause conjunction -- so every goal clause must be entailed
/// separately (`Γ ∧ ¬cᵢ` UNSAT for each `i`), not just one of them.
#[test]
fn sandwich_bounds_with_a_conjunctive_goal() {
    let result = only_call_site(
        "#[mvl::requires(n > 0 && n < 100)]\n\
         fn require_in_range(n: i32) -> i32 { n }\n\
         #[mvl::requires(x > 10 && x < 50 && y > x && y < 90)]\n\
         fn sandwich(x: i32, y: i32) -> i32 { require_in_range(y) }",
    );
    assert_proven_at(&result, Layer::L4);
}

/// `08_cross_param_bounds.mvl`: the argument is a linear *expression*, not a
/// bare variable -- the case real MVL's own L4 does handle.
#[test]
fn cross_param_bounds_with_a_linear_argument() {
    let result = only_call_site(
        "#[mvl::requires(n >= 0)]\n\
         fn require_non_negative(n: i32) -> i32 { n }\n\
         #[mvl::requires(a > b)]\n\
         fn difference(a: i32, b: i32) -> i32 { require_non_negative(a - b) }",
    );
    assert_proven_at(&result, Layer::L4);
}

/// `09_violations_literal.mvl`: a literal argument that provably breaks the
/// precondition is a compile-time error, not a runtime check.
#[test]
fn literal_argument_violating_the_precondition_is_an_error() {
    let diagnostics = check_source(
        "#[mvl::requires(n > 0)]\n\
         fn require_positive(n: i32) -> i32 { n }\n\
         fn caller() -> i32 { require_positive(-5) }",
    )
    .unwrap();

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.level == Level::Error)
        .collect();
    assert_eq!(errors.len(), 1, "got: {diagnostics:#?}");
    assert!(
        errors[0].message.contains("can never hold"),
        "message was: {}",
        errors[0].message
    );
}

/// `10_nonlinear_runtime.mvl`: non-linear arithmetic falls through to a
/// runtime check rather than being wrongly claimed either way, *without*
/// `L5`. Real MVL reaches the same outcome via Z3 returning `unknown`
/// (no SMT dispatch of its own on this port's default build); this
/// backend reaches it by `linterm_from_expr` refusing variable × variable.
///
/// With the `z3` feature on, this exact scenario is no longer out of
/// reach -- QF-NIA proves `x > 1 && y > 1 => x * y > 0` trivially -- so
/// this test is default-features-only; see
/// `a_genuine_nonlinear_entailment_proves_at_l5_with_z3` below for the
/// `z3`-feature counterpart (#37).
#[test]
#[cfg(not(feature = "z3"))]
fn nonlinear_argument_falls_through_to_runtime() {
    let result = only_call_site(
        "#[mvl::requires(n > 0)]\n\
         fn require_positive(n: i32) -> i32 { n }\n\
         #[mvl::requires(x > 1 && y > 1)]\n\
         fn product(x: i32, y: i32) -> i32 { require_positive(x * y) }",
    );
    assert_eq!(result, DischargeResult::Runtime);
}

/// #37: the same fixture, run only under `--features z3`. `L1`-`L4` cannot
/// represent `x * y > 0` (variable × variable is outside the linear
/// fragment by construction), but it is trivial for QF-NIA, and `L5`
/// closes it once Z3 is available.
#[test]
#[cfg(feature = "z3")]
fn a_genuine_nonlinear_entailment_proves_at_l5_with_z3() {
    let result = only_call_site(
        "#[mvl::requires(n > 0)]\n\
         fn require_positive(n: i32) -> i32 { n }\n\
         #[mvl::requires(x > 1 && y > 1)]\n\
         fn product(x: i32, y: i32) -> i32 { require_positive(x * y) }",
    );
    assert_proven_at(&result, Layer::L5);
}

/// #37, #72: a callee precondition with *two* clauses in the same
/// obligation -- `m > 0` (linear, closed by `L4`'s `refutes_negation`) and
/// `m * n > 4` (genuinely nonlinear, closed by `L5`) -- to exercise
/// `entail_expr`'s two-phase split: L4 narrows `unresolved` down to just
/// what it can't refute, and only that remainder is handed to Z3. Without
/// the split (or if L5 were skipped) this would fall to `Runtime`, since
/// `m * n > 4` alone is out of L4's reach; with `z3`, the whole
/// obligation proves, reported at `L5` for the deepest layer used.
#[test]
#[cfg(feature = "z3")]
fn l4_and_l5_jointly_close_a_two_clause_obligation() {
    let result = only_call_site(
        "#[mvl::requires(m > 0 && m * n > 4)]\n\
         fn needs_positive_product(m: i32, n: i32) -> i32 { m }\n\
         #[mvl::requires(a > 2 && b > 2)]\n\
         fn combo(a: i32, b: i32) -> i32 { needs_positive_product(a - 1, b) }",
    );
    assert_proven_at(&result, Layer::L5);
}

// ── Γ's three sources of facts ────────────────────────────────────────────

#[test]
fn a_call_with_no_hypotheses_at_all_falls_to_runtime() {
    // Nothing is known about `x`, so the precondition may or may not hold.
    let result = only_call_site(
        "#[mvl::requires(n > 0)]\n\
         fn require_positive(n: i32) -> i32 { n }\n\
         fn caller(x: i32) -> i32 { require_positive(x) }",
    );
    assert_eq!(result, DischargeResult::Runtime);
}

#[test]
fn a_caller_precondition_entails_the_callees() {
    let result = only_call_site(
        "#[mvl::requires(n > 0)]\n\
         fn require_positive(n: i32) -> i32 { n }\n\
         #[mvl::requires(x > 10)]\n\
         fn caller(x: i32) -> i32 { require_positive(x) }",
    );
    assert_proven_at(&result, Layer::L2);
}

#[test]
fn branch_narrowing_proves_a_call_inside_the_then_arm() {
    // L2's own "branch narrowing" scenario: nothing is declared about `x`,
    // but inside `if x > 0` it is known.
    let result = only_call_site(
        "#[mvl::requires(n > 0)]\n\
         fn require_positive(n: i32) -> i32 { n }\n\
         fn caller(x: i32) -> i32 { if x > 0 { require_positive(x) } else { 0 } }",
    );
    assert_proven_at(&result, Layer::L2);
}

#[test]
fn branch_narrowing_negates_the_condition_in_the_else_arm() {
    // The `else` arm knows `!(x > 0)`, i.e. `x <= 0` -- which makes a call
    // needing `n > 0` provably impossible, not merely unproven.
    let result = only_call_site(
        "#[mvl::requires(n > 0)]\n\
         fn require_positive(n: i32) -> i32 { n }\n\
         fn caller(x: i32) -> i32 { if x > 0 { 0 } else { require_positive(x) } }",
    );
    assert!(
        matches!(result, DischargeResult::Violated { .. }),
        "got {result:?}"
    );
}

#[test]
fn narrowing_does_not_leak_out_of_the_branch() {
    // Two calls: the one inside the `if` is proven, the one after it is not
    // -- the condition must not survive past the block that established it.
    let sites = call_sites(
        "#[mvl::requires(n > 0)]\n\
         fn require_positive(n: i32) -> i32 { n }\n\
         fn caller(x: i32) -> i32 { if x > 0 { require_positive(x); } require_positive(x) }",
    );
    assert_eq!(sites.len(), 2, "got: {sites:?}");
    assert_proven_at(&sites[0].1, Layer::L2);
    assert_eq!(sites[1].1, DischargeResult::Runtime);
}

#[test]
fn else_if_chains_accumulate_the_preceding_negations() {
    // In the final `else`, both `x > 100` and `x > 0` are known false, so
    // `x <= 0` holds and a call needing `n > 0` is provably impossible.
    let sites = call_sites(
        "#[mvl::requires(n > 0)]\n\
         fn require_positive(n: i32) -> i32 { n }\n\
         fn caller(x: i32) -> i32 {\n\
           if x > 100 { 1 } else if x > 0 { require_positive(x) } else { require_positive(x) }\n\
         }",
    );
    assert_eq!(sites.len(), 2, "got: {sites:?}");
    assert_proven_at(&sites[0].1, Layer::L2);
    assert!(
        matches!(sites[1].1, DischargeResult::Violated { .. }),
        "final else arm: got {:?}",
        sites[1].1
    );
}

#[test]
fn a_while_condition_narrows_its_body() {
    let result = only_call_site(
        "#[mvl::requires(n > 0)]\n\
         fn require_positive(n: i32) -> i32 { n }\n\
         fn caller(x: i32) { while x > 0 { require_positive(x); } }",
    );
    assert_proven_at(&result, Layer::L2);
}

#[test]
fn a_callees_postcondition_becomes_a_fact_about_the_binding() {
    // `produce`'s `ensures` is what makes the second call provable: nothing
    // is declared about the caller's own parameters at all.
    let sites = call_sites(
        "#[mvl::ensures(result > 0)]\n\
         fn produce() -> i32 { 1 }\n\
         #[mvl::requires(n > 0)]\n\
         fn require_positive(n: i32) -> i32 { n }\n\
         fn caller() -> i32 { let y = produce(); require_positive(y) }",
    );
    assert_eq!(sites.len(), 1, "got: {sites:?}");
    assert_proven_at(&sites[0].1, Layer::L2);
}

#[test]
fn a_postcondition_mentioning_parameters_is_substituted_with_the_arguments() {
    // `ensures(result > x)` with `x := 10` gives `y > (10)` -- a constant
    // bound once the argument is substituted, so L2 closes it. Without the
    // substitution it would be the cross-variable `y > x`, which L2 cannot
    // read at all; that difference is what this test is really checking.
    let sites = call_sites(
        "#[mvl::ensures(result > x)]\n\
         fn at_least(x: i32) -> i32 { x + 1 }\n\
         #[mvl::requires(n > 5)]\n\
         fn require_gt5(n: i32) -> i32 { n }\n\
         fn caller() -> i32 { let y = at_least(10); require_gt5(y) }",
    );
    assert_eq!(sites.len(), 1, "got: {sites:?}");
    assert_proven_at(&sites[0].1, Layer::L2);
}

#[test]
fn a_postcondition_does_not_outlive_its_block() {
    let sites = call_sites(
        "#[mvl::ensures(result > 0)]\n\
         fn produce() -> i32 { 1 }\n\
         #[mvl::requires(n > 0)]\n\
         fn require_positive(n: i32) -> i32 { n }\n\
         fn caller(y: i32) -> i32 { { let y = produce(); require_positive(y); } require_positive(y) }",
    );
    assert_eq!(sites.len(), 2, "got: {sites:?}");
    assert_proven_at(&sites[0].1, Layer::L2);
    assert_eq!(sites[1].1, DischargeResult::Runtime);
}

// ── Scope boundaries, asserted so they stay deliberate ────────────────────

#[test]
fn a_call_to_an_unresolvable_function_produces_no_obligation() {
    // Same boundary `rust-effect` draws: no cross-file resolution, so a
    // call to something not defined here is silently skipped rather than
    // guessed at.
    assert!(call_sites("fn caller(x: i32) { external_crate_fn(x); }").is_empty());
}

#[test]
fn a_call_inside_a_macro_invocation_is_invisible() {
    // `syn` keeps a macro body as an opaque token stream, so there is no
    // call expression to find. Asserted so the boundary is deliberate
    // rather than a surprise: this is the most common way a real call site
    // escapes the scan.
    assert!(call_sites(
        "#[mvl::requires(n > 0)]\n\
         fn require_positive(n: i32) -> i32 { n }\n\
         fn caller() { println!(\"{}\", require_positive(-5)); }"
    )
    .is_empty());
}

#[test]
fn a_callee_with_no_precondition_produces_no_obligation() {
    assert!(call_sites("fn g(n: i32) -> i32 { n } fn caller() -> i32 { g(1) }").is_empty());
}

#[test]
fn an_arity_mismatch_produces_no_obligation() {
    // Wrong arity is rustc's error to report, not ours -- and substitution
    // would be meaningless, so nothing is claimed about the call.
    assert!(call_sites(
        "#[mvl::requires(a > 0 && b > 0)]\n\
         fn g(a: i32, b: i32) -> i32 { a }\n\
         fn caller() -> i32 { g(1) }"
    )
    .is_empty());
}

#[test]
fn a_nested_fn_does_not_inherit_the_enclosing_gamma() {
    // `inner`'s call must NOT be provable: the `x > 0` belongs to `outer`'s
    // parameter, and `inner`'s own `x` is a different variable entirely.
    let result = only_call_site(
        "#[mvl::requires(n > 0)]\n\
         fn require_positive(n: i32) -> i32 { n }\n\
         #[mvl::requires(x > 0)]\n\
         fn outer(x: i32) -> i32 { fn inner(x: i32) -> i32 { require_positive(x) } inner(x) }",
    );
    assert_eq!(result, DischargeResult::Runtime);
}

#[test]
fn declaration_site_obligations_are_still_reported_alongside_call_sites() {
    // The two kinds coexist: this file has one `requires` declaration and
    // one call site, and both appear.
    let found = find_obligations(
        "#[mvl::requires(n > 0)]\n\
         fn require_positive(n: i32) -> i32 { n }\n\
         fn caller(x: i32) -> i32 { require_positive(x) }",
    )
    .unwrap();

    assert_eq!(found.len(), 2, "got: {found:#?}");
    assert_eq!(found[0].kind, ObligationKind::Requires);
    assert_eq!(
        found[1].kind,
        ObligationKind::CallSite {
            callee: "require_positive".to_string()
        }
    );
}

#[test]
fn a_quantified_precondition_is_a_usable_call_site_goal() {
    // The goal is quantified, so it expands at L3 -- one entailment query
    // per instance, each against the same Γ.
    let result = only_call_site(
        "#[mvl::requires(forall i in [0..3] . i < n)]\n\
         fn require_gt_all(n: i32) -> i32 { n }\n\
         #[mvl::requires(x > 100)]\n\
         fn caller(x: i32) -> i32 { require_gt_all(x) }",
    );
    assert_proven_at(&result, Layer::L3);
}

// ── Γ invalidation: a fact only survives while its variable does ──────────
//
// Γ's clauses describe named variables. The moment a name is rebound or
// mutated, whatever Γ recorded about it describes a value that is no longer
// there -- and a stale hypothesis is strictly worse than a missing one,
// because it proves goals that are actually false. Each of these reported
// `Proven` before #40's review.

#[test]
fn a_shadowing_let_invalidates_the_hypotheses_about_that_name() {
    let result = only_call_site(
        "#[mvl::requires(x > 10)]\n\
         fn caller(x: i64) { let x = -5; require_positive(x); }\n\
         #[mvl::requires(v > 0)]\n\
         fn require_positive(v: i64) {}",
    );
    assert!(
        !matches!(result, DischargeResult::Proven { .. }),
        "`let x = -5` must retire `x > 10`, got {result:?}"
    );
}

#[test]
fn assigning_a_mut_parameter_invalidates_its_hypotheses() {
    let result = only_call_site(
        "#[mvl::requires(x > 10)]\n\
         fn caller(mut x: i64) { x = -5; require_positive(x); }\n\
         #[mvl::requires(v > 0)]\n\
         fn require_positive(v: i64) {}",
    );
    assert!(
        !matches!(result, DischargeResult::Proven { .. }),
        "`x = -5` must retire `x > 10`, got {result:?}"
    );
}

#[test]
fn compound_assignment_invalidates_its_hypotheses() {
    // `-=` is a `Binary` node in syn, not an `Assign` one.
    let result = only_call_site(
        "#[mvl::requires(x > 10)]\n\
         fn caller(mut x: i64) { x -= 100; require_positive(x); }\n\
         #[mvl::requires(v > 0)]\n\
         fn require_positive(v: i64) {}",
    );
    assert!(
        !matches!(result, DischargeResult::Proven { .. }),
        "`x -= 100` must retire `x > 10`, got {result:?}"
    );
}

#[test]
fn a_mutable_borrow_invalidates_its_hypotheses() {
    // This backend cannot see whether `bump` writes through the reference,
    // so it assumes the worst.
    let result = only_call_site(
        "#[mvl::requires(x > 10)]\n\
         fn caller(mut x: i64) { bump(&mut x); require_positive(x); }\n\
         fn bump(v: &mut i64) {}\n\
         #[mvl::requires(v > 0)]\n\
         fn require_positive(v: i64) {}",
    );
    assert!(
        !matches!(result, DischargeResult::Proven { .. }),
        "`&mut x` must retire `x > 10`, got {result:?}"
    );
}

#[test]
fn shadowing_a_binding_retires_its_propagated_postcondition() {
    let result = only_call_site(
        "#[mvl::ensures(result > 0)]\n\
         fn produce(a: i64) -> i64 { 1 }\n\
         fn caller(a: i64) { let y = produce(a); let y = -1; require_positive(y); }\n\
         #[mvl::requires(v > 0)]\n\
         fn require_positive(v: i64) {}",
    );
    assert!(
        !matches!(result, DischargeResult::Proven { .. }),
        "the second `let y` must retire `produce`'s postcondition, got {result:?}"
    );
}

#[test]
fn an_unrelated_assignment_leaves_other_hypotheses_intact() {
    // Invalidation is per-name, not a blanket reset -- otherwise the fix
    // above would quietly disable call-site checking for any function that
    // assigns anything.
    let result = only_call_site(
        "#[mvl::requires(x > 10)]\n\
         fn caller(x: i64, mut other: i64) { other = -5; require_positive(x); }\n\
         #[mvl::requires(v > 0)]\n\
         fn require_positive(v: i64) {}",
    );
    assert_proven_at(&result, Layer::L2);
}

#[test]
fn a_local_binding_shadowing_a_free_fn_is_not_resolved_against_it() {
    // `callee` here is a closure; the free function of the same name is a
    // different thing entirely. Resolving to it reported a hard error on
    // correct code.
    let sites = call_sites(
        "fn caller() {\n\
         let require_positive = |v: i64| v;\n\
         require_positive(-5);\n\
         }\n\
         #[mvl::requires(v > 0)]\n\
         fn require_positive(v: i64) -> i64 { v }",
    );
    assert!(
        sites.is_empty(),
        "a shadowed name must not resolve to the free fn, got {sites:?}"
    );
}

// ── Return-site obligations (#42) ─────────────────────────────────────────
//
// `ensures` used to be checked only for internal coherence, never against the
// body. Since #38 propagates a callee's postcondition into the caller's Γ,
// an unverified `ensures` was not merely bad documentation -- it was a premise
// the solver proved other things from. These pin the return points that now
// have to establish it.
//
// The mechanism is the mirror of `propagate_postcondition`: bind `result` to
// the returned expression instead of to a `let` binding, then discharge
// against Γ as it stands at that point in the body.

/// Every return-site obligation in `source`, as `(rendered goal, outcome)`.
fn return_sites(source: &str) -> Vec<(String, DischargeResult)> {
    find_obligations(source)
        .expect("fixture parses")
        .iter()
        .filter(|found| found.kind == ObligationKind::ReturnSite)
        .map(|found| (found.predicate_text(), found.discharge()))
        .collect()
}

/// The single return-site outcome in `source` -- asserts there is exactly
/// one, so a fixture that stops producing an obligation fails loudly.
fn only_return_site(source: &str) -> DischargeResult {
    let sites = return_sites(source);
    assert_eq!(
        sites.len(),
        1,
        "expected exactly one return-site obligation, got {sites:?}"
    );
    sites.into_iter().next().unwrap().1
}

#[test]
fn a_body_contradicting_its_own_ensures_is_violated() {
    // The motivating case from #42. Before return sites existed this whole
    // file reported green: the `ensures` line said "discharged at L2",
    // meaning `result > 0` is coherent, and nothing read the `-1`.
    let result = only_return_site(
        "#[mvl::ensures(result > 0)]\n\
         fn always_positive(a: i64) -> i64 { -1 }",
    );
    assert!(
        matches!(result, DischargeResult::Violated { .. }),
        "a body returning -1 cannot establish `result > 0`, got {result:?}"
    );
}

#[test]
fn a_body_establishing_its_ensures_is_proven() {
    assert_proven_at(
        &only_return_site(
            "#[mvl::ensures(result > 0)]\n\
             fn five(a: i64) -> i64 { 5 }",
        ),
        Layer::L1,
    );
}

#[test]
fn a_functional_postcondition_closes_by_reflexivity() {
    // The shape #43 existed to make provable, and the reason it blocked this
    // issue: substitution produces `(a + b) == a + b`, grouped on one side,
    // which only L1 structural reflexivity reaches. Every functional
    // postcondition was `Runtime` before #44.
    assert_proven_at(
        &only_return_site(
            "#[mvl::ensures(result == a + b)]\n\
             fn add(a: i64, b: i64) -> i64 { a + b }",
        ),
        Layer::L1,
    );
}

#[test]
fn a_postcondition_over_a_call_stays_runtime_until_the_purity_signal_lands() {
    // Known limitation, not a defect: #44 gated L1 reflexivity to call-free
    // terms because reflexivity is unsound for an impure term. A body that
    // returns a call therefore cannot be discharged by tree comparison, and
    // return expressions are very often calls. #45 tracks the real fix.
    let result = only_return_site(
        "#[mvl::ensures(result == double(a))]\n\
         fn twice(a: i64) -> i64 { double(a) }\n\
         fn double(x: i64) -> i64 { x * 2 }",
    );
    assert_eq!(result, DischargeResult::Runtime);
}

#[test]
fn an_explicit_return_is_a_return_point() {
    let result = only_return_site(
        "#[mvl::ensures(result > 0)]\n\
         fn f(a: i64) -> i64 { return -7; }",
    );
    assert!(
        matches!(result, DischargeResult::Violated { .. }),
        "expected Violated, got {result:?}"
    );
}

#[test]
fn the_postcondition_is_discharged_against_the_callers_own_requires() {
    // Γ at a return point starts from the function's own precondition, so a
    // postcondition that follows from it proves without any literal in play.
    assert_proven_at(
        &only_return_site(
            "#[mvl::requires(a > 100)]\n\
             #[mvl::ensures(result > 50)]\n\
             fn f(a: i64) -> i64 { a }",
        ),
        Layer::L2,
    );
}

#[test]
fn a_return_inside_a_narrowed_branch_uses_that_branchs_gamma() {
    // `a` alone establishes nothing; `a` under the `then` branch of
    // `if a > 10` does. Both arms are return points and both must close.
    let sites = return_sites(
        "#[mvl::ensures(result > 0)]\n\
         fn f(a: i64) -> i64 { if a > 10 { a } else { 1 } }",
    );
    assert_eq!(sites.len(), 2, "both arms are return points, got {sites:?}");
    for (goal, result) in &sites {
        assert!(
            matches!(result, DischargeResult::Proven { .. }),
            "`{goal}` should be proven under its branch, got {result:?}"
        );
    }
}

#[test]
fn each_tail_match_arm_is_its_own_return_point() {
    // Arm patterns do not narrow Γ (module doc's scope note), so each arm is
    // discharged against the enclosing Γ -- imprecise, never unsound. The
    // `-3` arm is still caught, because a literal needs no hypotheses.
    let sites = return_sites(
        "#[mvl::ensures(result > 0)]\n\
         fn f(a: i64) -> i64 { match a { 0 => -3, _ => 2 } }",
    );
    assert_eq!(sites.len(), 2, "one per arm, got {sites:?}");
    assert!(sites
        .iter()
        .any(|(_, r)| matches!(r, DischargeResult::Violated { .. })));
}

#[test]
fn a_diverging_body_has_no_return_point_to_check() {
    // `panic!()` produces no `result`, so there is nothing to substitute and
    // the postcondition holds vacuously -- the same conclusion the solver
    // reaches for an unreachable program point (ADR-0005).
    for body in [
        "panic!(\"no\")",
        "todo!()",
        "unimplemented!()",
        "unreachable!()",
    ] {
        let source = format!(
            "#[mvl::ensures(result > 0)]\n\
             fn f(a: i64) -> i64 {{ {body} }}"
        );
        let sites = return_sites(&source);
        assert!(
            sites.is_empty(),
            "`{body}` is not a return point, got {sites:?}"
        );
    }
}

#[test]
fn a_closure_body_is_not_the_enclosing_functions_return_point() {
    // The trap in tail-position tracking: `syn` descends into a closure by
    // default, so an unclear flag would report the closure's `-1` as a
    // violating return of `f`. A false return-site violation is an error that
    // fails the build, so this is the louder failure mode of the two.
    let sites = return_sites(
        "#[mvl::ensures(result > 0)]\n\
         fn f(a: i64) -> i64 { let g = |x: i64| -1; 7 }",
    );
    assert_eq!(sites.len(), 1, "only `7` returns from `f`, got {sites:?}");
    assert!(matches!(sites[0].1, DischargeResult::Proven { .. }));
}

#[test]
fn an_explicit_return_inside_a_closure_is_not_the_enclosing_functions_return() {
    // The case the tail-position test above misses. Clearing `in_tail` alone
    // is not enough: `visit_expr_return` fires wherever a `return` appears,
    // so it needs its own `returns_here` gate. Without it this reported the
    // closure's `-1` as a violating return of `f` -- a `Level::Error` that
    // fails the build on correct code, which is the direction #42 explicitly
    // set out not to fail in.
    let sites = return_sites(
        "#[mvl::ensures(result > 0)]\n\
         fn f(a: i64) -> i64 { let g = |x: i64| { return -1; }; 7 }",
    );
    assert_eq!(sites.len(), 1, "only `7` returns from `f`, got {sites:?}");
    assert!(matches!(sites[0].1, DischargeResult::Proven { .. }));
}

#[test]
fn an_explicit_return_inside_an_async_block_is_not_the_functions_return() {
    // An `async` block evaluates to a future, so a `return` inside it returns
    // from the future's body, not from `f`.
    let sites = return_sites(
        "#[mvl::ensures(result > 0)]\n\
         fn f(a: i64) -> i64 { let fut = async { return -1; }; 7 }",
    );
    assert_eq!(sites.len(), 1, "only `7` returns from `f`, got {sites:?}");
    assert!(matches!(sites[0].1, DischargeResult::Proven { .. }));
}

#[test]
fn a_nested_fn_inside_a_closure_still_owns_its_own_returns() {
    // `returns_here` is cleared for the closure body, but a nested `fn` in
    // there is its own return target and must re-establish it -- otherwise
    // the gate that fixes the case above would silently swallow real
    // violations.
    let found = find_obligations(
        "fn outer() {\n\
             let g = || {\n\
                 #[mvl::ensures(result > 0)]\n\
                 fn inner(b: i64) -> i64 { return -1; }\n\
                 inner(1)\n\
             };\n\
         }",
    )
    .expect("fixture parses");
    let sites: Vec<_> = found
        .iter()
        .filter(|f| f.kind == ObligationKind::ReturnSite)
        .map(|f| (f.fn_name.clone(), f.discharge()))
        .collect();
    assert_eq!(sites.len(), 1, "`inner`'s own return, got {sites:?}");
    assert_eq!(sites[0].0, "inner");
    assert!(
        matches!(sites[0].1, DischargeResult::Violated { .. }),
        "`(-1) > 0` is false and must still be caught, got {:?}",
        sites[0].1
    );
}

#[test]
fn a_while_body_is_not_a_return_point_but_a_return_inside_it_is() {
    // A `while` evaluates to `()`, so nothing in its body becomes the return
    // value except through an explicit `return` -- which still gets the
    // loop condition in Γ.
    let sites = return_sites(
        "#[mvl::ensures(result > 0)]\n\
         fn f(a: i64) -> i64 { while a > 0 { return a; } 1 }",
    );
    assert_eq!(
        sites.len(),
        2,
        "the `return a` and the tail `1`, got {sites:?}"
    );
    for (goal, result) in &sites {
        assert!(
            matches!(result, DischargeResult::Proven { .. }),
            "`{goal}` should prove, got {result:?}"
        );
    }
}

#[test]
fn a_block_in_statement_position_is_not_a_return_point() {
    // Only a *trailing* expression carries the block's value outwards.
    let sites = return_sites(
        "#[mvl::ensures(result > 0)]\n\
         fn f(a: i64) -> i64 { { let _q = -5; } 3 }",
    );
    assert_eq!(sites.len(), 1, "only the tail `3`, got {sites:?}");
    assert!(matches!(sites[0].1, DischargeResult::Proven { .. }));
}

#[test]
fn a_nested_block_in_tail_position_forwards_through() {
    let result = only_return_site(
        "#[mvl::ensures(result > 0)]\n\
         fn f(a: i64) -> i64 { { -2 } }",
    );
    assert!(
        matches!(result, DischargeResult::Violated { .. }),
        "the inner `-2` is the return value, got {result:?}"
    );
}

#[test]
fn a_nested_fns_postcondition_does_not_leak_to_its_parent() {
    // A nested `fn` has its own contract and its own Γ; its return points
    // establish *its* `ensures`, attributed to its own name.
    let found = find_obligations(
        "#[mvl::ensures(result > 0)]\n\
         fn outer(a: i64) -> i64 {\n\
             #[mvl::ensures(result > 100)]\n\
             fn inner(b: i64) -> i64 { 5 }\n\
             9\n\
         }",
    )
    .expect("fixture parses");
    let sites: Vec<_> = found
        .iter()
        .filter(|f| f.kind == ObligationKind::ReturnSite)
        .map(|f| (f.fn_name.clone(), f.predicate_text(), f.discharge()))
        .collect();
    assert_eq!(sites.len(), 2, "one per function, got {sites:?}");
    let inner = sites.iter().find(|(n, ..)| n == "inner").expect("inner");
    assert!(
        matches!(inner.2, DischargeResult::Violated { .. }),
        "`5 > 100` is false, got {:?}",
        inner.2
    );
    let outer = sites.iter().find(|(n, ..)| n == "outer").expect("outer");
    assert!(matches!(outer.2, DischargeResult::Proven { .. }));
}

#[test]
fn a_function_without_ensures_produces_no_return_obligation() {
    assert!(return_sites("fn f(a: i64) -> i64 { -1 }").is_empty());
}

#[test]
fn a_violated_return_site_is_an_error_and_fails_the_build() {
    // Only `Level::Error` fails the Gate, so this is what makes #42 more
    // than a reporting change.
    let diagnostics = check_source(
        "#[mvl::ensures(result > 0)]\n\
         fn f(a: i64) -> i64 { -1 }",
    )
    .expect("fixture parses");
    assert!(
        diagnostics.iter().any(|d| d.level == Level::Error
            && d.message.contains("postcondition")
            && d.message.contains("cannot hold")),
        "expected a postcondition error, got {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn an_informational_outcome_does_not_fail_the_build() {
    // Spec 008 Requirement 1: only `Level::Error` fails the build. A file whose
    // obligations are all proven or all deferred to a runtime check must not
    // produce an error, which is what gate mode keys on. `mask_low_nibble`'s
    // shape is the interesting half: `&` is outside the linear fragment, so its
    // return site is undischarged — and that must still be a note, not an error.
    let diagnostics = check_source(
        "#[mvl::requires(0 <= b && b <= 255)]\n\
         #[mvl::ensures(0 <= result && result <= 15)]\n\
         fn mask_low_nibble(b: i64) -> i64 { b & 15 }",
    )
    .expect("fixture parses");
    assert!(
        !diagnostics.is_empty(),
        "expected informational diagnostics to be reported at all"
    );
    assert!(
        !diagnostics.iter().any(|d| d.level == Level::Error),
        "an undischarged obligation must be informational, not build-failing; got {:?}",
        diagnostics
            .iter()
            .filter(|d| d.level == Level::Error)
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ── Γ's soundness invariant (#47) ─────────────────────────────────────────
//
// ADR-0006 §5: a fact is admitted to Γ only if it has been established, or is
// an obligation some other program point is required to discharge. A
// postcondition whose own return site did not close is neither — `rust-refine`
// inserts no runtime check, so nothing anywhere enforces it.

#[test]
fn an_unenforced_postcondition_does_not_enter_gamma() {
    // Spec 007 Requirement 2. `b & 15` is at most 15, so `result > 100` is
    // false for every input — and `&` is outside the linear fragment, so the
    // return site cannot be discharged either way. Before #47 the caller
    // picked it up regardless and reported `(y) > 50` as "proven at L2" from
    // a premise false for every `i64`.
    //
    // Since #69, an *undischarged-but-enforced* postcondition (no
    // `#[mvl::unchecked]`) now legitimately propagates — see
    // `an_enforced_but_undischarged_postcondition_now_enters_gamma` below,
    // which is exactly this fixture without the opt-out. "Unenforced" is
    // the narrower, genuinely-uncovered case this test now pins:
    // `#[mvl::unchecked]` means no runtime check exists at all, so nothing
    // backs the postcondition either statically or at runtime.
    let result = only_call_site(
        "#[mvl::unchecked]\n\
         #[mvl::ensures(result > 100)]\n\
         fn suspicious(b: i64) -> i64 { b & 15 }\n\
         #[mvl::requires(v > 50)]\n\
         fn needs_big(v: i64) {}\n\
         fn caller(b: i64) { let y = suspicious(b); needs_big(y); }",
    );
    assert_eq!(
        result,
        DischargeResult::Runtime,
        "a genuinely unenforced postcondition must not prove a downstream call"
    );
}

#[test]
fn an_enforced_but_undischarged_postcondition_now_enters_gamma() {
    // #69: the same fixture as `an_unenforced_postcondition_does_not_enter_gamma`,
    // minus `#[mvl::unchecked]`. `suspicious` carries `#[mvl::ensures]` and is
    // not opted out, so it is enforced regardless of the fact that `b & 15`
    // leaves its own return site undischarged (`&` is outside the linear
    // fragment) -- ADR-0006 Section 5's soundness argument for enforcement is
    // unconditional: an `assert!` at the return means "either the
    // postcondition holds, or the process aborted", which licenses
    // propagation even though no static layer closed it.
    //
    // Requirement 6: the caller's own call-site obligation must not be
    // reported as a plain proof just because the solver says `Proven` --
    // it rests on `suspicious`'s enforcement, and the wire-facing `Warrant`
    // must say so explicitly.
    let (result, warrant) = only_call_site_warrant(
        "#[mvl::ensures(result > 100)]\n\
         fn suspicious(b: i64) -> i64 { b & 15 }\n\
         #[mvl::requires(v > 50)]\n\
         fn needs_big(v: i64) {}\n\
         fn caller(b: i64) { let y = suspicious(b); needs_big(y); }",
    );
    assert!(
        matches!(result, DischargeResult::Proven { .. }),
        "an enforced postcondition must propagate even though its own return site is undischarged, got {result:?}"
    );
    assert_eq!(
        warrant,
        Warrant::Enforcement {
            premises: vec!["suspicious".to_string()]
        },
        "a proof resting on suspicious's enforcement must not be reported as a plain proof"
    );
}

#[test]
fn an_established_postcondition_still_enters_gamma() {
    // The gate must cost nothing where the callee does deliver: `(1) > 0`
    // closes at L1, so `produce`'s postcondition is established and remains a
    // usable premise. Without this the fix would silently disable propagation
    // altogether, which passes the test above for the wrong reason.
    //
    // #69: also a regression guard the other way -- a proof that never
    // touched an enforced-not-proven premise must stay a real `Proof`, not
    // get swept into `Enforcement` just because propagation happened at all.
    let (result, warrant) = only_call_site_warrant(
        "#[mvl::ensures(result > 0)]\n\
         fn produce() -> i64 { 1 }\n\
         #[mvl::requires(v > 0)]\n\
         fn need_pos(v: i64) {}\n\
         fn caller() { let y = produce(); need_pos(y); }",
    );
    assert!(
        matches!(result, DischargeResult::Proven { .. }),
        "an established postcondition must still propagate, got {result:?}"
    );
    assert_eq!(
        warrant,
        Warrant::Proof,
        "a fully statically-established chain must remain a real proof, got {warrant:?}"
    );
}

#[test]
fn a_red_herring_enforced_hypothesis_does_not_taint_an_unrelated_proof() {
    // #69's exactness guarantee, not the coarse "any tainted clause present"
    // approximation it replaces: `suspicious`'s enforced-not-proven fact
    // `y > 100` sits in Γ when `need_pos(z)` is checked, but the goal `z > 0`
    // is proven entirely from the branch-narrowing fact `z > 0` -- the
    // tainted hypothesis is never actually used. This must stay a real
    // `Proof`, not get swept into `Enforcement` just because an unrelated
    // enforced fact happened to coexist in Γ.
    let (result, warrant) = only_call_site_warrant(
        "#[mvl::ensures(result > 100)]\n\
         fn suspicious(b: i64) -> i64 { b & 15 }\n\
         #[mvl::requires(v > 0)]\n\
         fn need_pos(v: i64) {}\n\
         fn caller(b: i64, z: i64) {\n\
             let y = suspicious(b);\n\
             if z > 0 {\n\
                 need_pos(z);\n\
             }\n\
         }",
    );
    assert!(
        matches!(result, DischargeResult::Proven { .. }),
        "expected `z > 0` to close from branch narrowing, got {result:?}"
    );
    assert_eq!(
        warrant,
        Warrant::Proof,
        "an unrelated enforced fact in Γ must not taint a proof that never used it, got {warrant:?}"
    );
}

#[test]
fn two_jointly_necessary_enforced_premises_are_both_named() {
    // #69: a conjunctive goal needing *both* enforced-not-proven facts at
    // once -- neither alone suffices, so leave-one-out against the full
    // hypothesis set finds each individually necessary, and both are named.
    let (result, warrant) = only_call_site_warrant(
        "#[mvl::ensures(result > 0)]\n\
         fn get_x(a: i64) -> i64 { a & 1 }\n\
         #[mvl::ensures(result > 0)]\n\
         fn get_y(a: i64) -> i64 { a & 1 }\n\
         #[mvl::requires(v > 0 && w > 0)]\n\
         fn need_both(v: i64, w: i64) {}\n\
         fn caller(a: i64) {\n\
             let x = get_x(a);\n\
             let y = get_y(a);\n\
             need_both(x, y);\n\
         }",
    );
    assert!(
        matches!(result, DischargeResult::Proven { .. }),
        "expected the conjunction to close using both propagated facts, got {result:?}"
    );
    match warrant {
        Warrant::Enforcement { mut premises } => {
            premises.sort();
            assert_eq!(premises, vec!["get_x".to_string(), "get_y".to_string()]);
        }
        other => panic!("expected Enforcement naming both premises, got {other:?}"),
    }
}

#[test]
fn a_violated_postcondition_does_not_enter_gamma_either() {
    // The other half of "not established": a return site that is definitely
    // `Violated` is no more usable as a premise than one that is merely
    // undischarged, *for a function with no enforcement at all*. The callee
    // is an error in its own right; that must not also license a proof in
    // the caller.
    //
    // Since #69, this specifically needs `#[mvl::unchecked]` to stay blocked
    // -- see `a_violated_but_enforced_postcondition_still_propagates_soundly`
    // below for why an *enforced* `Violated` return site is safe to
    // propagate after all (the assert backstops it too).
    let result = only_call_site(
        "#[mvl::unchecked]\n\
         #[mvl::ensures(result > 0)]\n\
         fn always_negative() -> i64 { -1 }\n\
         #[mvl::requires(v > 0)]\n\
         fn need_pos(v: i64) {}\n\
         fn caller() { let y = always_negative(); need_pos(y); }",
    );
    assert_eq!(result, DischargeResult::Runtime);
}

#[test]
fn a_violated_but_enforced_postcondition_still_propagates_soundly() {
    // #69: `always_negative`'s body always returns `-1`, so its own return
    // site is a demonstrated `Violated`, not merely undischarged -- and
    // yet, since it is *enforced* (no `#[mvl::unchecked]`), every actual
    // call to it aborts every time. ADR-0006 Section 5's argument covers
    // this the same way it covers a diverging body (see
    // `a_diverging_body_propagates_because_the_continuation_is_unreachable`
    // above): the caller's continuation after the call is unreachable in
    // any real execution, so assuming its postcondition there is vacuous,
    // not unsound.
    let (result, warrant) = only_call_site_warrant(
        "#[mvl::ensures(result > 0)]\n\
         fn always_negative() -> i64 { -1 }\n\
         #[mvl::requires(v > 0)]\n\
         fn need_pos(v: i64) {}\n\
         fn caller() { let y = always_negative(); need_pos(y); }",
    );
    assert!(
        matches!(result, DischargeResult::Proven { .. }),
        "an enforced postcondition must propagate even from a demonstrably violated return site, got {result:?}"
    );
    assert_eq!(
        warrant,
        Warrant::Enforcement {
            premises: vec!["always_negative".to_string()]
        }
    );
}

#[test]
fn a_runtime_outcome_does_not_claim_a_check_was_inserted() {
    // Spec 007 Requirement 1. The tool emits no runtime check, so a diagnostic
    // saying it does is a claim nothing backs — and it is what made an
    // unenforced fact read as usable in the first place (#47).
    //
    // Since #69 this specifically needs `#[mvl::unchecked]`: without it,
    // `mask_low_nibble` (a real `#53` example) is enforced by `mvl-macros`
    // regardless of this return site being undischarged, and the correct
    // diagnostic says so -- see
    // `an_enforced_runtime_outcome_says_so_instead_of_unverified` below.
    let diagnostics = check_source(
        "#[mvl::unchecked]\n\
         #[mvl::requires(0 <= b && b <= 255)]\n\
         #[mvl::ensures(0 <= result && result <= 15)]\n\
         fn mask_low_nibble(b: i64) -> i64 { b & 15 }",
    )
    .expect("fixture parses");
    let runtime: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("is not established"))
        .collect();
    assert!(!runtime.is_empty(), "expected an undischarged return site");
    for d in runtime {
        assert!(
            !d.message.contains("inserting a runtime check"),
            "must not claim a check was inserted: {}",
            d.message
        );
        assert!(
            d.message.contains("unverified"),
            "must say the obligation is unverified: {}",
            d.message
        );
    }
}

#[test]
fn an_enforced_runtime_outcome_says_so_instead_of_unverified() {
    // #69: the same fixture, minus `#[mvl::unchecked]`. `mask_low_nibble`
    // carries `#[mvl::ensures]` and is not opted out, so `mvl-macros` (#53)
    // really does inject an `assert!` here -- calling this "unverified"
    // would itself be the stale claim once #53 shipped. `rust-refine` still
    // never claims *it* inserted anything; it names the enforcement rather
    // than asserting or denying its existence in the abstract.
    let diagnostics = check_source(
        "#[mvl::requires(0 <= b && b <= 255)]\n\
         #[mvl::ensures(0 <= result && result <= 15)]\n\
         fn mask_low_nibble(b: i64) -> i64 { b & 15 }",
    )
    .expect("fixture parses");
    let enforced: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("is not established statically"))
        .collect();
    assert!(
        !enforced.is_empty(),
        "expected an undischarged-but-enforced return site"
    );
    for d in enforced {
        assert!(
            d.message.contains("mask_low_nibble"),
            "must name the enforcing function: {}",
            d.message
        );
        assert!(
            !d.message.contains("unverified"),
            "must not call an enforced obligation unverified: {}",
            d.message
        );
    }
}

// ── Γ construction: shadowing, capture, loop-carried mutation (#50) ───────
//
// Three classes of unestablished fact, all producing a false `Proven` on
// compiling code, all found by the audit that produced #47's invariant. They
// share a root cause: Γ retained a fact about a name whose value had changed.

#[test]
fn a_for_pattern_shadows_the_hypothesis_about_that_name() {
    let result = only_call_site(
        "#[mvl::requires(x > 10)]\n\
         fn f(x: i64) { for x in -5..0 { need_pos(x); } }\n\
         #[mvl::requires(v > 0)]\n\
         fn need_pos(v: i64) {}",
    );
    assert_eq!(result, DischargeResult::Runtime);
}

#[test]
fn a_closure_parameter_shadows_the_hypothesis_about_that_name() {
    let result = only_call_site(
        "#[mvl::requires(x > 10)]\n\
         fn f(x: i64) { let g = |x: i64| need_pos(x); }\n\
         #[mvl::requires(v > 0)]\n\
         fn need_pos(v: i64) {}",
    );
    assert_eq!(result, DischargeResult::Runtime);
}

#[test]
fn a_match_arm_binding_shadows_the_hypothesis_about_that_name() {
    let result = only_call_site(
        "#[mvl::requires(x > 10)]\n\
         fn f(x: i64, o: Option<i64>) { match o { Some(x) => { need_pos(x); }, None => {} } }\n\
         #[mvl::requires(v > 0)]\n\
         fn need_pos(v: i64) {}",
    );
    assert_eq!(result, DischargeResult::Runtime);
}

#[test]
fn an_if_let_binding_shadows_the_hypothesis_about_that_name() {
    let result = only_call_site(
        "#[mvl::requires(x > 10)]\n\
         fn f(x: i64, o: Option<i64>) { if let Some(x) = o { need_pos(x); } }\n\
         #[mvl::requires(v > 0)]\n\
         fn need_pos(v: i64) {}",
    );
    assert_eq!(result, DischargeResult::Runtime);
}

#[test]
fn shadowing_is_scoped_and_the_hypothesis_returns_afterwards() {
    // The guard against fixing the four above with a blanket reset. `x > 10`
    // must be gone *inside* the loop and present *after* it -- a permanent
    // invalidation would pass every test above while quietly disabling
    // call-site checking for any function that shadows a parameter name.
    let sites = call_sites(
        "#[mvl::requires(x > 10)]\n\
         fn f(x: i64) { for x in -5..0 { need_pos(x); } need_pos(x); }\n\
         #[mvl::requires(v > 0)]\n\
         fn need_pos(v: i64) {}",
    );
    assert_eq!(sites.len(), 2, "got: {sites:?}");
    assert_eq!(
        sites[0].1,
        DischargeResult::Runtime,
        "inside the loop `x` is the loop binding"
    );
    assert!(
        matches!(sites[1].1, DischargeResult::Proven { .. }),
        "after the loop `x` is the parameter again, so `x > 10` proves `x > 0`; got {:?}",
        sites[1].1
    );
}

#[test]
fn an_arity_mismatch_propagates_nothing() {
    // `FnFacts::params` skips the tuple-pattern parameter, so params.len() (1)
    // != args.len() (2). Binding only `result` left the callee's `n` free to
    // capture the *caller's* `n > 100` -- and #47's gate does not stop it,
    // because this callee's return site genuinely closes: `(n + 1) > n` at L4.
    // `y` is actually -4.
    let result = only_call_site(
        "#[mvl::ensures(result > n)]\n\
         fn produce((a, b): (i64, i64), n: i64) -> i64 { n + 1 }\n\
         #[mvl::requires(v > 0)]\n\
         fn need_pos(v: i64) -> i64 { v }\n\
         #[mvl::requires(n > 100)]\n\
         fn caller(n: i64) -> i64 { let y = produce((1, 2), -5); need_pos(y) }",
    );
    assert_eq!(
        result,
        DischargeResult::Runtime,
        "the callee's parameter name must not capture the caller's variable"
    );
}

#[test]
fn a_loop_body_retires_what_it_assigns_before_the_walk() {
    // The walk is a single in-order pass, so a mutation *after* the call never
    // retired the hypothesis the call used. False on every iteration but the
    // first.
    let result = only_call_site(
        "#[mvl::requires(x > 10)]\n\
         fn f(mut x: i64) { loop { need_pos(x); x = -1; } }\n\
         #[mvl::requires(v > 0)]\n\
         fn need_pos(v: i64) {}",
    );
    assert_eq!(result, DischargeResult::Runtime);
}

#[test]
fn a_while_body_retires_what_it_assigns_too() {
    let result = only_call_site(
        "#[mvl::requires(x > 10)]\n\
         fn f(mut x: i64) { while x != 0 { need_pos(x); x = -1; } }\n\
         #[mvl::requires(v > 0)]\n\
         fn need_pos(v: i64) {}",
    );
    assert_eq!(result, DischargeResult::Runtime);
}

#[test]
fn a_loop_that_assigns_nothing_keeps_its_hypotheses() {
    // The retirement is per-name, not a blanket reset on entering any loop.
    let result = only_call_site(
        "#[mvl::requires(x > 10)]\n\
         fn f(x: i64, mut other: i64) { loop { need_pos(x); other = -1; } }\n\
         #[mvl::requires(v > 0)]\n\
         fn need_pos(v: i64) {}",
    );
    assert!(
        matches!(result, DischargeResult::Proven { .. }),
        "assigning `other` must not retire `x`'s hypothesis, got {result:?}"
    );
}

#[test]
fn an_unmodelled_tail_construct_is_substituted_whole_and_stays_unclosed() {
    // #48. The docs used to claim an unmodelled construct yields *no*
    // obligation. It yields one over the whole expression instead, and that
    // difference is load-bearing since #47: a function with zero return-site
    // obligations is treated as closed (`all()` over an empty set), so skipping
    // would mark this closed and propagate `result > 0` from a body that
    // returns -5.
    let sites = return_sites(
        "#[mvl::ensures(result > 0)]\n\
         fn f() -> i64 { loop { break -5; } }",
    );
    assert_eq!(
        sites.len(),
        1,
        "the loop is substituted whole, got {sites:?}"
    );
    assert_eq!(
        sites[0].1,
        DischargeResult::Runtime,
        "the solver cannot decide it, which is what keeps the fn unclosed"
    );
}

#[test]
fn an_unmodelled_tail_construct_blocks_propagation() {
    // The consequence of the above, end to end, *for a genuinely unenforced
    // function*: because `f`'s return site never closes and it opts out of
    // enforcement, its postcondition must not reach the caller's Γ either
    // way. Since #69 this needs `#[mvl::unchecked]` to stay blocked -- an
    // enforced `f` would propagate regardless, the same as
    // `an_enforced_but_undischarged_postcondition_now_enters_gamma`.
    let result = only_call_site(
        "#[mvl::unchecked]\n\
         #[mvl::ensures(result > 0)]\n\
         fn f() -> i64 { loop { break -5; } }\n\
         #[mvl::requires(v > 0)]\n\
         fn need_pos(v: i64) {}\n\
         fn caller() { let y = f(); need_pos(y); }",
    );
    assert_eq!(result, DischargeResult::Runtime);
}

#[test]
fn a_diverging_body_propagates_because_the_continuation_is_unreachable() {
    // Pins the one safe reading of `return_site_closure`'s empty `all()` (#48).
    // `diverges` has no return point, so zero return-site obligations, so it
    // counts as closed and its postcondition propagates. Sound *only* because
    // the function never returns — the caller's `need_pos(y)` is unreachable,
    // and proving things about unreachable code is vacuous.
    //
    // This test exists so that if the empty case ever stops being safe, it
    // fails here rather than silently licensing a false proof elsewhere.
    let result = only_call_site(
        "#[mvl::ensures(result > 0)]\n\
         fn diverges() -> i64 { panic!(\"never returns\") }\n\
         #[mvl::requires(v > 0)]\n\
         fn need_pos(v: i64) {}\n\
         fn caller() { let y = diverges(); need_pos(y); }",
    );
    assert!(
        matches!(result, DischargeResult::Proven { .. }),
        "documented as vacuously sound; got {result:?}"
    );
}
