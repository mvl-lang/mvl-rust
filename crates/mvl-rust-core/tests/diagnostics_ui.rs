use expect_test::expect;
use mvl_rust_core::diagnostics::{Diagnostic, Level};
use syn::spanned::Spanned;

fn attr_span(source: &str) -> proc_macro2::Span {
    let file: syn::File = syn::parse_str(source).unwrap();
    match &file.items[0] {
        syn::Item::Fn(f) => f.attrs[0].span(),
        other => panic!("expected a fn item, got {other:?}"),
    }
}

#[test]
fn total_violation_includes_span_message_and_suggestion() {
    let source = "#[total] fn f(x: Option<i32>) -> i32 { match x { Some(n) => n } }";
    let diagnostic = Diagnostic::new(
        Level::Error,
        "non-exhaustive match under `#[total]`: variant `None` not handled",
        attr_span(source),
    )
    .with_label("this function is marked #[total]")
    .with_suggestion("add a `None => ...` arm to the match");

    let rendered = diagnostic.render(source, "src/lib.rs");

    assert!(rendered.contains("non-exhaustive match under `#[total]`"));
    assert!(rendered.contains("this function is marked #[total]"));
    assert!(rendered.contains("src/lib.rs"));
    assert!(rendered.contains("#[total]"));
    assert!(rendered.contains("help: add a `None => ...` arm to the match"));
}

#[test]
fn renders_with_rustc_style_source_caret_formatting() {
    let source = "#[total] fn f(x: Option<i32>) -> i32 { match x { Some(n) => n } }";
    let diagnostic = Diagnostic::new(
        Level::Error,
        "non-exhaustive match under `#[total]`: variant `None` not handled",
        attr_span(source),
    )
    .with_label("this function is marked #[total]");

    let rendered = diagnostic.render(source, "src/lib.rs");

    expect![[r#"
        error: non-exhaustive match under `#[total]`: variant `None` not handled
         --> src/lib.rs:1:1
          |
        1 | #[total] fn f(x: Option<i32>) -> i32 { match x { Some(n) => n } }
          | ^^^^^^^^ this function is marked #[total]
          |"#]]
    .assert_eq(&rendered);
}
