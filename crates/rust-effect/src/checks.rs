//! Effect-propagation checking (spec Requirement 4, decided v1 scope per
//! issue #9): a caller must declare every effect its same-file callees
//! declare; a pure function (no `#[mvl::effect(...)]`, or an explicit
//! empty `#[mvl::effect()]`) must not call an effectful one.
//!
//! v1 scope, deliberately smaller than real MVL's effect system (epic
//! `mvl-lang/mvl#846`): flat, exact-set matching only (no subsumption
//! hierarchy like `effect Log > Clock`), and call resolution is
//! **same-file only** — `syn`-based scanning has no type information and
//! no cross-file/cross-crate resolution, so a call to anything not
//! defined as a free function in the same file is silently unresolvable
//! and isn't flagged either way. Free functions only, not methods in
//! `impl` blocks (same limitation `rust-total` documents for the same
//! reason).

use std::collections::{HashMap, HashSet};

use mvl_rust_core::attrs::MvlAttr;
use mvl_rust_core::diagnostics::{Diagnostic, Level};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Attribute, Expr, ExprCall, Item, ItemFn};
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

/// A function's declared effect set. Absence of `#[mvl::effect(...)]` and
/// an explicit empty `#[mvl::effect()]` both map to the empty set --
/// matching MVL's "no hidden effects" principle, where not declaring any
/// effect means the function is pure.
fn effect_set(attrs: &[Attribute]) -> HashSet<String> {
    for attr in attrs {
        if let Some(Ok(MvlAttr::Effect(effect))) = MvlAttr::try_from_attribute(attr) {
            return effect.effects.iter().map(ToString::to_string).collect();
        }
    }
    HashSet::new()
}

struct CallVisitor<'a, 'd> {
    caller_name: &'a str,
    caller_effects: &'a HashSet<String>,
    functions: &'a HashMap<String, HashSet<String>>,
    diagnostics: &'d mut Vec<Diagnostic>,
}

impl<'ast> Visit<'ast> for CallVisitor<'_, '_> {
    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Expr::Path(path_expr) = &*node.func {
            if let Some(ident) = path_expr.path.get_ident() {
                let callee_name = ident.to_string();
                if let Some(callee_effects) = self.functions.get(&callee_name) {
                    let mut missing: Vec<&str> = callee_effects
                        .difference(self.caller_effects)
                        .map(String::as_str)
                        .collect();
                    if !missing.is_empty() {
                        missing.sort_unstable();
                        self.diagnostics.push(
                            Diagnostic::new(
                                Level::Error,
                                format!(
                                    "`{}` calls effectful `{callee_name}` without declaring: {}",
                                    self.caller_name,
                                    missing.join(", ")
                                ),
                                node.span(),
                            )
                            .with_label("undeclared effect propagation"),
                        );
                    }
                }
            }
        }
        visit::visit_expr_call(self, node);
    }
}

/// Runs the check against already-loaded source text.
pub fn check_source(source: &str) -> Result<Vec<Diagnostic>, CheckError> {
    let file: syn::File = syn::parse_str(source).map_err(CheckError::Parse)?;

    let mut functions: HashMap<String, HashSet<String>> = HashMap::new();
    for item in &file.items {
        if let Item::Fn(ItemFn { sig, attrs, .. }) = item {
            functions.insert(sig.ident.to_string(), effect_set(attrs));
        }
    }

    let mut diagnostics = Vec::new();
    for item in &file.items {
        if let Item::Fn(item_fn) = item {
            let caller_name = item_fn.sig.ident.to_string();
            let caller_effects = functions.get(&caller_name).cloned().unwrap_or_default();
            let mut visitor = CallVisitor {
                caller_name: &caller_name,
                caller_effects: &caller_effects,
                functions: &functions,
                diagnostics: &mut diagnostics,
            };
            visitor.visit_block(&item_fn.block);
        }
    }
    Ok(diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_calling_pure_is_fine() {
        let diagnostics = check_source("fn a() {} fn b() { a(); }").unwrap();
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn pure_calling_effectful_is_an_error() {
        let diagnostics =
            check_source("#[mvl::effect(Console)] fn log() {} fn f() { log(); }").unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Console"));
    }

    #[test]
    fn effectful_calling_effectful_with_full_declaration_is_fine() {
        let diagnostics = check_source(
            "#[mvl::effect(Console)] fn log() {} \
             #[mvl::effect(Console)] fn f() { log(); }",
        )
        .unwrap();
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn effectful_calling_effectful_with_missing_declaration_is_an_error() {
        let diagnostics = check_source(
            "#[mvl::effect(Console, Net)] fn fetch_and_log() {} \
             #[mvl::effect(Console)] fn f() { fetch_and_log(); }",
        )
        .unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Net"));
        assert!(!diagnostics[0].message.contains("Console,"));
    }

    #[test]
    fn explicit_empty_effect_attr_is_pure() {
        let diagnostics = check_source(
            "#[mvl::effect(Console)] fn log() {} \
             #[mvl::effect()] fn f() { log(); }",
        )
        .unwrap();
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn call_to_unresolvable_function_is_silently_skipped() {
        let diagnostics = check_source("fn f() { external_crate_fn(); }").unwrap();
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn an_explicit_purity_claim_is_not_verified_against_unresolvable_calls() {
        // The trust boundary, pinned (ADR-0008 §3). Both functions declare
        // purity and neither is pure -- each returns a different value on
        // successive calls -- but effects reach them through method and
        // cross-file calls, which this checker cannot resolve and is silent
        // about by design (`call_to_unresolvable_function_is_silently_skipped`
        // above is the same boundary seen from the other side).
        //
        // So `#[mvl::effect()]` is an *unverified assertion*, not an
        // established fact. This matters beyond effects: anything treating it
        // as a purity licence inherits the hole. L1 reflexivity trusting it
        // would make `(wall_clock()) == wall_clock()` provable, dropping a
        // check that can genuinely fail -- the #44 regression arriving through
        // the explicit annotation rather than through absence. #45.
        let diagnostics = check_source(
            "#[mvl::effect()] fn wall_clock() -> i64 { \
                 std::time::SystemTime::now().elapsed().unwrap().as_secs() as i64 \
             } \
             #[mvl::effect()] fn counter(c: &std::cell::Cell<i64>) -> i64 { \
                 c.set(c.get() + 1); c.get() \
             }",
        )
        .unwrap();
        assert!(
            diagnostics.is_empty(),
            "documents the gap rather than endorsing it: a purity claim this \
             checker cannot check is accepted in silence"
        );
    }

    #[test]
    fn self_recursive_call_is_always_fine() {
        let diagnostics =
            check_source("#[mvl::effect(Console)] fn f(n: i32) { if n > 0 { f(n - 1); } }")
                .unwrap();
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn malformed_source_returns_parse_error() {
        assert!(check_source("fn f( {{{").is_err());
    }
}
