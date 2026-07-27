//! Integration tests for `cargo-mvl-limit --emit-assurance-json` (spec
//! Requirement 14). Spawns the actual compiled binary (not just the
//! library function) so these genuinely exercise the CLI flag parsing,
//! not just the underlying `lints::check_source` call.

use mvl_rust_core::assurance::schema::AssuranceReport;
use std::path::{Path, PathBuf};
use std::process::Command;

fn write_fixture(name: &str, content: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("rust-limit-assurance-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

fn run_assurance_mode(path: &Path) -> AssuranceReport {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-mvl-limit"))
        .arg("--emit-assurance-json")
        .arg(path)
        .output()
        .expect("failed to spawn cargo-mvl-limit");

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
fn emits_valid_assurance_json_for_compliant_source() {
    let path = write_fixture("compliant.rs", "fn f() -> i32 { 1 }");
    let report = run_assurance_mode(&path);

    assert_eq!(report.version, "1.0");
    assert_eq!(report.target.crate_name, "rust-limit");
    let check = report.check.expect("check section must be populated");
    assert!(check.diagnostics.is_empty());
    assert!(check.obligations.is_empty());
    assert!(report.prove.is_none());
}

#[test]
fn emits_valid_assurance_json_with_diagnostics_for_violating_source() {
    let path = write_fixture("violating.rs", "fn f() { unsafe {} }");
    let report = run_assurance_mode(&path);

    let check = report.check.expect("check section must be populated");
    assert_eq!(check.diagnostics.len(), 1);
    assert_eq!(check.diagnostics[0].level, "error");
    assert!(check.diagnostics[0].message.contains("unsafe"));
    assert!(check.diagnostics[0]
        .provenance
        .ends_with("violating.rs:1:10"));
}

#[test]
fn assurance_mode_never_fails_the_build_even_with_multiple_violations() {
    let path = write_fixture(
        "many_violations.rs",
        "fn f() { unsafe {} } fn g() { unsafe {} }",
    );
    let report = run_assurance_mode(&path);
    let check = report.check.expect("check section must be populated");
    assert_eq!(check.diagnostics.len(), 2);
}

#[test]
fn assurance_mode_captures_a_read_error_as_a_diagnostic_instead_of_aborting() {
    let missing_path = std::env::temp_dir().join(format!(
        "rust-limit-assurance-test-{}-nonexistent.rs",
        std::process::id()
    ));
    let report = run_assurance_mode(&missing_path);

    let check = report.check.expect("check section must be populated");
    assert_eq!(check.diagnostics.len(), 1);
    assert_eq!(check.diagnostics[0].level, "error");
}
