//! Integration tests for `cargo mvl prove`/`test`/`assurance` (spec
//! Requirement 15). Spawns the real `cargo-mvl` binary so these
//! genuinely exercise CLI argument parsing and process spawning
//! (`cargo mvl test` shells out to `cargo test` itself).

use mvl_rust_core::assurance::schema::AssuranceReport;
use std::path::PathBuf;
use std::process::Command;

fn write_fixture(name: &str, content: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("cargo-mvl-subcommands-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

fn run(args: &[&str]) -> (bool, AssuranceReport) {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-mvl"))
        .args(args)
        .output()
        .expect("failed to spawn cargo-mvl");

    let report = serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "output must deserialize as AssuranceReport: {err}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.success(), report)
}

#[test]
fn prove_emits_a_prove_section_with_no_check_or_test() {
    let path = write_fixture(
        "prove_compliant.rs",
        "#[mvl::requires(0 <= b && b <= 255)]\nfn f(b: i32) {}",
    );
    let (success, report) = run(&["prove", path.to_str().unwrap()]);

    assert!(success, "prove must always exit 0");
    let prove = report.prove.expect("prove section must be populated");
    assert_eq!(prove.obligations.len(), 1);
    assert!(report.check.is_none());
    assert!(report.test.is_none());
}

#[test]
fn prove_never_fails_on_a_missing_file() {
    let missing = std::env::temp_dir().join(format!(
        "cargo-mvl-subcommands-test-{}-nonexistent.rs",
        std::process::id()
    ));
    let (success, report) = run(&["prove", missing.to_str().unwrap()]);

    assert!(success, "prove must always exit 0, even for a missing file");
    let prove = report.prove.expect("prove section must be populated");
    assert!(prove.obligations.is_empty());
}

#[test]
fn assurance_aggregates_check_prove_and_test_sections() {
    let path = write_fixture(
        "assurance_violating.rs",
        "fn leak<T>(value: mvl::Tainted<T>) -> T { value.into_inner() }",
    );
    let (success, report) = run(&["assurance", path.to_str().unwrap()]);

    assert!(
        success,
        "assurance must always exit 0, even with violations"
    );
    let check = report.check.expect("check section must be populated");
    assert!(
        check
            .diagnostics
            .iter()
            .any(|d| d.message.contains("Tainted")),
        "expected an ifc diagnostic, got: {:?}",
        check.diagnostics
    );
    assert!(report.prove.is_some());
    assert!(report.test.is_some());
}
