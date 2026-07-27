//! Finds every `#[mvl::requires(pred)]`/`#[mvl::ensures(pred)]` obligation
//! in a source file and discharges each one through the native `L1`+`L2`
//! backend ([`mvl_rust_core::solver::native::NativeBackend`], ADR-0001).
//!
//! v1 scope: free functions only (`fn f() { ... }`), same limitation
//! `rust-total` documents for the same reason — methods inside `impl`
//! blocks aren't scanned yet. `requires`/`ensures` are recognized
//! independently of `#[mvl::total]`/`#[mvl::partial]` (refinement and
//! totality are orthogonal concerns per spec Requirements 2 and 3).
//!
//! Only plain comparison/boolean predicates are discharged here —
//! quantifiers (`forall i in [1..50]. pred`) aren't valid `syn::Expr`
//! syntax and need the bespoke predicate-language parser discussed in
//! #22, which doesn't exist yet. `#[mvl::requires]`/`#[mvl::ensures]`
//! still only parse a `syn::Expr` (see `mvl_rust_core::attrs`), so a
//! quantified predicate fails to parse as an attribute at all today — a
//! known v1 gap, not something this module works around.

use mvl_rust_core::attrs::MvlAttr;
use mvl_rust_core::diagnostics::{Diagnostic, Level};
use mvl_rust_core::solver::native::discharge_predicate;
use mvl_rust_core::solver::{DischargeResult, Layer};
use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Expr, ItemFn};
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

/// Which clause an obligation came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObligationKind {
    Requires,
    Ensures,
}

impl ObligationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ObligationKind::Requires => "requires",
            ObligationKind::Ensures => "ensures",
        }
    }
}

/// One `requires`/`ensures` obligation found on a function, not yet
/// bound to a file (the caller supplies the origin path when it needs a
/// full `mvl_rust_core::solver::Obligation`, mirroring how
/// [`Diagnostic`] carries a bare `Span` rather than a baked
/// `file:line:col` string).
#[derive(Debug, Clone)]
pub struct FoundObligation {
    pub fn_name: String,
    pub kind: ObligationKind,
    pub predicate: Expr,
    pub span: Span,
}

impl FoundObligation {
    pub fn id(&self) -> String {
        format!("{}::{}", self.fn_name, self.kind.as_str())
    }

    pub fn predicate_text(&self) -> String {
        let predicate = &self.predicate;
        quote::quote!(#predicate).to_string()
    }

    pub fn discharge(&self) -> DischargeResult {
        discharge_predicate(&self.predicate)
    }
}

struct ObligationFinder<'o> {
    found: &'o mut Vec<FoundObligation>,
}

impl<'ast> Visit<'ast> for ObligationFinder<'_> {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let fn_name = node.sig.ident.to_string();
        for attr in &node.attrs {
            match MvlAttr::try_from_attribute(attr) {
                Some(Ok(MvlAttr::Requires(requires))) => {
                    self.found.push(FoundObligation {
                        fn_name: fn_name.clone(),
                        kind: ObligationKind::Requires,
                        predicate: requires.predicate,
                        span: attr.span(),
                    });
                }
                Some(Ok(MvlAttr::Ensures(ensures))) => {
                    self.found.push(FoundObligation {
                        fn_name: fn_name.clone(),
                        kind: ObligationKind::Ensures,
                        predicate: ensures.predicate,
                        span: attr.span(),
                    });
                }
                _ => {}
            }
        }
        visit::visit_item_fn(self, node);
    }
}

/// Finds every `requires`/`ensures` obligation in `source`, without
/// discharging or rendering them yet.
pub fn find_obligations(source: &str) -> Result<Vec<FoundObligation>, CheckError> {
    let file: syn::File = syn::parse_str(source).map_err(CheckError::Parse)?;
    let mut found = Vec::new();
    let mut finder = ObligationFinder { found: &mut found };
    finder.visit_file(&file);
    Ok(found)
}

/// Renders one obligation's discharge outcome as a Gate-mode diagnostic.
/// Every obligation produces one, regardless of outcome, per spec
/// Requirement 3's "report which layer discharged it" UX -- `Proven`/
/// `Runtime` are informational (`Level::Note`, doesn't fail the build);
/// only `Violated` is `Level::Error`.
pub fn to_diagnostic(found: &FoundObligation, result: &DischargeResult) -> Diagnostic {
    match result {
        DischargeResult::Proven { layer } => Diagnostic::new(
            Level::Note,
            format!(
                "`{}` {} discharged at {}",
                found.fn_name,
                found.kind.as_str(),
                layer_str(*layer)
            ),
            found.span,
        )
        .with_label("proven"),
        DischargeResult::Runtime => Diagnostic::new(
            Level::Note,
            format!(
                "`{}` {} could not be discharged by L1-L2, inserting a runtime check",
                found.fn_name,
                found.kind.as_str()
            ),
            found.span,
        )
        .with_label("runtime fallback"),
        DischargeResult::Violated { counterexample } => Diagnostic::new(
            Level::Error,
            format!(
                "`{}` {} is violated: {counterexample}",
                found.fn_name,
                found.kind.as_str()
            ),
            found.span,
        )
        .with_label("unsatisfiable"),
    }
}

fn layer_str(layer: Layer) -> &'static str {
    match layer {
        Layer::L1 => "L1",
        Layer::L2 => "L2",
        Layer::L3 => "L3",
        Layer::L4 => "L4",
        Layer::L5 => "L5",
        Layer::Runtime => "runtime",
    }
}

/// Gate-mode entry point: finds every obligation and reports its
/// discharge outcome as a [`Diagnostic`]. Matches `rust-limit`/
/// `rust-total`'s `check_source(source: &str) -> Result<Vec<Diagnostic>,
/// _>` shape so `cargo-mvl` can dispatch to it identically.
pub fn check_source(source: &str) -> Result<Vec<Diagnostic>, CheckError> {
    let found = find_obligations(source)?;
    Ok(found
        .iter()
        .map(|f| to_diagnostic(f, &f.discharge()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_requires_and_ensures_on_a_function() {
        let found = find_obligations(
            "#[mvl::total]\n\
             #[mvl::requires(0 <= b && b <= 255)]\n\
             #[mvl::ensures(0 <= result && result <= 15)]\n\
             fn mask_low_nibble(b: i32) -> i32 { b & 15 }",
        )
        .unwrap();

        assert_eq!(found.len(), 2);
        assert_eq!(found[0].kind, ObligationKind::Requires);
        assert_eq!(found[0].fn_name, "mask_low_nibble");
        assert_eq!(found[1].kind, ObligationKind::Ensures);
    }

    #[test]
    fn function_with_no_refinement_attrs_finds_nothing() {
        let found = find_obligations("fn f(x: i32) -> i32 { x }").unwrap();
        assert!(found.is_empty());
    }

    #[test]
    fn satisfiable_requires_is_a_note_not_an_error() {
        let diagnostics =
            check_source("#[mvl::requires(x >= 0 && x < 100)]\nfn f(x: i32) {}").unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].level, Level::Note);
    }

    #[test]
    fn contradictory_requires_is_an_error() {
        let diagnostics =
            check_source("#[mvl::requires(x >= 10 && x < 5)]\nfn f(x: i32) {}").unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].level, Level::Error);
    }

    #[test]
    fn quantifier_predicate_is_silently_not_found_a_known_v1_gap() {
        // `forall i in [1..50] . pred` isn't valid `syn::Expr` syntax (see
        // #22) -- the attribute's *file-level* parse still succeeds
        // (attribute args are an opaque token stream at that layer), but
        // `RequiresAttr`'s `attr.parse_args::<Expr>()` fails, and
        // `ObligationFinder` silently drops any attribute whose args
        // don't parse (matching `rust-total`'s existing convention for
        // `#[mvl::decreases]`). The predicate-language parser needed to
        // support this is out of scope for v1 -- see the module doc.
        let found = find_obligations(
            "#[mvl::requires(forall i in [1..50] . sections.get(i) != None)]\nfn f() {}",
        )
        .unwrap();
        assert!(found.is_empty());
    }
}
