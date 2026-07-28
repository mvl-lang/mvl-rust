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
use mvl_rust_core::solver::{DischargeResult, Layer};
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
/// runtime check rather than being wrongly claimed either way. Real MVL
/// reaches the same outcome via Z3 returning `unknown`; we reach it by
/// `linterm_from_expr` refusing variable × variable.
#[test]
fn nonlinear_argument_falls_through_to_runtime() {
    let result = only_call_site(
        "#[mvl::requires(n > 0)]\n\
         fn require_positive(n: i32) -> i32 { n }\n\
         #[mvl::requires(x > 1 && y > 1)]\n\
         fn product(x: i32, y: i32) -> i32 { require_positive(x * y) }",
    );
    assert_eq!(result, DischargeResult::Runtime);
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
