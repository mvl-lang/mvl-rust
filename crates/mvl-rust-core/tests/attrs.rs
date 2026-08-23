use mvl_rust_core::attrs::{MvlAttr, Predicate};
use syn::Attribute;

fn parse_fn_attrs(src: &str) -> Vec<Attribute> {
    let file: syn::File = syn::parse_str(src).expect("valid rust source");
    match &file.items[0] {
        syn::Item::Fn(f) => f.attrs.clone(),
        other => panic!("expected a fn item, got {other:?}"),
    }
}

#[test]
fn refine_is_no_longer_recognised() {
    // Removed in #54. Parsed and read by no tool, so an author who wrote it
    // got silence rather than verification -- and `refine` was the
    // attribute spec 001 advertised as the headline example. `requires`/
    // `ensures` cover everything `refine` was for.
    //
    // It now falls through to `None`, the same as any third-party attribute:
    // unrecognised rather than recognised-and-ignored. Re-adding it with an
    // implementation is cheap; an inert attribute is worse than an absent one.
    for src in [
        "#[refine(x >= 0)] fn f(x: i32) -> i32 { x }",
        "#[mvl::refine(x >= 0)] fn f(x: i32) -> i32 { x }",
    ] {
        let attrs = parse_fn_attrs(src);
        assert!(
            MvlAttr::try_from_attribute(&attrs[0]).is_none(),
            "`{src}` must not be recognised"
        );
    }
}

#[test]
fn parses_partial_attr_with_no_arguments() {
    // `partial` was removed alongside `refine` in #54 (dead weight, read by
    // no tool) and re-added by ADR-0012 (#117) with a real, load-bearing
    // meaning: the explicit opposite of `#[mvl::total]`, not the inert
    // attribute #54 removed.
    for src in ["#[partial] fn f() {}", "#[mvl::partial] fn f() {}"] {
        let attrs = parse_fn_attrs(src);
        assert!(matches!(
            MvlAttr::try_from_attribute(&attrs[0]),
            Some(Ok(MvlAttr::Partial(_)))
        ));
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
fn parses_unchecked_attr_with_no_arguments() {
    // #69: rust-refine needs to see `#[mvl::unchecked]` to know whether a
    // function's contract is actually enforced, not just declared.
    let attrs = parse_fn_attrs("#[unchecked] fn f() {}");
    assert!(matches!(
        MvlAttr::try_from_attribute(&attrs[0]),
        Some(Ok(MvlAttr::Unchecked(_)))
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
            match r.predicate {
                Predicate::Expr(e) => assert_eq!(e, expected),
                other => panic!("expected Predicate::Expr, got {other:?}"),
            }
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
            match e.predicate {
                Predicate::Expr(expr) => assert_eq!(expr, expected),
                other => panic!("expected Predicate::Expr, got {other:?}"),
            }
        }
        other => panic!("expected Ok(Ensures(_)), got {other:?}"),
    }
}

#[test]
fn parses_requires_attr_with_a_bounded_quantifier() {
    let attrs = parse_fn_attrs(
        "#[requires(forall i in [1..50] . sections.get(i) != None)] fn f(sections: i32) {}",
    );
    match MvlAttr::try_from_attribute(&attrs[0]) {
        Some(Ok(MvlAttr::Requires(r))) => {
            assert!(matches!(r.predicate, Predicate::Forall { .. }));
        }
        other => panic!("expected Ok(Requires(_)), got {other:?}"),
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
fn malformed_predicate_returns_parse_error() {
    // The distinction that matters: an attribute this workspace *owns* with a
    // malformed argument is `Some(Err(_))` -- a reported parse error -- while an
    // attribute it does not own is `None` and skipped silently. Getting those
    // two confused would either reject third-party attributes or swallow a
    // typo in one of ours.
    //
    // Vehicle changed from `refine` to `requires` in #54; `refine` was removed
    // and now correctly returns `None` like any unowned attribute.
    let attrs = parse_fn_attrs("#[requires(x >=)] fn f() {}");
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
