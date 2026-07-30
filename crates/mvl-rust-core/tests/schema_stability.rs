//! Two things, per issue #13's acceptance criteria:
//!
//! 1. The committed `schemas/assurance-v<ASSURANCE_SCHEMA_VERSION>.json` must match what
//!    `schemars` derives from `AssuranceReport` right now — if they
//!    differ, the schema shape changed without deliberately regenerating
//!    the committed file (and, per `assurance/version.rs`'s doc comment,
//!    without bumping `ASSURANCE_SCHEMA_VERSION`).
//! 2. "Rust types + JSON Schema in sync (verified via a test that
//!    round-trips)" — a real, fully-populated sample report serializes to
//!    JSON and deserializes back to the same value.
//!
//! (An earlier draft of this file validated a sample report against the
//! schema using the `jsonschema` crate — stricter than "round-trips," but
//! its dependency tree pulls in `icu_*`/`idna`/`url` crates that require a
//! newer `rustc` than this workspace's MSRV, breaking CI. Reverted to what
//! the acceptance criteria actually asks for.)

use mvl_rust_core::assurance::schema::{
    AssuranceReport, AssuranceSection, CheckSection, CoverageSection, CoverageStat,
    DiagnosticRecord, McdcCondition, McdcSection, ObligationRecord, ProveSection, TestRecord,
    TestSection, TestSummary,
};
use mvl_rust_core::assurance::version::ASSURANCE_SCHEMA_VERSION;
use mvl_rust_core::solver::{DischargeResult, Layer, Obligation, ObligationClass};
use std::path::Path;

/// Derived from [`ASSURANCE_SCHEMA_VERSION`] rather than hardcoded, so a
/// version bump cannot leave this test validating against a superseded
/// file. It was hardcoded to `v1.0` until #56 needed the first real bump.
fn committed_schema_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("schemas")
        .join(format!("assurance-v{ASSURANCE_SCHEMA_VERSION}.json"))
}

#[test]
fn committed_schema_matches_the_derived_schema() {
    let derived = schemars::schema_for!(AssuranceReport);
    let derived_value = serde_json::to_value(&derived).unwrap();

    let path = committed_schema_path();
    let committed_text = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    let committed_value: serde_json::Value = serde_json::from_str(&committed_text).unwrap();

    assert_eq!(
        derived_value,
        committed_value,
        "{} is out of date with AssuranceReport's current shape.\n\
         Regenerate it with: cargo test -p mvl-rust-core --test schema_stability -- \\\n\
         \x20  --ignored bless_committed_schema\n\
         Then decide whether ASSURANCE_SCHEMA_VERSION needs bumping: only a *shape* change \
         does (field added, removed, or retyped). A doc-comment edit also lands here, because \
         `schemars` embeds doc comments as `description` -- regenerate, but do NOT bump, or \
         you falsely signal a break to consumers pinned to the current version. See #64.",
        path.display()
    );
}

/// Regenerates the committed schema for the *current*
/// [`ASSURANCE_SCHEMA_VERSION`]. Ignored by default — this writes a
/// checked-in file, so it runs only when asked:
///
/// ```text
/// cargo test -p mvl-rust-core --test schema_stability -- --ignored bless_committed_schema
/// ```
///
/// Before #56 there was no supported way to do this; the file was
/// hand-maintained, which is how it drifted (#64). Earlier versions' files
/// stay committed and are never rewritten by this — a consumer pinned to an
/// older version can still validate against the shape it was promised.
#[test]
#[ignore = "writes a checked-in file; run explicitly with --ignored"]
fn bless_committed_schema() {
    let derived = schemars::schema_for!(AssuranceReport);
    let mut text = serde_json::to_string_pretty(&derived).unwrap();
    text.push('\n');
    let path = committed_schema_path();
    std::fs::write(&path, text)
        .unwrap_or_else(|err| panic!("failed to write {}: {err}", path.display()));
    eprintln!("wrote {}", path.display());
}

fn fully_populated_report() -> AssuranceReport {
    let mut report = AssuranceReport::new("rust-limit", "2026-07-27T00:00:00Z");

    report.check = Some(CheckSection {
        obligations: vec![],
        diagnostics: vec![DiagnosticRecord {
            level: "error".into(),
            message: "`unsafe` block is outside the qualified subset".into(),
            provenance: "src/lib.rs:3:5".into(),
            label: Some("unsafe block".into()),
            suggestion: None,
        }],
    });

    let obligation = Obligation {
        id: "ob1".into(),
        predicate: "x >= 0 && x < 100".into(),
        provenance: "src/lib.rs:10:1".into(),
        kind: ObligationClass::CallSite,
    };
    let result = DischargeResult::Proven { layer: Layer::L2 };
    report.prove = Some(ProveSection {
        obligations: vec![ObligationRecord::new(&obligation, &result)],
    });

    report.test = Some(TestSection {
        tests: vec![TestRecord {
            name: "it_works".into(),
            outcome: "passed".into(),
            duration_ms: 12,
        }],
        summary: TestSummary {
            passed: 1,
            failed: 0,
            ignored: 0,
        },
    });

    report.mcdc = Some(McdcSection {
        conditions: vec![McdcCondition {
            id: "cond1".into(),
            covered: true,
        }],
        coverage_pct: 87.5,
    });

    report.coverage = Some(CoverageSection {
        lines: CoverageStat {
            covered: 90,
            total: 100,
        },
        branches: CoverageStat {
            covered: 40,
            total: 50,
        },
    });

    report.assurance = Some(AssuranceSection {
        claim: "src/lib.rs satisfies its declared obligations".into(),
        argument_tree: serde_json::json!({ "kind": "placeholder", "see": "#22" }),
        leaves: vec![],
    });

    report
}

#[test]
fn a_fully_populated_report_round_trips_through_json() {
    let report = fully_populated_report();
    let json = serde_json::to_string(&report).unwrap();
    let decoded: AssuranceReport = serde_json::from_str(&json).unwrap();
    assert_eq!(
        serde_json::to_value(&decoded).unwrap(),
        serde_json::to_value(&report).unwrap()
    );
}

#[test]
fn a_minimal_report_with_every_section_absent_round_trips_through_json() {
    let report = AssuranceReport::new("rust-total", "2026-07-27T00:00:00Z");
    let json = serde_json::to_string(&report).unwrap();
    let decoded: AssuranceReport = serde_json::from_str(&json).unwrap();
    assert_eq!(
        serde_json::to_value(&decoded).unwrap(),
        serde_json::to_value(&report).unwrap()
    );
}
