use mvl_rust_core::attrs::MvlAttr;
use syn::Attribute;

fn parse_fn_attrs(src: &str) -> Vec<Attribute> {
    let file: syn::File = syn::parse_str(src).expect("valid rust source");
    match &file.items[0] {
        syn::Item::Fn(f) => f.attrs.clone(),
        other => panic!("expected a fn item, got {other:?}"),
    }
}

#[test]
fn parses_refine_attr() {
    let attrs = parse_fn_attrs("#[refine(x >= 0 && x < 100)] fn f(x: i32) -> i32 { x }");
    match MvlAttr::try_from_attribute(&attrs[0]) {
        Some(Ok(MvlAttr::Refine(r))) => {
            let expected: syn::Expr = syn::parse_quote!(x >= 0 && x < 100);
            assert_eq!(r.predicate, expected);
        }
        other => panic!("expected Ok(Refine(_)), got {other:?}"),
    }
}

#[test]
fn parses_total_attr_with_no_arguments() {
    let attrs = parse_fn_attrs("#[total] fn f() {}");
    assert!(matches!(
        MvlAttr::try_from_attribute(&attrs[0]),
        Some(Ok(MvlAttr::Total(_)))
    ));
}

#[test]
fn parses_partial_attr_with_no_arguments() {
    let attrs = parse_fn_attrs("#[partial] fn f() {}");
    assert!(matches!(
        MvlAttr::try_from_attribute(&attrs[0]),
        Some(Ok(MvlAttr::Partial(_)))
    ));
}

#[test]
fn parses_decreases_attr() {
    let attrs = parse_fn_attrs("#[decreases(len - i)] fn f() {}");
    match MvlAttr::try_from_attribute(&attrs[0]) {
        Some(Ok(MvlAttr::Decreases(d))) => {
            let expected: syn::Expr = syn::parse_quote!(len - i);
            assert_eq!(d.measure, expected);
        }
        other => panic!("expected Ok(Decreases(_)), got {other:?}"),
    }
}

#[test]
fn parses_effect_attr_with_multiple_effects() {
    let attrs = parse_fn_attrs("#[effect(Console, Time)] fn f() {}");
    match MvlAttr::try_from_attribute(&attrs[0]) {
        Some(Ok(MvlAttr::Effect(e))) => {
            let names: Vec<String> = e.effects.iter().map(|i| i.to_string()).collect();
            assert_eq!(names, vec!["Console", "Time"]);
        }
        other => panic!("expected Ok(Effect(_)), got {other:?}"),
    }
}

#[test]
fn parses_empty_effect_attr() {
    let attrs = parse_fn_attrs("#[effect()] fn f() {}");
    match MvlAttr::try_from_attribute(&attrs[0]) {
        Some(Ok(MvlAttr::Effect(e))) => assert!(e.effects.is_empty()),
        other => panic!("expected Ok(Effect(_)), got {other:?}"),
    }
}

#[test]
fn parses_requires_attr() {
    let attrs = parse_fn_attrs("#[requires(x > 0)] fn f(x: i32) {}");
    match MvlAttr::try_from_attribute(&attrs[0]) {
        Some(Ok(MvlAttr::Requires(r))) => {
            let expected: syn::Expr = syn::parse_quote!(x > 0);
            assert_eq!(r.predicate, expected);
        }
        other => panic!("expected Ok(Requires(_)), got {other:?}"),
    }
}

#[test]
fn parses_ensures_attr() {
    let attrs = parse_fn_attrs("#[ensures(result > 0)] fn f() -> i32 { 1 }");
    match MvlAttr::try_from_attribute(&attrs[0]) {
        Some(Ok(MvlAttr::Ensures(e))) => {
            let expected: syn::Expr = syn::parse_quote!(result > 0);
            assert_eq!(e.predicate, expected);
        }
        other => panic!("expected Ok(Ensures(_)), got {other:?}"),
    }
}

#[test]
fn parses_label_attr_with_no_arguments() {
    let file: syn::File = syn::parse_str("#[label] struct Phi;").unwrap();
    let attrs = match &file.items[0] {
        syn::Item::Struct(s) => &s.attrs,
        other => panic!("expected a struct item, got {other:?}"),
    };
    assert!(matches!(
        MvlAttr::try_from_attribute(&attrs[0]),
        Some(Ok(MvlAttr::Label(_)))
    ));
}

#[test]
fn parses_relabel_attr_with_from_to_and_audit() {
    let attrs = parse_fn_attrs(
        r#"#[relabel(from = "Tainted", to = "_", audit)] fn f(x: i32) -> i32 { x }"#,
    );
    match MvlAttr::try_from_attribute(&attrs[0]) {
        Some(Ok(MvlAttr::Relabel(r))) => {
            assert_eq!(r.from.value(), "Tainted");
            assert_eq!(r.to.value(), "_");
            assert!(r.audit);
        }
        other => panic!("expected Ok(Relabel(_)), got {other:?}"),
    }
}

#[test]
fn parses_relabel_attr_without_audit() {
    let attrs = parse_fn_attrs(r#"#[relabel(from = "_", to = "Phi")] fn f(x: i32) -> i32 { x }"#);
    match MvlAttr::try_from_attribute(&attrs[0]) {
        Some(Ok(MvlAttr::Relabel(r))) => assert!(!r.audit),
        other => panic!("expected Ok(Relabel(_)), got {other:?}"),
    }
}

#[test]
fn unrecognized_attribute_returns_none() {
    let attrs = parse_fn_attrs("#[allow(dead_code)] fn f() {}");
    assert!(MvlAttr::try_from_attribute(&attrs[0]).is_none());
}

#[test]
fn malformed_refine_predicate_returns_parse_error() {
    let attrs = parse_fn_attrs("#[refine(x >=)] fn f() {}");
    match MvlAttr::try_from_attribute(&attrs[0]) {
        Some(Err(_)) => {}
        other => panic!("expected Some(Err(_)), got {other:?}"),
    }
}

// Regression: real usage is always the fully-qualified `#[mvl::total]` form
// (see the `mvl` crate) — `syn::Path::get_ident()` returns `None` for any
// multi-segment path, so a naive `get_ident()`-based match would silently
// fail to recognize every real annotation. Matching on the last segment
// instead handles both forms identically.
#[test]
fn recognizes_fully_qualified_mvl_paths() {
    let cases = [
        ("#[mvl::total] fn f() {}", "total"),
        ("#[mvl::partial] fn f() {}", "partial"),
        ("#[mvl::refine(x > 0)] fn f(x: i32) {}", "refine"),
        ("#[mvl::decreases(n)] fn f() {}", "decreases"),
        ("#[mvl::effect(Console)] fn f() {}", "effect"),
        ("#[mvl::requires(x > 0)] fn f(x: i32) {}", "requires"),
        ("#[mvl::ensures(result > 0)] fn f() -> i32 { 1 }", "ensures"),
    ];
    for (src, name) in cases {
        let attrs = parse_fn_attrs(src);
        assert!(
            MvlAttr::try_from_attribute(&attrs[0]).is_some(),
            "expected #[mvl::{name}] to be recognized"
        );
    }
}
