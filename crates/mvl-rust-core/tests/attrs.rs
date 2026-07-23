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
fn parses_refine_ret_attr() {
    let attrs = parse_fn_attrs("#[refine_ret(y => y >= 0)] fn f() -> i32 { 0 }");
    match MvlAttr::try_from_attribute(&attrs[0]) {
        Some(Ok(MvlAttr::RefineRet(r))) => {
            assert_eq!(r.binder, "y");
            let expected: syn::Expr = syn::parse_quote!(y >= 0);
            assert_eq!(r.predicate, expected);
        }
        other => panic!("expected Ok(RefineRet(_)), got {other:?}"),
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
fn parses_label_attr() {
    let attrs = parse_fn_attrs("#[label(Secret)] fn f() {}");
    match MvlAttr::try_from_attribute(&attrs[0]) {
        Some(Ok(MvlAttr::Label(l))) => assert_eq!(l.label, "Secret"),
        other => panic!("expected Ok(Label(_)), got {other:?}"),
    }
}

#[test]
fn parses_declassify_attr_with_no_arguments() {
    let attrs = parse_fn_attrs("#[declassify] fn f() {}");
    assert!(matches!(
        MvlAttr::try_from_attribute(&attrs[0]),
        Some(Ok(MvlAttr::Declassify(_)))
    ));
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
