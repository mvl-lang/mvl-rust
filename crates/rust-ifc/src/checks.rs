//! IFC label enforcement (spec Requirement 5, decided v1 scope per issue
//! #10): verifies every `Labeled::into_inner()` (declassify) / `::new()`
//! (classify) call that touches a recognized labeled type sits inside a
//! function whose own `#[mvl::relabel(from = ..., to = ...)]` attribute
//! declares exactly that transition.
//!
//! No call graph, no local-variable dataflow (see issue #10's design
//! history for why neither is needed) -- both directions are recognized
//! from purely local, syntactically-explicit facts:
//!
//! - **Declassify** (`.into_inner()`): the receiver must be a bare
//!   identifier that is one of the *enclosing function's own parameters*,
//!   whose declared type is `Tainted<T>`/`Secret<T>` (the built-in
//!   aliases) or the direct two-argument `Labeled<L, T>` form (`L`'s own
//!   name becomes the label). A value that only becomes labeled via an
//!   intermediate `let` binding, a generic helper, or a field access is a
//!   known, deliberate v1 gap -- not attempted.
//! - **Classify** (`::new()`): the call's own path directly names the
//!   label -- `Tainted::new(..)`/`Secret::new(..)`, or
//!   `Labeled::<L, _>::new(..)` with an explicit turbofish. A bare
//!   `Labeled::new(..)` with no turbofish doesn't reveal `L` syntactically
//!   and is a known, deliberate v1 gap.
//!
//! Only recognizing these specific, closed type/path names (rather than
//! e.g. "any single-generic-argument type") is deliberate: it keeps false
//! positives at zero against unrelated stdlib types that also have an
//! `.into_inner()` method (`RefCell`, `Mutex`, `BufWriter`, ...), since
//! none of those are named `Tainted`/`Secret`/`Labeled`.
//!
//! Naming note: `relabel`'s `from`/`to` strings must match the label name
//! *exactly as it's spelled at the recognition site*. For the built-in
//! aliases that's the alias itself (`"Tainted"`, matching `Tainted<T>`,
//! not the underlying `TaintedLabel` marker struct). For the direct
//! `Labeled<L, T>` form, it's `L`'s own name verbatim (e.g. a marker
//! struct named plain `Reviewed`, not `ReviewedLabel`, if that's what a
//! `relabel(.., to = "Reviewed")` names).

use std::collections::HashMap;

use mvl_rust_core::attrs::{MvlAttr, RelabelAttr};
use mvl_rust_core::diagnostics::{Diagnostic, Level};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{
    Attribute, Expr, ExprCall, ExprMethodCall, FnArg, GenericArgument, Item, ItemFn, Pat,
    PathArguments, Signature, Type,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CheckError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse source as Rust: {0}")]
    Parse(#[source] syn::Error),
}

fn relabel_of(attrs: &[Attribute]) -> Option<RelabelAttr> {
    for attr in attrs {
        if let Some(Ok(MvlAttr::Relabel(relabel))) = MvlAttr::try_from_attribute(attr) {
            return Some(relabel);
        }
    }
    None
}

fn simple_type_name(ty: &Type) -> Option<String> {
    if let Type::Path(type_path) = ty {
        if type_path.qself.is_none() {
            return type_path.path.segments.last().map(|s| s.ident.to_string());
        }
    }
    None
}

/// The label name a *type* carries, if it's one of the recognized forms.
fn recognized_label_type(ty: &Type) -> Option<String> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    if type_path.qself.is_some() {
        return None;
    }
    let segment = type_path.path.segments.last()?;
    match segment.ident.to_string().as_str() {
        "Tainted" | "Secret" => Some(segment.ident.to_string()),
        "Labeled" => {
            let PathArguments::AngleBracketed(args) = &segment.arguments else {
                return None;
            };
            let type_args: Vec<&Type> = args
                .args
                .iter()
                .filter_map(|a| match a {
                    GenericArgument::Type(t) => Some(t),
                    _ => None,
                })
                .collect();
            if type_args.len() == 2 {
                simple_type_name(type_args[0])
            } else {
                None
            }
        }
        _ => None,
    }
}

/// The label name a `Path::new(..)` *call* constructs, if it's one of the
/// recognized forms. Matches on the last two path segments, so both bare
/// (`Tainted::new`) and module-qualified (`mvl::Tainted::new`) forms work
/// -- the same convention `MvlAttr::try_from_attribute` uses for attribute
/// paths.
fn recognized_label_construction(path: &syn::Path) -> Option<String> {
    let segments: Vec<_> = path.segments.iter().collect();
    if segments.len() < 2 {
        return None;
    }
    let method_segment = segments[segments.len() - 1];
    if method_segment.ident != "new" {
        return None;
    }
    let type_segment = segments[segments.len() - 2];
    match type_segment.ident.to_string().as_str() {
        "Tainted" | "Secret" => Some(type_segment.ident.to_string()),
        "Labeled" => {
            let PathArguments::AngleBracketed(args) = &type_segment.arguments else {
                return None;
            };
            let type_args: Vec<&Type> = args
                .args
                .iter()
                .filter_map(|a| match a {
                    GenericArgument::Type(t) => Some(t),
                    _ => None,
                })
                .collect();
            if type_args.len() == 2 {
                simple_type_name(type_args[0])
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Maps each of a function's own parameter names to the label its declared
/// type carries (only for parameters whose type is one of the recognized
/// forms).
fn labeled_params(sig: &Signature) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for input in &sig.inputs {
        if let FnArg::Typed(pat_type) = input {
            if let Pat::Ident(pat_ident) = &*pat_type.pat {
                if let Some(label) = recognized_label_type(&pat_type.ty) {
                    map.insert(pat_ident.ident.to_string(), label);
                }
            }
        }
    }
    map
}

struct IfcVisitor<'a, 'd> {
    fn_name: &'a str,
    relabel: Option<&'a RelabelAttr>,
    labeled_params: &'a HashMap<String, String>,
    diagnostics: &'d mut Vec<Diagnostic>,
}

impl<'ast> Visit<'ast> for IfcVisitor<'_, '_> {
    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if node.method == "into_inner" {
            if let Expr::Path(path_expr) = &*node.receiver {
                if let Some(ident) = path_expr.path.get_ident() {
                    if let Some(label) = self.labeled_params.get(&ident.to_string()) {
                        let declared = self.relabel.is_some_and(|r| r.from.value() == *label);
                        if !declared {
                            self.diagnostics.push(
                                Diagnostic::new(
                                    Level::Error,
                                    format!(
                                        "`{}` strips label `{label}` via `.into_inner()` without a matching `#[mvl::relabel(from = \"{label}\", ..)]`",
                                        self.fn_name
                                    ),
                                    node.method.span(),
                                )
                                .with_label("illegal declassification"),
                            );
                        }
                    }
                }
            }
        }
        visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Expr::Path(path_expr) = &*node.func {
            if let Some(label) = recognized_label_construction(&path_expr.path) {
                let declared = self.relabel.is_some_and(|r| r.to.value() == label);
                if !declared {
                    self.diagnostics.push(
                        Diagnostic::new(
                            Level::Error,
                            format!(
                                "`{}` constructs label `{label}` without a matching `#[mvl::relabel(.., to = \"{label}\")]`",
                                self.fn_name
                            ),
                            node.span(),
                        )
                        .with_label("illegal classification"),
                    );
                }
            }
        }
        visit::visit_expr_call(self, node);
    }
}

/// Runs the check against already-loaded source text.
pub fn check_source(source: &str) -> Result<Vec<Diagnostic>, CheckError> {
    let file: syn::File = syn::parse_str(source).map_err(CheckError::Parse)?;

    let mut diagnostics = Vec::new();
    for item in &file.items {
        if let Item::Fn(ItemFn {
            attrs, sig, block, ..
        }) = item
        {
            let relabel = relabel_of(attrs);
            let params = labeled_params(sig);
            let fn_name = sig.ident.to_string();
            let mut visitor = IfcVisitor {
                fn_name: &fn_name,
                relabel: relabel.as_ref(),
                labeled_params: &params,
                diagnostics: &mut diagnostics,
            };
            visitor.visit_block(block);
        }
    }
    Ok(diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_relabel_declassify_is_fine() {
        let diagnostics = check_source(
            r#"#[mvl::relabel(from = "Tainted", to = "_", audit)]
               fn trust<T>(value: Tainted<T>, tag: &'static str) -> T { value.into_inner() }"#,
        )
        .unwrap();
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn declassify_without_relabel_is_an_error() {
        let diagnostics =
            check_source("fn leak<T>(value: Tainted<T>) -> T { value.into_inner() }").unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Tainted"));
    }

    #[test]
    fn declassify_with_mismatched_relabel_is_an_error() {
        let diagnostics = check_source(
            r#"#[mvl::relabel(from = "Secret", to = "_", audit)]
               fn leak<T>(value: Tainted<T>) -> T { value.into_inner() }"#,
        )
        .unwrap();
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn matching_relabel_classify_is_fine() {
        let diagnostics = check_source(
            r#"#[mvl::relabel(from = "_", to = "Tainted", audit)]
               fn taint(value: String) -> Tainted<String> { Tainted::new(value) }"#,
        )
        .unwrap();
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn classify_without_relabel_is_an_error() {
        let diagnostics =
            check_source("fn taint(value: String) -> Tainted<String> { Tainted::new(value) }")
                .unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Tainted"));
    }

    #[test]
    fn direct_labeled_form_is_recognized() {
        let diagnostics =
            check_source("fn leak<T>(value: Labeled<PhiLabel, T>) -> T { value.into_inner() }")
                .unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("PhiLabel"));
    }

    #[test]
    fn multi_hop_chain_is_fine_as_two_independent_hops() {
        let diagnostics = check_source(
            r#"#[mvl::relabel(from = "Tainted", to = "Reviewed", audit)]
               fn step1(value: Tainted<String>) -> String { value.into_inner() }
               #[mvl::relabel(from = "_", to = "Reviewed", audit)]
               fn wrap_reviewed(value: String) -> Labeled<Reviewed, String> { Labeled::<Reviewed, _>::new(value) }"#,
        )
        .unwrap();
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn unrelated_into_inner_on_refcell_is_not_flagged() {
        let diagnostics =
            check_source("fn f(cell: std::cell::RefCell<i32>) -> i32 { cell.into_inner() }")
                .unwrap();
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn bare_labeled_new_without_turbofish_is_a_known_gap_not_flagged() {
        let diagnostics = check_source(
            "fn taint(value: String) -> Labeled<TaintedLabel, String> { Labeled::new(value) }",
        )
        .unwrap();
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn malformed_source_returns_parse_error() {
        assert!(check_source("fn f( {{{").is_err());
    }
}
