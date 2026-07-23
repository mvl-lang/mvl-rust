use mvl_rust_core::walker::{walk_file, Walker};
use syn::{Expr, Field, ItemFn};

#[derive(Default)]
struct Counter {
    fns: Vec<String>,
    fields: Vec<String>,
    expr_count: usize,
}

impl Walker for Counter {
    fn visit_fn(&mut self, item: &ItemFn) {
        self.fns.push(item.sig.ident.to_string());
    }

    fn visit_field(&mut self, field: &Field) {
        if let Some(ident) = &field.ident {
            self.fields.push(ident.to_string());
        }
    }

    fn visit_expr(&mut self, _expr: &Expr) {
        self.expr_count += 1;
    }
}

#[test]
fn visits_every_function_including_nested() {
    let file: syn::File = syn::parse_str(
        r#"
        fn outer() {
            fn inner() {}
        }
        fn sibling() {}
        "#,
    )
    .unwrap();

    let mut counter = Counter::default();
    walk_file(&file, &mut counter);

    assert_eq!(counter.fns, vec!["outer", "inner", "sibling"]);
}

#[test]
fn visits_struct_fields() {
    let file: syn::File = syn::parse_str(
        r#"
        struct Point {
            x: i32,
            y: i32,
        }
        "#,
    )
    .unwrap();

    let mut counter = Counter::default();
    walk_file(&file, &mut counter);

    assert_eq!(counter.fields, vec!["x", "y"]);
}

#[test]
fn visits_expressions_inside_a_function_body() {
    let file: syn::File = syn::parse_str(
        r#"
        fn f() -> i32 {
            let a = 1;
            let b = 2;
            a + b
        }
        "#,
    )
    .unwrap();

    let mut counter = Counter::default();
    walk_file(&file, &mut counter);

    assert!(
        counter.expr_count >= 3,
        "expected at least 3 expressions, got {}",
        counter.expr_count
    );
}

#[test]
fn default_walker_hooks_do_nothing() {
    struct NoOp;
    impl Walker for NoOp {}

    let file: syn::File = syn::parse_str("fn f() { let x = 1; }").unwrap();
    let mut noop = NoOp;
    walk_file(&file, &mut noop);
}
