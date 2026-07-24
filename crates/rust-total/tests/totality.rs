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

// ── spec Requirement 2 scenarios ────────────────────────────────────────

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
