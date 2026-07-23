//! Per-construct lint checks. Each submodule implements one forbidden
//! construct from spec Requirement 1 as a `syn::visit::Visit` pass and
//! reports violations as [`mvl_rust_core::diagnostics::Diagnostic`].

mod dyn_trait;
mod lifetimes;
mod macros;
mod raw_addr;
mod transmute;
mod unsafe_construct;

use mvl_rust_core::diagnostics::Diagnostic;
use std::path::Path;
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

/// Reads and checks the file at `path` against every qualified-subset rule,
/// returning every violation found (empty if fully compliant).
pub fn check_file(path: &Path) -> Result<Vec<Diagnostic>, CheckError> {
    let source = std::fs::read_to_string(path).map_err(|source| CheckError::Io {
        path: path.display().to_string(),
        source,
    })?;
    check_source(&source)
}

/// Runs every check against already-loaded source text.
pub fn check_source(source: &str) -> Result<Vec<Diagnostic>, CheckError> {
    let file: syn::File = syn::parse_str(source).map_err(CheckError::Parse)?;
    let mut diagnostics = Vec::new();
    unsafe_construct::check(&file, &mut diagnostics);
    dyn_trait::check(&file, &mut diagnostics);
    lifetimes::check(&file, &mut diagnostics);
    macros::check(&file, &mut diagnostics);
    transmute::check(&file, &mut diagnostics);
    raw_addr::check(&file, &mut diagnostics);
    Ok(diagnostics)
}
