//! Forbids macro invocations outside a curated allowlist. Covers
//! invocation-style macros (`foo!(...)` in expression, statement, or item
//! position) via `syn`'s single shared `Macro` node; derive/attribute macros
//! are a separate syntax form and aren't covered here.

use mvl_rust_core::diagnostics::{Diagnostic, Level};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{File, Macro};

/// Starter allowlist covering common formatting, assertion, and stub
/// macros. Easy to extend as real usage surfaces what's actually needed.
const ALLOWED_MACROS: &[&str] = &[
    "println",
    "print",
    "format",
    "write",
    "writeln",
    "vec",
    "assert",
    "assert_eq",
    "assert_ne",
    "matches",
    "panic",
    "todo",
    "unimplemented",
];

struct MacroVisitor<'d> {
    diagnostics: &'d mut Vec<Diagnostic>,
}

impl<'ast> Visit<'ast> for MacroVisitor<'_> {
    fn visit_macro(&mut self, node: &'ast Macro) {
        let name = node
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string());

        // `macro_rules! foo { ... }` parses as a `Macro` node with path
        // `macro_rules` itself — that's a macro *definition*, not an
        // invocation, so it isn't subject to the invocation allowlist.
        let is_definition = name.as_deref() == Some("macro_rules");

        let is_allowed = is_definition
            || name
                .as_deref()
                .is_some_and(|name| ALLOWED_MACROS.contains(&name));

        if !is_allowed {
            let name = name.unwrap_or_else(|| "<unknown>".to_string());
            self.diagnostics.push(
                Diagnostic::new(
                    Level::Error,
                    format!("macro `{name}!` is outside the qualified subset's curated allowlist"),
                    node.span(),
                )
                .with_label("macro invocation"),
            );
        }
        visit::visit_macro(self, node);
    }
}

pub fn check(file: &File, diagnostics: &mut Vec<Diagnostic>) {
    let mut visitor = MacroVisitor { diagnostics };
    visitor.visit_file(file);
}
