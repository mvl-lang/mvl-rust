//! Integration tests for `cargo mvl prove`/`test`/`assurance` (spec
//! Requirement 15). Spawns the real `cargo-mvl` binary so these
//! genuinely exercise CLI argument parsing and process spawning
//! (`cargo mvl test` shells out to `cargo test` itself).
//!
//! Anything that reaches `cargo test` runs inside a throwaway fixture
//! crate: run from this package's directory it would re-run this very
//! file, which spawns `cargo test` again, without end (#126).

use mvl_rust_core::assurance::schema::AssuranceReport;
use std::path::{Path, PathBuf};
use std::process::Command;

fn write_fixture(name: &str, content: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("cargo-mvl-subcommands-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

/// A standalone crate (its own `[workspace]`) whose only content is
/// `lib_rs`, for `cargo mvl test`/`assurance` to run `cargo test` in.
fn write_fixture_crate(name: &str, lib_rs: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join(format!("cargo-mvl-subcommands-test-{}", std::process::id()))
        .join(name);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[workspace]\n"),
    )
    .unwrap();
    std::fs::write(dir.join("src/lib.rs"), lib_rs).unwrap();
    dir
}

fn run(args: &[&str]) -> (bool, AssuranceReport) {
    run_in(&std::env::temp_dir(), args)
}

fn run_in(dir: &Path, args: &[&str]) -> (bool, AssuranceReport) {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-mvl"))
        .args(args)
        .current_dir(dir)
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
    let krate = write_fixture_crate("assurance_fixture", "#[test]\nfn passes() {}\n");
    let (success, report) = run_in(&krate, &["assurance", path.to_str().unwrap()]);

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
    let test = report.test.expect("test section must be populated");
    assert_eq!(test.summary.passed, 1);
    assert_eq!(test.summary.failed, 0);
}

#[test]
fn test_reports_failing_tests_in_the_section_and_still_exits_zero() {
    let krate = write_fixture_crate(
        "test_fixture",
        "#[test]\nfn passes() {}\n\n#[test]\nfn fails() { panic!(\"expected\") }\n",
    );
    let (success, report) = run_in(&krate, &["test"]);

    assert!(success, "a failing test is a finding, not a tool error");
    let test = report.test.expect("test section must be populated");
    assert_eq!(test.summary.passed, 1);
    assert_eq!(test.summary.failed, 1);
}

#[test]
fn test_reports_a_build_failure_as_an_error_not_an_empty_section() {
    let krate = write_fixture_crate("broken_fixture", "fn broken( {}\n");
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-mvl"))
        .arg("test")
        .current_dir(&krate)
        .output()
        .expect("failed to spawn cargo-mvl");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty(), "no report on a build failure");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("without running any tests"),
        "stderr: {stderr}"
    );
}

#[test]
fn mcdc_redirects_standalone_subcommands_instead_of_misreading_them_as_files() {
    for keyword in ["scan", "discharge", "harvest", "generate"] {
        let output = Command::new(env!("CARGO_BIN_EXE_cargo-mvl"))
            .args(["mcdc", keyword, "src/lib.rs"])
            .output()
            .expect("failed to spawn cargo-mvl");

        assert!(
            !output.status.success(),
            "`cargo mvl mcdc {keyword} ...` must not succeed"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("cargo-mvl-mcdc"),
            "expected a redirect to the standalone binary, got: {stderr}"
        );
        assert!(
            !stderr.contains("failed to read"),
            "must not be misread as a file path, got: {stderr}"
        );
    }
}

#[test]
fn mcdc_scan_reports_compiler_void_not_discharge() {
    let path = write_fixture(
        "mcdc_scan.rs",
        "fn f(x: i32) -> i32 { match x { n if n > 0 => n, _ => 0 } }",
    );
    let (success, report) = run(&["mcdc", path.to_str().unwrap()]);

    assert!(success);
    let mcdc = report.mcdc.expect("mcdc section must be populated");
    // One compiler-void `match`, one real (uncovered) guard decision.
    assert_eq!(mcdc.conditions.len(), 2);
    assert_eq!(mcdc.conditions.iter().filter(|c| c.covered).count(), 1);
}
