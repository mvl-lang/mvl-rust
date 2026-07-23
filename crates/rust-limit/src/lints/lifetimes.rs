//! Forbids explicit named lifetimes beyond function-scoped elision.
//!
//! Elided lifetimes produce no syntax to check, so this flags any
//! *written-out* lifetime — declarations (`fn f<'a>(...)`) and uses
//! (`&'a T`) alike — except `'static` (too common and necessary to forbid)
//! and `'_` (the explicit "infer this" placeholder, not an arbitrary name).

use mvl_rust_core::diagnostics::{Diagnostic, Level};
use syn::visit::{self, Visit};
use syn::{File, Lifetime};

struct LifetimeVisitor<'d> {
    diagnostics: &'d mut Vec<Diagnostic>,
}

impl<'ast> Visit<'ast> for LifetimeVisitor<'_> {
    fn visit_lifetime(&mut self, node: &'ast Lifetime) {
        if node.ident != "static" && node.ident != "_" {
            self.diagnostics.push(
                Diagnostic::new(
                    Level::Error,
                    format!(
                        "explicit lifetime `'{}` is outside the qualified subset (beyond function-scoped elision)",
                        node.ident
                    ),
                    node.span(),
                )
                .with_label("explicit lifetime"),
            );
        }
        visit::visit_lifetime(self, node);
    }
}

pub fn check(file: &File, diagnostics: &mut Vec<Diagnostic>) {
    let mut visitor = LifetimeVisitor { diagnostics };
    visitor.visit_file(file);
}
