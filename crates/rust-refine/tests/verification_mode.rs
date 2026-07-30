//! Integration tests for `cargo-mvl-refine --emit-verification-json` (spec
//! Requirement 14, extended to `rust-refine`'s `prove` section). Spawns
//! the actual compiled binary, not just the library function, so these
//! genuinely exercise the CLI flag parsing.

use mvl_rust_core::assurance::schema::AssuranceReport;
use mvl_rust_core::assurance::version::ASSURANCE_SCHEMA_VERSION;
use mvl_rust_core::solver::Layer;
use std::path::{Path, PathBuf};
use std::process::Command;

fn write_fixture(name: &str, content: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rust-refine-verification-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

fn run_verification_mode(path: &Path) -> AssuranceReport {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-mvl-refine"))
        .arg("--emit-verification-json")
        .arg(path)
        .output()
        .expect("failed to spawn cargo-mvl-refine");

    assert!(
        output.status.success(),
        "verification mode must always exit 0, even with violations found -- stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "verification-mode output must deserialize as AssuranceReport: {err}\nstdout: {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

#[test]
fn emits_valid_verification_json_for_source_with_no_obligations() {
    let path = write_fixture("no_obligations.rs", "fn f(x: i32) -> i32 { x }");
    let report = run_verification_mode(&path);

    // Against the const, not a literal: what this test cares about is that
    // the emitter stamps the version it was built with, not which version
    // that is. A literal here just breaks on every bump (it did on #56's).
    assert_eq!(report.version, ASSURANCE_SCHEMA_VERSION);
    assert_eq!(report.target.crate_name, "rust-refine");
    let prove = report.prove.expect("prove section must be populated");
    assert!(prove.obligations.is_empty());
    assert!(report.check.is_none());
}

#[test]
fn emits_a_proven_obligation_at_l2_for_a_satisfiable_interval_bound() {
    let path = write_fixture(
        "compliant.rs",
        "#[mvl::requires(0 <= b && b <= 255)]\nfn f(b: i32) -> i32 { b }",
    );
    let report = run_verification_mode(&path);

    let prove = report.prove.expect("prove section must be populated");
    assert_eq!(prove.obligations.len(), 1);
    assert_eq!(prove.obligations[0].id, "f::requires#0");
    assert_eq!(prove.obligations[0].layer, Some(Layer::L2));
    assert!(prove.obligations[0].counterexample.is_none());
}

#[test]
fn emits_a_violated_obligation_with_a_counterexample_for_a_contradiction() {
    let path = write_fixture(
        "violating.rs",
        "#[mvl::requires(x >= 10 && x < 5)]\nfn f(x: i32) {}",
    );
    let report = run_verification_mode(&path);

    let prove = report.prove.expect("prove section must be populated");
    assert_eq!(prove.obligations.len(), 1);
    assert!(prove.obligations[0].layer.is_none());
    assert!(prove.obligations[0].counterexample.is_some());
}

#[test]
fn emits_a_runtime_obligation_for_a_predicate_l1_l2_cannot_decompose() {
    let path = write_fixture(
        "runtime_fallback.rs",
        "#[mvl::requires(len(sections) == 51)]\nfn f(sections: i32) {}",
    );
    let report = run_verification_mode(&path);

    let prove = report.prove.expect("prove section must be populated");
    assert_eq!(prove.obligations.len(), 1);
    assert_eq!(prove.obligations[0].layer, Some(Layer::Runtime));
}

#[test]
fn verification_mode_reports_no_obligations_for_a_missing_file_instead_of_aborting() {
    let missing_path = std::env::temp_dir().join(format!(
        "rust-refine-verification-test-{}-nonexistent.rs",
        std::process::id()
    ));
    let report = run_verification_mode(&missing_path);

    let prove = report.prove.expect("prove section must be populated");
    assert!(prove.obligations.is_empty());
}
