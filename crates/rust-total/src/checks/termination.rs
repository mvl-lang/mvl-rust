//! Requires `#[mvl::decreases(measure)]` on any `#[mvl::total]` function
//! that directly calls itself.
//!
//! v1 checks *presence* only — it does not attempt to prove the measure
//! actually decreases on each recursive call (a real interval-analysis
//! proof, per spec Requirement 2, is deferred future work). Only direct
//! self-recursion is detected; mutual recursion between two functions is
//! out of scope for v1.

use mvl_rust_core::diagnostics::{Diagnostic, Level};
use syn::visit::{self, Visit};
use syn::{Expr, ExprCall, Ident, ItemFn};

struct RecursionDetector<'a> {
    fn_name: &'a Ident,
    found: bool,
}

impl<'ast> Visit<'ast> for RecursionDetector<'_> {
    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Expr::Path(path_expr) = &*node.func {
            if path_expr
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == *self.fn_name)
            {
                self.found = true;
            }
        }
        visit::visit_expr_call(self, node);
    }
}

fn is_directly_recursive(item_fn: &ItemFn) -> bool {
    let mut detector = RecursionDetector {
        fn_name: &item_fn.sig.ident,
        found: false,
    };
    detector.visit_block(&item_fn.block);
    detector.found
}

/// `has_decreases` reflects whether the caller already found a
/// `#[mvl::decreases(...)]` attribute on this same function.
pub fn check(item_fn: &ItemFn, has_decreases: bool, diagnostics: &mut Vec<Diagnostic>) {
    if is_directly_recursive(item_fn) && !has_decreases {
        diagnostics.push(
            Diagnostic::new(
                Level::Error,
                format!(
                    "recursive `#[mvl::total]` function `{}` requires `#[mvl::decreases(measure)]`",
                    item_fn.sig.ident
                ),
                item_fn.sig.ident.span(),
            )
            .with_label("recursive call found, no termination measure")
            .with_suggestion(
                "add #[mvl::decreases(measure)] with a measure that strictly decreases on each recursive call",
            ),
        );
    }
}
