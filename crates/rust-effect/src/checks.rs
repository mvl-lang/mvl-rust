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
//! defined as a free function in the same file (including any method
//! call) is unresolvable: it isn't flagged as a propagation violation
//! either way.
//!
//! **Impl methods are checked too** (issue #89, following `rust-refine`'s
//! and `rust-total`'s fixes for the identical gap): a method's own
//! declared effect set is collected and its body scanned exactly like a
//! free function's, keyed by a qualified `"Type::method"` name
//! ([`mvl_rust_core::impl_methods::impl_methods`]) so it can't collide
//! with a free function or another impl's identically named method. What
//! stays unchanged: call resolution is still same-file *free functions*
//! only — a call *into* a checked method (`self.foo()`, `x.method()`,
//! `Type::method(x)`) is exactly as unresolvable as it was before, for the
//! same reason (no type information to resolve a receiver's type).
//!
//! That silence is fine for an *implicit* pure function -- absence of
//! `#[mvl::effect(...)]` never claimed anything was checked. It is not
//! fine for an *explicit* `#[mvl::effect()]`, which is a positive claim of
//! purity (issue #67, ADR-0008 §3): when such a function contains
//! unresolvable calls, the claim is trusted rather than checked, and a
//! `Level::Note` diagnostic says so.

use std::collections::{HashMap, HashSet};

use mvl_rust_core::attrs::MvlAttr;
use mvl_rust_core::diagnostics::{Diagnostic, Level};
use mvl_rust_core::impl_methods::impl_methods;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Attribute, Expr, ExprCall, ExprMethodCall, Item, ItemFn};
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

/// Whether `#[mvl::effect(...)]` was written at all, as distinct from
/// [`effect_set`]'s empty set, which absence and an explicit empty
/// attribute both produce. Needed to flag *explicit* purity claims (issue
/// #67) without also flagging ordinary implicit-pure functions that simply
/// never mentioned effects.
fn has_explicit_effect_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        matches!(
            MvlAttr::try_from_attribute(attr),
            Some(Ok(MvlAttr::Effect(_)))
        )
    })
}

struct CallVisitor<'a, 'd> {
    caller_name: &'a str,
    caller_effects: &'a HashSet<String>,
    functions: &'a HashMap<String, HashSet<String>>,
    diagnostics: &'d mut Vec<Diagnostic>,
    unresolved_calls: &'d mut usize,
}

impl<'ast> Visit<'ast> for CallVisitor<'_, '_> {
    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        let mut resolved = false;
        if let Expr::Path(path_expr) = &*node.func {
            if let Some(ident) = path_expr.path.get_ident() {
                let callee_name = ident.to_string();
                if let Some(callee_effects) = self.functions.get(&callee_name) {
                    resolved = true;
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
        if !resolved {
            *self.unresolved_calls += 1;
        }
        visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        // Method calls carry no callee-side effect info `syn` can see --
        // always unresolvable, same boundary as a cross-file free-function
        // call (module doc comment).
        *self.unresolved_calls += 1;
        visit::visit_expr_method_call(self, node);
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
    for (name, method) in impl_methods(&file) {
        functions.insert(name, effect_set(&method.attrs));
    }

    let mut diagnostics = Vec::new();
    for item in &file.items {
        if let Item::Fn(item_fn) = item {
            let caller_name = item_fn.sig.ident.to_string();
            scan_caller(
                &caller_name,
                &item_fn.attrs,
                item_fn.sig.span(),
                &item_fn.block,
                &functions,
                &mut diagnostics,
            );
        }
    }
    for (name, method) in impl_methods(&file) {
        scan_caller(
            &name,
            &method.attrs,
            method.sig.span(),
            &method.block,
            &functions,
            &mut diagnostics,
        );
    }
    Ok(diagnostics)
}

/// Scans one function-or-method body for undeclared effect propagation,
/// shared by [`check_source`]'s free-function and impl-method loops --
/// the scan itself doesn't care which kind of item `caller_name`/`attrs`/
/// `block` came from.
fn scan_caller(
    caller_name: &str,
    attrs: &[Attribute],
    sig_span: proc_macro2::Span,
    block: &syn::Block,
    functions: &HashMap<String, HashSet<String>>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let caller_effects = functions.get(caller_name).cloned().unwrap_or_default();
    let mut unresolved_calls = 0usize;
    let mut visitor = CallVisitor {
        caller_name,
        caller_effects: &caller_effects,
        functions,
        diagnostics,
        unresolved_calls: &mut unresolved_calls,
    };
    visitor.visit_block(block);

    // A purity claim (explicit `#[mvl::effect()]`) is verified only
    // against same-file, resolvable, free-function calls (module doc
    // comment). When such a function contains calls this checker cannot
    // see through, the claim is trusted, not checked -- say so rather
    // than staying silent (issue #67).
    if unresolved_calls > 0 && caller_effects.is_empty() && has_explicit_effect_attr(attrs) {
        diagnostics.push(
            Diagnostic::new(
                Level::Note,
                format!(
                    "`{caller_name}` claims purity via `#[mvl::effect()]`, but this \
                     is not verified: {unresolved_calls} unresolvable call(s)"
                ),
                sig_span,
            )
            .with_label("purity claim not verified"),
        );
    }
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
        // The trust boundary (ADR-0008 §3, issue #67). Both functions
        // declare purity and neither is pure -- each returns a different
        // value on successive calls -- but effects reach them through
        // method and cross-file calls, which this checker cannot resolve
        // (`call_to_unresolvable_function_is_silently_skipped` above is the
        // same boundary seen from the other side, for an implicit-pure
        // function, where it stays silent).
        //
        // So `#[mvl::effect()]` is an *unverified assertion*, not an
        // established fact -- and now flagged as such, at `Level::Note`, so
        // the report doesn't present it as an established one. This matters
        // beyond effects: anything treating it as a purity licence inherits
        // the hole. L1 reflexivity trusting it would make
        // `(wall_clock()) == wall_clock()` provable, dropping a check that
        // can genuinely fail -- the #44 regression arriving through the
        // explicit annotation rather than through absence. #45.
        let diagnostics = check_source(
            "#[mvl::effect()] fn wall_clock() -> i64 { \
                 std::time::SystemTime::now().elapsed().unwrap().as_secs() as i64 \
             } \
             #[mvl::effect()] fn counter(c: &std::cell::Cell<i64>) -> i64 { \
                 c.set(c.get() + 1); c.get() \
             }",
        )
        .unwrap();
        assert_eq!(diagnostics.len(), 2);
        for diagnostic in &diagnostics {
            assert_eq!(diagnostic.level, Level::Note);
            assert!(diagnostic.message.contains("not verified"));
        }
        assert!(diagnostics[0].message.contains("wall_clock"));
        assert!(diagnostics[1].message.contains("counter"));
    }

    #[test]
    fn implicit_pure_function_with_unresolvable_calls_stays_silent() {
        // No `#[mvl::effect(...)]` at all means no claim was made -- unlike
        // the explicit-empty-attribute case above, there's nothing to flag
        // as unverified.
        let diagnostics =
            check_source("fn f(c: &std::cell::Cell<i64>) -> i64 { c.set(c.get() + 1); c.get() }")
                .unwrap();
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn explicit_non_empty_effect_with_unresolvable_calls_stays_silent() {
        // Issue #67 is scoped to purity claims specifically -- a function
        // that already declares effects isn't claiming to be call-safe in
        // the way an explicit empty `#[mvl::effect()]` is.
        let diagnostics = check_source(
            "#[mvl::effect(Console)] fn f(c: &std::cell::Cell<i64>) -> i64 { \
                 c.set(c.get() + 1); c.get() \
             }",
        )
        .unwrap();
        assert!(diagnostics.is_empty());
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

    // ── impl-method scope (#89) ──────────────────────────────────────

    #[test]
    fn an_impl_method_declaring_the_same_effect_as_its_callee_is_fine() {
        let diagnostics = check_source(
            "#[mvl::effect(Console)] fn log() {} \
             struct T; \
             impl T { \
                 #[mvl::effect(Console)] \
                 fn f(&self) { log(); } \
             }",
        )
        .unwrap();
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn an_impl_method_missing_a_declared_effect_is_an_error() {
        let diagnostics = check_source(
            "#[mvl::effect(Console)] fn log() {} \
             struct T; \
             impl T { \
                 fn f(&self) { log(); } \
             }",
        )
        .unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Console"));
    }

    #[test]
    fn a_method_and_a_free_function_with_the_same_name_have_independent_effects() {
        let diagnostics = check_source(
            "#[mvl::effect(Console)] fn f() {} \
             struct T; \
             impl T { \
                 #[mvl::effect(Console)] \
                 fn f(&self) {} \
             }",
        )
        .unwrap();
        assert!(diagnostics.is_empty());
    }
}
