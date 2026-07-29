//! Integration tests for `cargo-mvl-effect --emit-verification-json` (spec
//! Requirement 14). Spawns the actual compiled binary, not just the
//! library function, so these genuinely exercise the CLI flag parsing.

use mvl_rust_core::assurance::schema::AssuranceReport;
use std::path::{Path, PathBuf};
use std::process::Command;

fn write_fixture(name: &str, content: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rust-effect-verification-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

fn run_verification_mode(path: &Path) -> AssuranceReport {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-mvl-effect"))
        .arg("--emit-verification-json")
        .arg(path)
        .output()
        .expect("failed to spawn cargo-mvl-effect");

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
    let path = write_fixture(
        "compliant.rs",
        "#[mvl::effect(Console)] fn log(msg: &str) { println!(\"{msg}\"); } \
         #[mvl::effect(Console)] fn f() { log(\"hi\"); }",
    );
    let report = run_verification_mode(&path);

    assert_eq!(report.version, "1.0");
    assert_eq!(report.target.crate_name, "rust-effect");
    let check = report.check.expect("check section must be populated");
    assert!(check.diagnostics.is_empty());
    assert!(report.prove.is_none());
}

#[test]
fn emits_valid_verification_json_with_diagnostics_for_violating_source() {
    let path = write_fixture(
        "violating.rs",
        "#[mvl::effect(Console)] fn log(msg: &str) { println!(\"{msg}\"); } \
         fn f() { log(\"hi\"); }",
    );
    let report = run_verification_mode(&path);

    let check = report.check.expect("check section must be populated");
    assert_eq!(check.diagnostics.len(), 1);
    assert_eq!(check.diagnostics[0].level, "error");
    assert!(check.diagnostics[0].message.contains("Console"));
}

#[test]
fn verification_mode_captures_a_read_error_as_a_diagnostic_instead_of_aborting() {
    let missing_path = std::env::temp_dir().join(format!(
        "rust-effect-verification-test-{}-nonexistent.rs",
        std::process::id()
    ));
    let report = run_verification_mode(&missing_path);

    let check = report.check.expect("check section must be populated");
    assert_eq!(check.diagnostics.len(), 1);
    assert_eq!(check.diagnostics[0].level, "error");
}
