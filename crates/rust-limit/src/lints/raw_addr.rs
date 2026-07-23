//! Forbids raw address-of operations (`&raw const place` / `&raw mut place`).

use mvl_rust_core::diagnostics::{Diagnostic, Level};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{ExprRawAddr, File};

struct RawAddrVisitor<'d> {
    diagnostics: &'d mut Vec<Diagnostic>,
}

impl<'ast> Visit<'ast> for RawAddrVisitor<'_> {
    fn visit_expr_raw_addr(&mut self, node: &'ast ExprRawAddr) {
        self.diagnostics.push(
            Diagnostic::new(
                Level::Error,
                "raw address-of (`&raw ...`) is outside the qualified subset",
                node.span(),
            )
            .with_label("raw pointer operation"),
        );
        visit::visit_expr_raw_addr(self, node);
    }
}

pub fn check(file: &File, diagnostics: &mut Vec<Diagnostic>) {
    let mut visitor = RawAddrVisitor { diagnostics };
    visitor.visit_file(file);
}
