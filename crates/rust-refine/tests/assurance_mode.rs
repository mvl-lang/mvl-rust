//! Integration tests for `cargo-mvl-refine --emit-assurance-json` (spec
//! Requirement 14, extended to `rust-refine`'s `prove` section). Spawns
//! the actual compiled binary, not just the library function, so these
//! genuinely exercise the CLI flag parsing.

use mvl_rust_core::assurance::schema::AssuranceReport;
use mvl_rust_core::solver::Layer;
use std::path::{Path, PathBuf};
use std::process::Command;

fn write_fixture(name: &str, content: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("rust-refine-assurance-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

fn run_assurance_mode(path: &Path) -> AssuranceReport {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-mvl-refine"))
        .arg("--emit-assurance-json")
        .arg(path)
        .output()
        .expect("failed to spawn cargo-mvl-refine");

    assert!(
        output.status.success(),
        "assurance mode must always exit 0, even with violations found -- stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "assurance-mode output must deserialize as AssuranceReport: {err}\nstdout: {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

#[test]
fn emits_valid_assurance_json_for_source_with_no_obligations() {
    let path = write_fixture("no_obligations.rs", "fn f(x: i32) -> i32 { x }");
    let report = run_assurance_mode(&path);

    assert_eq!(report.version, "1.0");
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
    let report = run_assurance_mode(&path);

    let prove = report.prove.expect("prove section must be populated");
    assert_eq!(prove.obligations.len(), 1);
    assert_eq!(prove.obligations[0].id, "f::requires");
    assert_eq!(prove.obligations[0].layer, Some(Layer::L2));
    assert!(prove.obligations[0].counterexample.is_none());
}

#[test]
fn emits_a_violated_obligation_with_a_counterexample_for_a_contradiction() {
    let path = write_fixture(
        "violating.rs",
        "#[mvl::requires(x >= 10 && x < 5)]\nfn f(x: i32) {}",
    );
    let report = run_assurance_mode(&path);

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
    let report = run_assurance_mode(&path);

    let prove = report.prove.expect("prove section must be populated");
    assert_eq!(prove.obligations.len(), 1);
    assert_eq!(prove.obligations[0].layer, Some(Layer::Runtime));
}

#[test]
fn assurance_mode_reports_no_obligations_for_a_missing_file_instead_of_aborting() {
    let missing_path = std::env::temp_dir().join(format!(
        "rust-refine-assurance-test-{}-nonexistent.rs",
        std::process::id()
    ));
    let report = run_assurance_mode(&missing_path);

    let prove = report.prove.expect("prove section must be populated");
    assert!(prove.obligations.is_empty());
}
