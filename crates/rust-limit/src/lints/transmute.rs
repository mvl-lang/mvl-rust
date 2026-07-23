//! Forbids calls to `std::mem::transmute` (and its `core`/aliased-import
//! spellings). Matched syntactically on the callee path's last segment —
//! this tool parses plain source text with no name resolution, so it can't
//! distinguish a genuine `mem::transmute` call from an unrelated function
//! that merely happens to be named `transmute`. Same limitation as any
//! `syn`-only static check.

use mvl_rust_core::diagnostics::{Diagnostic, Level};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Expr, ExprCall, File};

struct TransmuteVisitor<'d> {
    diagnostics: &'d mut Vec<Diagnostic>,
}

impl<'ast> Visit<'ast> for TransmuteVisitor<'_> {
    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Expr::Path(path_expr) = &*node.func {
            let is_transmute = path_expr
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "transmute");
            if is_transmute {
                self.diagnostics.push(
                    Diagnostic::new(
                        Level::Error,
                        "`transmute` is outside the qualified subset",
                        node.span(),
                    )
                    .with_label("transmute call"),
                );
            }
        }
        visit::visit_expr_call(self, node);
    }
}

pub fn check(file: &File, diagnostics: &mut Vec<Diagnostic>) {
    let mut visitor = TransmuteVisitor { diagnostics };
    visitor.visit_file(file);
}
