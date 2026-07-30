//! Integration tests for `cargo-mvl-total --emit-verification-json` (spec
//! Requirement 14). Spawns the actual compiled binary (not just the
//! library function) so these genuinely exercise the CLI flag parsing,
//! not just the underlying `checks::check_source` call.

use mvl_rust_core::assurance::schema::AssuranceReport;
use mvl_rust_core::assurance::version::ASSURANCE_SCHEMA_VERSION;
use std::path::{Path, PathBuf};
use std::process::Command;

fn write_fixture(name: &str, content: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rust-total-verification-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

fn run_verification_mode(path: &Path) -> AssuranceReport {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-mvl-total"))
        .arg("--emit-verification-json")
        .arg(path)
        .output()
        .expect("failed to spawn cargo-mvl-total");

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
fn emits_valid_verification_json_for_compliant_source() {
    let path = write_fixture("compliant.rs", "#[mvl::total]\nfn f(x: i32) -> i32 { x }");
    let report = run_verification_mode(&path);

    // Against the const, not a literal: this asserts the emitter stamps the
    // version it was built with, not which version that is (#56).
    assert_eq!(report.version, ASSURANCE_SCHEMA_VERSION);
    assert_eq!(report.target.crate_name, "rust-total");
    let check = report.check.expect("check section must be populated");
    assert!(check.diagnostics.is_empty());
    assert!(check.obligations.is_empty());
    assert!(report.prove.is_none());
}

#[test]
fn emits_valid_verification_json_with_diagnostics_for_violating_source() {
    let path = write_fixture(
        "violating.rs",
        "#[mvl::total]\nfn f(x: Option<i32>) -> i32 { x.unwrap() }",
    );
    let report = run_verification_mode(&path);

    let check = report.check.expect("check section must be populated");
    assert_eq!(check.diagnostics.len(), 1);
    assert_eq!(check.diagnostics[0].level, "error");
    assert!(check.diagnostics[0].message.contains("unwrap"));
}

#[test]
fn verification_mode_never_fails_the_build_even_with_multiple_violations() {
    let path = write_fixture(
        "many_violations.rs",
        "#[mvl::total]\nfn f(x: Option<i32>) -> i32 { x.unwrap() }\n\
         #[mvl::total]\nfn g(a: i32, b: i32) -> i32 { a / b }",
    );
    let report = run_verification_mode(&path);
    let check = report.check.expect("check section must be populated");
    assert_eq!(check.diagnostics.len(), 2);
}

#[test]
fn verification_mode_captures_a_read_error_as_a_diagnostic_instead_of_aborting() {
    let missing_path = std::env::temp_dir().join(format!(
        "rust-total-verification-test-{}-nonexistent.rs",
        std::process::id()
    ));
    let report = run_verification_mode(&missing_path);

    let check = report.check.expect("check section must be populated");
    assert_eq!(check.diagnostics.len(), 1);
    assert_eq!(check.diagnostics[0].level, "error");
}
