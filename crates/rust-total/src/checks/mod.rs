//! Entry point: finds every `#[mvl::total]`-annotated function in a source
//! file and runs both checks (panic-freedom, termination) against it.
//! Functions without `#[mvl::total]` aren't scanned at all — rust-total's
//! checks are opt-in per function, not file-wide.
//!
//! **Impl methods are checked too** (issue #89, following `rust-refine`'s
//! fix for the identical gap): [`mvl_rust_core::impl_methods::impl_methods`]
//! collects every method across the file's `impl` blocks, and each one
//! carrying `#[mvl::total]` is checked by cloning its `attrs`/`vis`/`sig`/
//! `block` into a synthetic [`ItemFn`] — the three check modules
//! (`panic_freedom`, `termination`, `loop_termination`) stay typed against
//! `&ItemFn` unchanged, since a method and a free function share that exact
//! shape once separated from their enclosing `impl`.
//!
//! **Known simplification**, unlike `rust-refine`'s obligation ids: a
//! method's diagnostics use its own bare name (`usable_page_size`), not the
//! qualified `Type::method` form — the three check modules build their
//! diagnostic text from `&Ident` (the synthetic `ItemFn`'s own
//! `sig.ident`), and `Type::method` isn't a valid `Ident` (`::` isn't an
//! identifier character). Qualifying it would mean threading a separate
//! display string through every diagnostic builder in all three modules
//! for a caret-pointed message that already names its span; not done here,
//! `rust-total` has no cross-function obligation-id map that would
//! actually collide on an unqualified name the way `rust-refine`'s
//! `functions: HashMap<String, FnFacts>` could.

mod loop_termination;
mod panic_freedom;
mod termination;

use mvl_rust_core::attrs::{MvlAttr, Predicate};
use mvl_rust_core::diagnostics::Diagnostic;
use mvl_rust_core::impl_methods::impl_methods;
use std::path::Path;
use syn::visit::{self, Visit};
use syn::{Attribute, Expr, ItemFn};
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

/// Reads and checks the file at `path`, returning every violation found
/// (empty if fully compliant, or if it uses no `#[mvl::total]` functions).
pub fn check_file(path: &Path) -> Result<Vec<Diagnostic>, CheckError> {
    let source = std::fs::read_to_string(path).map_err(|source| CheckError::Io {
        path: path.display().to_string(),
        source,
    })?;
    check_source(&source)
}

/// Runs the checks against already-loaded source text.
pub fn check_source(source: &str) -> Result<Vec<Diagnostic>, CheckError> {
    let file: syn::File = syn::parse_str(source).map_err(CheckError::Parse)?;
    let mut diagnostics = Vec::new();
    let mut finder = TotalFnFinder {
        diagnostics: &mut diagnostics,
    };
    finder.visit_file(&file);

    for (_name, method) in impl_methods(&file) {
        check_total_item(&method_as_item_fn(method), &mut diagnostics);
    }
    Ok(diagnostics)
}

/// A method's `attrs`/`vis`/`sig`/`block`, cloned into a free-standing
/// `ItemFn` -- see the module doc for why this is the chosen adapter
/// rather than generalizing the three check modules over `ImplItemFn` too.
fn method_as_item_fn(method: &syn::ImplItemFn) -> ItemFn {
    ItemFn {
        attrs: method.attrs.clone(),
        vis: method.vis.clone(),
        sig: method.sig.clone(),
        block: Box::new(method.block.clone()),
    }
}

/// The same `#[mvl::total]` dispatch [`TotalFnFinder::visit_item_fn`] does,
/// factored out so the impl-method loop above and the free-function
/// visitor share one call site rather than drifting apart.
fn check_total_item(item_fn: &ItemFn, diagnostics: &mut Vec<Diagnostic>) {
    let (is_total, decreases, requires) = total_decreases_and_requires(&item_fn.attrs);
    if is_total {
        panic_freedom::check(item_fn, diagnostics);
        termination::check(item_fn, decreases.as_ref(), &requires, diagnostics);
        loop_termination::check(item_fn, &requires, diagnostics);
    }
}

/// `requires` collects only the `Predicate::Expr` clauses -- a quantified
/// `requires` (`forall`/`exists`) isn't a plain `syn::Expr` hypothesis
/// `discharge_entailment` can take, so it's skipped here rather than
/// threaded through as something it isn't. That only narrows what
/// `decreases` can prove; it never widens it incorrectly.
fn total_decreases_and_requires(attrs: &[Attribute]) -> (bool, Option<Expr>, Vec<Expr>) {
    let mut is_total = false;
    let mut decreases = None;
    let mut requires = Vec::new();
    for attr in attrs {
        match MvlAttr::try_from_attribute(attr) {
            Some(Ok(MvlAttr::Total(_))) => is_total = true,
            Some(Ok(MvlAttr::Decreases(attr))) => decreases = Some(attr.measure),
            Some(Ok(MvlAttr::Requires(attr))) => {
                if let Predicate::Expr(expr) = attr.predicate {
                    requires.push(expr);
                }
            }
            _ => {}
        }
    }
    (is_total, decreases, requires)
}

struct TotalFnFinder<'d> {
    diagnostics: &'d mut Vec<Diagnostic>,
}

impl<'ast> Visit<'ast> for TotalFnFinder<'_> {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        check_total_item(node, self.diagnostics);
        visit::visit_item_fn(self, node);
    }
}
