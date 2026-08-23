//! Entry point: scans every `fn` item and `impl` method in a source file,
//! whole-file, and requires each one to carry exactly one of `#[mvl::total]`
//! or `#[mvl::partial]` (ADR-0012, #117). There is no third, silent,
//! unannotated state any more — a function with neither attribute is a
//! diagnostic error demanding an explicit declaration, and one with both is
//! a diagnostic error too. A `#[mvl::total]` function gets all three checks
//! (panic-freedom, termination, swallow); a `#[mvl::partial]` function gets
//! none of them — it has explicitly opted out, rather than having been
//! silently skipped.
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
mod swallow;
mod termination;

use mvl_rust_core::attrs::{MvlAttr, Predicate};
use mvl_rust_core::diagnostics::{Diagnostic, Level};
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

/// Which of the three checks to run — the `--check` CLI flag's parsed form.
/// Defaults to all three; a subset is opt-in narrowing, never widening.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckSet {
    pub panic: bool,
    pub termination: bool,
    pub swallow: bool,
}

impl CheckSet {
    pub const ALL: CheckSet = CheckSet {
        panic: true,
        termination: true,
        swallow: true,
    };

    /// Parses a comma-separated `--check` value, e.g. `"panic,swallow"`.
    /// An unrecognized name is an error rather than silently ignored, so a
    /// typo (`--check=pnaic`) doesn't quietly turn into "no checks".
    pub fn parse(spec: &str) -> Result<CheckSet, String> {
        let mut set = CheckSet {
            panic: false,
            termination: false,
            swallow: false,
        };
        for name in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            match name {
                "panic" => set.panic = true,
                "termination" => set.termination = true,
                "swallow" => set.swallow = true,
                other => return Err(format!("unknown --check value: `{other}`")),
            }
        }
        Ok(set)
    }
}

/// Reads and checks the file at `path`, returning every violation found
/// (empty if fully compliant, or if it uses no `#[mvl::total]` functions).
pub fn check_file(path: &Path) -> Result<Vec<Diagnostic>, CheckError> {
    check_file_with(path, CheckSet::ALL)
}

/// Same as [`check_file`], scoped to a subset of checks.
pub fn check_file_with(path: &Path, checks: CheckSet) -> Result<Vec<Diagnostic>, CheckError> {
    let source = std::fs::read_to_string(path).map_err(|source| CheckError::Io {
        path: path.display().to_string(),
        source,
    })?;
    check_source_with(&source, checks)
}

/// Runs all checks against already-loaded source text.
pub fn check_source(source: &str) -> Result<Vec<Diagnostic>, CheckError> {
    check_source_with(source, CheckSet::ALL)
}

/// Same as [`check_source`], scoped to a subset of checks.
pub fn check_source_with(source: &str, checks: CheckSet) -> Result<Vec<Diagnostic>, CheckError> {
    let file: syn::File = syn::parse_str(source).map_err(CheckError::Parse)?;
    let mut diagnostics = Vec::new();
    let mut finder = FnFinder {
        diagnostics: &mut diagnostics,
        checks,
    };
    finder.visit_file(&file);

    for (_name, method) in impl_methods(&file) {
        check_item(&method_as_item_fn(method), checks, &mut diagnostics);
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

/// The same declaration-dispatch [`FnFinder::visit_item_fn`] does, factored
/// out so the impl-method loop above and the free-function visitor share
/// one call site rather than drifting apart.
///
/// ADR-0012: every function must carry exactly one of `#[mvl::total]` /
/// `#[mvl::partial]`. Neither, or both, is itself a diagnostic error rather
/// than being resolved one way or the other by default — there is no
/// silent fallback in either direction.
fn check_item(item_fn: &ItemFn, checks: CheckSet, diagnostics: &mut Vec<Diagnostic>) {
    let (is_total, is_partial, decreases, requires) = declaration_and_attrs(&item_fn.attrs);
    match (is_total, is_partial) {
        (true, true) => diagnostics.push(Diagnostic::new(
            Level::Error,
            format!(
                "`{}` cannot be both `#[mvl::total]` and `#[mvl::partial]`",
                item_fn.sig.ident
            ),
            item_fn.sig.ident.span(),
        )),
        (false, false) => diagnostics.push(
            Diagnostic::new(
                Level::Error,
                format!(
                    "`{}` must be explicitly declared `#[mvl::total]` or `#[mvl::partial]`",
                    item_fn.sig.ident
                ),
                item_fn.sig.ident.span(),
            )
            .with_label("no totality declaration")
            .with_suggestion(
                "add `#[mvl::total]` if this function claims panic-freedom and termination, or `#[mvl::partial]` to explicitly opt out",
            ),
        ),
        (true, false) => {
            if checks.panic {
                panic_freedom::check(item_fn, diagnostics);
            }
            if checks.termination {
                termination::check(item_fn, decreases.as_ref(), &requires, diagnostics);
                loop_termination::check(item_fn, &requires, diagnostics);
            }
            if checks.swallow {
                swallow::check(item_fn, diagnostics);
            }
        }
        (false, true) => {}
    }
}

/// `requires` collects only the `Predicate::Expr` clauses -- a quantified
/// `requires` (`forall`/`exists`) isn't a plain `syn::Expr` hypothesis
/// `discharge_entailment` can take, so it's skipped here rather than
/// threaded through as something it isn't. That only narrows what
/// `decreases` can prove; it never widens it incorrectly.
fn declaration_and_attrs(attrs: &[Attribute]) -> (bool, bool, Option<Expr>, Vec<Expr>) {
    let mut is_total = false;
    let mut is_partial = false;
    let mut decreases = None;
    let mut requires = Vec::new();
    for attr in attrs {
        match MvlAttr::try_from_attribute(attr) {
            Some(Ok(MvlAttr::Total(_))) => is_total = true,
            Some(Ok(MvlAttr::Partial(_))) => is_partial = true,
            Some(Ok(MvlAttr::Decreases(attr))) => decreases = Some(attr.measure),
            Some(Ok(MvlAttr::Requires(attr))) => {
                if let Predicate::Expr(expr) = attr.predicate {
                    requires.push(expr);
                }
            }
            _ => {}
        }
    }
    (is_total, is_partial, decreases, requires)
}

struct FnFinder<'d> {
    diagnostics: &'d mut Vec<Diagnostic>,
    checks: CheckSet,
}

impl<'ast> Visit<'ast> for FnFinder<'_> {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        check_item(node, self.checks, self.diagnostics);
        visit::visit_item_fn(self, node);
    }
}
