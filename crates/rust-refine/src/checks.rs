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
//! Predicates are plain comparison/boolean expressions, or a bounded
//! quantifier (`forall`/`exists i in [lo..hi]. pred`) — see
//! `mvl_rust_core::attrs::Predicate` (#31) for the grammar and
//! `mvl_rust_core::solver::native` for how each is discharged (`L1`/`L2`
//! for plain expressions, `L3` expansion for quantifiers).

use mvl_rust_core::attrs::{MvlAttr, Predicate};
use mvl_rust_core::diagnostics::{Diagnostic, Level};
use mvl_rust_core::solver::native::discharge_predicate;
use mvl_rust_core::solver::{DischargeResult, Layer};
use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::ItemFn;
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
    pub predicate: Predicate,
    pub span: Span,
}

impl FoundObligation {
    pub fn id(&self) -> String {
        format!("{}::{}", self.fn_name, self.kind.as_str())
    }

    pub fn predicate_text(&self) -> String {
        self.predicate.render()
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
    fn bounded_quantifier_predicate_is_found_and_discharged_at_l3() {
        let diagnostics =
            check_source("#[mvl::requires(forall i in [0..9] . i < 10)]\nfn f(sections: i32) {}")
                .unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].level, Level::Note);
        assert!(diagnostics[0].message.contains("L3"));
    }

    #[test]
    fn bounded_quantifier_over_an_opaque_call_falls_to_runtime() {
        // Matches the `require_dense_fleet` shape: L3 unrolling doesn't
        // spuriously "prove" what the inner backend can't decide.
        let diagnostics = check_source(
            "#[mvl::requires(forall i in [1..50] . sections_get(i) != 0)]\nfn f(sections: i32) {}",
        )
        .unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].level, Level::Note);
        assert!(diagnostics[0].message.contains("runtime check"));
    }

    #[test]
    fn genuinely_unsatisfiable_quantifier_fails_the_build() {
        let diagnostics =
            check_source("#[mvl::requires(forall i in [0..9] . i < 5)]\nfn f() {}").unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].level, Level::Error);
        assert!(diagnostics[0].message.contains("i = 5"));
    }
}
