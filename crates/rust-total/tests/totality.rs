use rust_total::checks::check_source;

#[test]
fn compliant_total_function_has_no_diagnostics() {
    let source = r#"
        #[mvl::total]
        fn abs(x: i32) -> i32 {
            if x < 0 { -x } else { x }
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics, got {diagnostics:?}"
    );
}

#[test]
fn non_total_functions_are_not_scanned_at_all() {
    // Plenty of panic-risk constructs here, but with no #[mvl::total] this
    // function is entirely out of scope for rust-total.
    let source = r#"
        fn f(v: Vec<i32>, i: usize) -> i32 {
            v[i].checked_add(1).unwrap()
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics for a non-#[total] function, got {diagnostics:?}"
    );
}

// ── termination-trigger (`decreases`) scenarios ─────────────────────────

#[test]
fn wildcard_arm_panic_is_rejected() {
    // rustc itself already rejects a *genuinely* non-exhaustive match (no
    // wildcard, missing variants) as a hard compile error before our tool
    // ever runs -- nothing for rust-total to add there. The gap rustc
    // doesn't cover is a wildcard arm that copies out with a panic, which
    // is syntactically exhaustive but not actually total. That's just a
    // `panic!` sitting inside a match arm, which panic-freedom already
    // catches with no special-casing.
    let source = r#"
        #[mvl::total]
        fn describe(x: Option<i32>) -> i32 {
            match x {
                Some(n) => n,
                None => panic!("unexpected"),
            }
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("panic!"));
}

#[test]
fn terminating_recursion_with_decreases_is_accepted() {
    let source = r#"
        #[mvl::total]
        #[mvl::decreases(n)]
        fn factorial(n: u64) -> u64 {
            if n == 0 { 1 } else { n * factorial(n - 1) }
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics, got {diagnostics:?}"
    );
}

#[test]
fn missing_decreases_on_recursive_total_function_is_rejected() {
    let source = r#"
        #[mvl::total]
        fn factorial(n: u64) -> u64 {
            if n == 0 { 1 } else { n * factorial(n - 1) }
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("factorial"));
    assert!(diagnostics[0].message.contains("decreases"));
}

#[test]
fn non_decreasing_measure_is_rejected() {
    // ADR-0009: presence is no longer enough. `n` is passed unchanged, so
    // the measure never decreases, and the tool must now say so instead of
    // accepting it on presence alone.
    let source = r#"
        #[mvl::total]
        #[mvl::decreases(n)]
        fn loops_forever(n: u64) -> u64 {
            if n == 0 { 0 } else { loops_forever(n) }
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("loops_forever"));
    assert!(diagnostics[0]
        .message
        .contains("does not provably decrease"));
}

#[test]
fn division_is_never_provably_decreasing() {
    // Division/modulo is outside the native solver's linear-arithmetic
    // fragment entirely (ADR-0009): `discharge_entailment` returns
    // `Runtime` for `(n / 2) < n` regardless of hypotheses, confirmed
    // empirically against a `n > 0` hypothesis too. With no runtime
    // enforcement fallback for `decreases`, that's a rejection -- alongside
    // panic_freedom's pre-existing, unrelated division-by-zero diagnostic
    // (spec 003 Requirement 1).
    let source = r#"
        #[mvl::total]
        #[mvl::decreases(n)]
        fn halve(n: u64) -> u64 {
            if n == 0 { 0 } else { halve(n / 2) }
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 2, "got {diagnostics:?}");
    assert!(diagnostics
        .iter()
        .any(|d| d.message.contains("division/modulo")));
    assert!(diagnostics
        .iter()
        .any(|d| d.message.contains("does not provably decrease")));
}

#[test]
fn a_symbolic_decrement_is_proved_given_a_requires_hypothesis() {
    // The solver-backed check generalizes beyond literal constants: given
    // `#[mvl::requires(k > 0)]`, `fuel - k` is proved to decrease `fuel` at
    // L4 (Fourier-Motzkin), even though `k` is not itself a literal.
    let source = r#"
        #[mvl::total]
        #[mvl::decreases(fuel)]
        #[mvl::requires(k > 0)]
        fn countdown(fuel: u64, k: u64) -> u64 {
            if fuel == 0 { 0 } else { countdown(fuel - k, k) }
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics, got {diagnostics:?}"
    );
}

#[test]
fn a_symbolic_decrement_without_a_positivity_hypothesis_is_rejected() {
    // Same shape as the previous test, minus the `requires(k > 0)` -- with
    // no hypothesis bounding `k`, the solver cannot rule out `k <= 0`
    // (which would not decrease `fuel`), so the call is rejected.
    let source = r#"
        #[mvl::total]
        #[mvl::decreases(fuel)]
        fn countdown(fuel: u64, k: u64) -> u64 {
            if fuel == 0 { 0 } else { countdown(fuel - k, k) }
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0]
        .message
        .contains("does not provably decrease"));
}

#[test]
fn non_parameter_measure_is_rejected() {
    // ADR-0009 §1: the measure must be a bare parameter identifier. A
    // computed expression isn't analyzable by a syn-only, no-type-info
    // check, so it's rejected rather than silently accepted.
    let source = r#"
        #[mvl::total]
        #[mvl::decreases(n - 1)]
        fn factorial(n: u64) -> u64 {
            if n == 0 { 1 } else { n * factorial(n - 1) }
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("factorial"));
    assert!(diagnostics[0].message.contains("bare parameter"));
}

#[test]
fn non_recursive_total_function_needs_no_decreases() {
    let source = r#"
        #[mvl::total]
        fn double(n: u64) -> u64 {
            n + n
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics, got {diagnostics:?}"
    );
}

// ── panic-freedom, one construct at a time ──────────────────────────────

#[test]
fn unwrap_is_rejected() {
    let source = r#"
        #[mvl::total]
        fn f(x: Option<i32>) -> i32 {
            x.unwrap()
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("unwrap"));
}

#[test]
fn expect_is_rejected() {
    let source = r#"
        #[mvl::total]
        fn f(x: Option<i32>) -> i32 {
            x.expect("must be present")
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("expect"));
}

#[test]
fn panic_macro_as_a_statement_is_rejected() {
    let source = r#"
        #[mvl::total]
        fn f() -> i32 {
            panic!("nope");
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("panic!"));
}

#[test]
fn todo_and_unimplemented_are_rejected() {
    let source = r#"
        #[mvl::total]
        fn f() -> i32 {
            todo!()
        }
        #[mvl::total]
        fn g() -> i32 {
            unimplemented!()
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 2);
    assert!(diagnostics[0].message.contains("todo!"));
    assert!(diagnostics[1].message.contains("unimplemented!"));
}

#[test]
fn raw_indexing_is_rejected() {
    let source = r#"
        #[mvl::total]
        fn f(v: Vec<i32>, i: usize) -> i32 {
            v[i]
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("indexing"));
}

#[test]
fn division_and_modulo_are_rejected() {
    let source = r#"
        #[mvl::total]
        fn f(a: i32, b: i32) -> i32 {
            (a / b) + (a % b)
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 2);
    assert!(diagnostics
        .iter()
        .all(|d| d.message.contains("division/modulo")));
}

#[test]
fn malformed_source_returns_parse_error() {
    let result = check_source("fn f( {{{");
    assert!(result.is_err());
}

#[test]
fn binary_arithmetic_in_a_total_function_is_accepted() {
    // Spec 003 Requirement 2: `+`/`-`/`*` are deliberately NOT flagged for
    // overflow. Without type information, flagging them would flag nearly all
    // numeric code, so the rule is omitted on false-positive grounds rather
    // than because the risk is absent. The existing compliant fixture only
    // exercises unary negation, so it does not evidence this.
    let source = r#"
        #[mvl::total]
        fn combine(a: i32, b: i32, c: i32) -> i32 {
            a + b * c - a
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        diagnostics.is_empty(),
        "binary arithmetic must not be flagged, got {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn requires_and_ensures_on_a_total_function_are_not_flagged() {
    // Since #53 these attributes expand to a real `assert!`, which panics on
    // failure -- but this checker scans the author's source, never the
    // macro-expanded body, so there is no `assert!` token here to flag even
    // in principle. The decision (ADR-0003 §2, "total on its promised
    // domain"): a contract assert firing means a caller broke that domain,
    // which is outside what `#[mvl::total]` promises, not a counterexample
    // to it. This pins the silence as deliberate -- a regression here would
    // most likely arrive as someone adding `assert` to `PANICKING_MACROS`
    // without registering that distinction.
    let source = r#"
        #[mvl::total]
        #[mvl::requires(x > 0)]
        #[mvl::ensures(result > 0)]
        fn f(x: i32) -> i32 {
            x
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        diagnostics.is_empty(),
        "a contract assert must not be treated as violating #[mvl::total]'s panic-freedom promise, got {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}
