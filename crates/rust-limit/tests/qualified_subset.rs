use rust_limit::lints::check_source;

#[test]
fn forbidden_construct_rejected() {
    // spec Requirement 1, "Forbidden construct rejected"
    let source = "fn f() { unsafe { std::ptr::null::<i32>(); } }";
    let diagnostics = check_source(source).unwrap();

    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("unsafe"));
    assert!(diagnostics[0]
        .message
        .contains("outside the qualified subset"));
}

#[test]
fn whitelisted_construct_accepted() {
    // spec Requirement 1, "Whitelisted construct accepted": safe references,
    // Result/Option, non-generic (i.e. no explicit) lifetimes, no macros
    // beyond the allowlist.
    let source = r#"
        fn f(x: &i32) -> Option<i32> {
            let y: Result<i32, ()> = Ok(*x);
            println!("{:?}", y);
            match y {
                Ok(v) => Some(v),
                Err(_) => None,
            }
        }
    "#;
    let diagnostics = check_source(source).unwrap();

    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics, got {diagnostics:?}"
    );
}

#[test]
fn unsafe_fn_rejected() {
    let diagnostics = check_source("unsafe fn f() {}").unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("unsafe fn"));
}

#[test]
fn unsafe_impl_rejected() {
    let source = "struct S; unsafe impl Send for S {}";
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("unsafe impl"));
}

#[test]
fn unsafe_trait_rejected() {
    let diagnostics = check_source("unsafe trait Marker {}").unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("unsafe trait"));
}

#[test]
fn dyn_trait_rejected() {
    let source = "fn f(x: &dyn std::fmt::Debug) {}";
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("dyn Trait"));
}

#[test]
fn box_dyn_any_gets_a_friendlier_message() {
    let source = "fn f(x: Box<dyn std::any::Any>) {}";
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("dyn Any"));
    assert!(diagnostics[0].message.contains("type erasure"));
}

#[test]
fn explicit_lifetime_rejected() {
    let source = "fn f<'a>(x: &'a i32) -> &'a i32 { x }";
    let diagnostics = check_source(source).unwrap();
    // one for the declaration `<'a>`, one for each use site (`&'a i32` x2)
    assert_eq!(diagnostics.len(), 3);
    assert!(diagnostics
        .iter()
        .all(|d| d.message.contains("explicit lifetime")));
}

#[test]
fn static_and_placeholder_lifetimes_are_allowed() {
    let source = r#"
        fn f(x: &'static str) -> &'_ str {
            x
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics, got {diagnostics:?}"
    );
}

#[test]
fn macro_outside_allowlist_rejected() {
    let source = "fn f() { my_custom_macro!(); }";
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("my_custom_macro"));
    assert!(diagnostics[0].message.contains("curated allowlist"));
}

#[test]
fn macro_rules_definition_itself_is_not_flagged() {
    let source = r#"
        macro_rules! my_macro {
            () => {
                println!("hi")
            };
        }
        fn f() {
            my_macro!();
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    // the definition is exempt; the invocation of the not-allowlisted
    // `my_macro!` still gets flagged.
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("my_macro"));
}

#[test]
fn allowlisted_macros_are_accepted() {
    let source = r#"fn f() { println!("hi"); let v = vec![1, 2, 3]; assert_eq!(v.len(), 3); }"#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics, got {diagnostics:?}"
    );
}

#[test]
fn transmute_call_rejected() {
    let source = "fn f(x: u32) -> i32 { unsafe { std::mem::transmute(x) } }";
    let diagnostics = check_source(source).unwrap();
    // both the `unsafe` block and the `transmute` call are flagged
    assert_eq!(diagnostics.len(), 2);
    assert!(diagnostics.iter().any(|d| d.message.contains("transmute")));
}

#[test]
fn raw_address_of_rejected() {
    let source = "fn f(x: &i32) -> *const i32 { &raw const *x }";
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("raw address-of"));
}

#[test]
fn malformed_source_returns_parse_error() {
    let result = check_source("fn f( {{{");
    assert!(result.is_err());
}
