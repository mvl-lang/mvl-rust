//! Forbids dynamic dispatch (`dyn Trait`) in type position, including inside
//! generic arguments (e.g. `Box<dyn Any>`) — a single `dyn Trait` check
//! covers both, since `dyn Any` is itself a `TypeTraitObject` regardless of
//! what wraps it.

use mvl_rust_core::diagnostics::{Diagnostic, Level};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{File, TypeParamBound, TypeTraitObject};

struct DynTraitVisitor<'d> {
    diagnostics: &'d mut Vec<Diagnostic>,
}

impl<'ast> Visit<'ast> for DynTraitVisitor<'_> {
    fn visit_type_trait_object(&mut self, node: &'ast TypeTraitObject) {
        let is_any = node.bounds.iter().any(|bound| match bound {
            TypeParamBound::Trait(trait_bound) => trait_bound
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "Any"),
            _ => false,
        });

        let message = if is_any {
            "`dyn Any` is outside the qualified subset (type erasure via dynamic dispatch)"
                .to_string()
        } else {
            "`dyn Trait` is outside the qualified subset (no dynamic dispatch)".to_string()
        };

        self.diagnostics
            .push(Diagnostic::new(Level::Error, message, node.span()).with_label("trait object"));
        visit::visit_type_trait_object(self, node);
    }
}

pub fn check(file: &File, diagnostics: &mut Vec<Diagnostic>) {
    let mut visitor = DynTraitVisitor { diagnostics };
    visitor.visit_file(file);
}
