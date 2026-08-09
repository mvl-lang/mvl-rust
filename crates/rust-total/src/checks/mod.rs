//! Entry point: finds every `#[mvl::total]`-annotated function in a source
//! file and runs both checks (panic-freedom, termination) against it.
//! Functions without `#[mvl::total]` aren't scanned at all — rust-total's
//! checks are opt-in per function, not file-wide.
//!
//! v1 scope: free functions only (`fn f() { ... }`). Methods inside `impl`
//! blocks aren't scanned yet — a deliberate v1 limitation, not an
//! oversight, to keep the check functions cleanly typed against `ItemFn`
//! rather than generalizing over both `ItemFn` and `ImplItemFn`.

mod panic_freedom;
mod termination;

use mvl_rust_core::attrs::MvlAttr;
use mvl_rust_core::diagnostics::Diagnostic;
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
    Ok(diagnostics)
}

fn total_and_decreases(attrs: &[Attribute]) -> (bool, Option<Expr>) {
    let mut is_total = false;
    let mut decreases = None;
    for attr in attrs {
        match MvlAttr::try_from_attribute(attr) {
            Some(Ok(MvlAttr::Total(_))) => is_total = true,
            Some(Ok(MvlAttr::Decreases(attr))) => decreases = Some(attr.measure),
            _ => {}
        }
    }
    (is_total, decreases)
}

struct TotalFnFinder<'d> {
    diagnostics: &'d mut Vec<Diagnostic>,
}

impl<'ast> Visit<'ast> for TotalFnFinder<'_> {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let (is_total, decreases) = total_and_decreases(&node.attrs);
        if is_total {
            panic_freedom::check(node, self.diagnostics);
            termination::check(node, decreases.as_ref(), self.diagnostics);
        }
        visit::visit_item_fn(self, node);
    }
}
