//! Forbids `unsafe` in every form: blocks, `unsafe fn` (free, impl, or trait
//! methods), `unsafe impl`, and `unsafe trait`.

use mvl_rust_core::diagnostics::{Diagnostic, Level};
use syn::visit::{self, Visit};
use syn::{ExprUnsafe, File, ItemImpl, ItemTrait, Signature};

struct UnsafeVisitor<'d> {
    diagnostics: &'d mut Vec<Diagnostic>,
}

impl<'ast> Visit<'ast> for UnsafeVisitor<'_> {
    fn visit_expr_unsafe(&mut self, node: &'ast ExprUnsafe) {
        self.diagnostics.push(
            Diagnostic::new(
                Level::Error,
                "`unsafe` block is outside the qualified subset",
                node.unsafe_token.span,
            )
            .with_label("unsafe block"),
        );
        visit::visit_expr_unsafe(self, node);
    }

    fn visit_signature(&mut self, node: &'ast Signature) {
        if let Some(unsafe_token) = &node.unsafety {
            self.diagnostics.push(
                Diagnostic::new(
                    Level::Error,
                    format!("`unsafe fn {}` is outside the qualified subset", node.ident),
                    unsafe_token.span,
                )
                .with_label("unsafe function"),
            );
        }
        visit::visit_signature(self, node);
    }

    fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
        if let Some(unsafe_token) = &node.unsafety {
            self.diagnostics.push(
                Diagnostic::new(
                    Level::Error,
                    "`unsafe impl` is outside the qualified subset",
                    unsafe_token.span,
                )
                .with_label("unsafe impl"),
            );
        }
        visit::visit_item_impl(self, node);
    }

    fn visit_item_trait(&mut self, node: &'ast ItemTrait) {
        if let Some(unsafe_token) = &node.unsafety {
            self.diagnostics.push(
                Diagnostic::new(
                    Level::Error,
                    format!(
                        "`unsafe trait {}` is outside the qualified subset",
                        node.ident
                    ),
                    unsafe_token.span,
                )
                .with_label("unsafe trait"),
            );
        }
        visit::visit_item_trait(self, node);
    }
}

pub fn check(file: &File, diagnostics: &mut Vec<Diagnostic>) {
    let mut visitor = UnsafeVisitor { diagnostics };
    visitor.visit_file(file);
}
