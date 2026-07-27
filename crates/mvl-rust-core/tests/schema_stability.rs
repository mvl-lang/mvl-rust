//! Two things, per issue #13's acceptance criteria:
//!
//! 1. The committed `schemas/assurance-v1.0.json` must match what
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
    DiagnosticRecord, McdcCondition, McdcSection, ProveSection, ProvenObligationRecord, TestRecord,
    TestSection, TestSummary,
};
use mvl_rust_core::solver::{DischargeResult, Layer, Obligation};
use std::path::Path;

const COMMITTED_SCHEMA_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/schemas/assurance-v1.0.json");

#[test]
fn committed_schema_matches_the_derived_schema() {
    let derived = schemars::schema_for!(AssuranceReport);
    let derived_value = serde_json::to_value(&derived).unwrap();

    let committed_text = std::fs::read_to_string(Path::new(COMMITTED_SCHEMA_PATH))
        .unwrap_or_else(|err| panic!("failed to read {COMMITTED_SCHEMA_PATH}: {err}"));
    let committed_value: serde_json::Value = serde_json::from_str(&committed_text).unwrap();

    assert_eq!(
        derived_value, committed_value,
        "schemas/assurance-v1.0.json is out of date with AssuranceReport's current shape.\n\
         If this shape change is deliberate: regenerate the committed file and bump \
         ASSURANCE_SCHEMA_VERSION in assurance/version.rs."
    );
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
    };
    let result = DischargeResult::Proven { layer: Layer::L2 };
    report.prove = Some(ProveSection {
        obligations: vec![ProvenObligationRecord::new(&obligation, &result)],
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
