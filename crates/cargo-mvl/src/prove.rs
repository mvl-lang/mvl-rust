//! `cargo mvl prove`: aggregates `rust-refine`'s obligation trace across
//! files into one assurance-JSON `ProveSection` (spec Requirement 15).
//! `rust-refine` already discharges every `requires`/`ensures` obligation
//! per file (#8) -- this runs that same logic per file and bakes each
//! obligation's provenance with the file path, the same convention
//! `rust-refine`'s own `--emit-verification-json` uses.

use mvl_rust_core::solver::{DischargeResult, Obligation};
use rust_refine::checks::{self, CheckError};

/// Obligations found in `source`, discharged, with provenance bound to
/// `origin` (the file path `source` was read from).
pub fn prove_source(
    origin: &str,
    source: &str,
) -> Result<Vec<(Obligation, DischargeResult)>, CheckError> {
    let found = checks::find_obligations(source)?;
    Ok(found
        .iter()
        .map(|f| {
            let result = f.discharge();
            let start = f.span.start();
            let obligation = Obligation {
                id: f.id(),
                predicate: f.predicate_text(),
                provenance: format!("{origin}:{}:{}", start.line, start.column + 1),
                kind: f.class(),
            };
            (obligation, result)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregates_obligations_with_file_provenance() {
        let results = prove_source(
            "src/lib.rs",
            "#[mvl::requires(0 <= b && b <= 255)]\nfn f(b: i32) {}",
        )
        .unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].0.provenance.starts_with("src/lib.rs:"));
    }

    #[test]
    fn no_obligations_yields_an_empty_vec() {
        let results = prove_source("src/lib.rs", "fn f() {}").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn malformed_source_returns_a_parse_error() {
        assert!(prove_source("src/lib.rs", "fn f( {{{").is_err());
    }
}
