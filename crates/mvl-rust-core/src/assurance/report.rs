//! Helpers for building an [`AssuranceReport`] from a Gate-mode tool's own
//! diagnostics (spec Requirement 14). Shared since every diagnostic-only
//! tool (`rust-limit`, `rust-total`, and eventually `rust-effect`/
//! `rust-ifc`) needs the identical `Diagnostic` → `DiagnosticRecord` →
//! `CheckSection` → `AssuranceReport` pipeline, not just one of them.
//! `rust-refine`'s `prove` section (obligations, not diagnostics) is a
//! different shape and isn't covered here.

use super::schema::{AssuranceReport, CheckSection, DiagnosticRecord};
use crate::diagnostics::{Diagnostic, Level};

fn level_str(level: Level) -> &'static str {
    match level {
        Level::Error => "error",
        Level::Warning => "warning",
        Level::Note => "note",
    }
}

/// Converts one [`Diagnostic`] into its wire form. `origin` is the file
/// path the diagnostic's span belongs to — `Diagnostic` itself only
/// carries a span, not a filename, so the caller (which knows which file
/// it just scanned) supplies it.
pub fn diagnostic_to_record(diagnostic: &Diagnostic, origin: &str) -> DiagnosticRecord {
    let start = diagnostic.span.start();
    DiagnosticRecord {
        level: level_str(diagnostic.level).to_string(),
        message: diagnostic.message.clone(),
        // `start.column` is 0-indexed (per proc_macro2::LineColumn's own
        // doc comment); +1 for the conventional 1-indexed display rustc
        // itself uses.
        provenance: format!("{origin}:{}:{}", start.line, start.column + 1),
        label: diagnostic.label.clone(),
        suggestion: diagnostic.suggestion.clone(),
    }
}

/// Builds a full assurance report for a Gate-mode tool's `check` section.
///
/// `tool_name` becomes `target.crate` — a stand-in until these tools
/// operate at whole-crate granularity rather than per-file (a known
/// simplification, not a final design). `timestamp` is caller-supplied
/// (rather than read from the clock in here) so this stays testable
/// without needing to mock time.
pub fn build_check_report(
    tool_name: impl Into<String>,
    timestamp: impl Into<String>,
    diagnostics: Vec<DiagnosticRecord>,
) -> AssuranceReport {
    let mut report = AssuranceReport::new(tool_name, timestamp);
    report.check = Some(CheckSection {
        obligations: vec![],
        diagnostics,
    });
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use proc_macro2::Span;
    use syn::spanned::Spanned;

    fn sample_span() -> Span {
        let file: syn::File = syn::parse_str("fn f() { unsafe {} }").unwrap();
        match &file.items[0] {
            syn::Item::Fn(f) => f.block.span(),
            _ => unreachable!(),
        }
    }

    #[test]
    fn diagnostic_to_record_formats_provenance_as_file_line_col() {
        let diagnostic = Diagnostic::new(Level::Error, "test message", sample_span())
            .with_label("test label")
            .with_suggestion("test suggestion");

        let record = diagnostic_to_record(&diagnostic, "src/lib.rs");

        assert_eq!(record.level, "error");
        assert_eq!(record.message, "test message");
        assert_eq!(record.provenance, "src/lib.rs:1:8");
        assert_eq!(record.label.as_deref(), Some("test label"));
        assert_eq!(record.suggestion.as_deref(), Some("test suggestion"));
    }

    #[test]
    fn build_check_report_populates_check_section_only() {
        let diagnostic = Diagnostic::new(Level::Warning, "warn message", sample_span());
        let record = diagnostic_to_record(&diagnostic, "src/lib.rs");

        let report = build_check_report("rust-limit", "2026-07-27T00:00:00Z", vec![record]);

        assert_eq!(report.target.crate_name, "rust-limit");
        let check = report.check.expect("check section should be populated");
        assert_eq!(check.diagnostics.len(), 1);
        assert_eq!(check.diagnostics[0].level, "warning");
        assert!(check.obligations.is_empty());
        assert!(report.prove.is_none());
        assert!(report.test.is_none());
    }
}
