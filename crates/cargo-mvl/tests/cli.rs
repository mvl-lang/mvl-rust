//! Integration tests for `cargo mvl`'s argument handling (#124): the
//! success summary line, `-q`, per-subcommand `--help`, and rejecting
//! unknown options instead of reading them as file paths.

use std::path::PathBuf;
use std::process::{Command, Output};

fn write_fixture(name: &str, content: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cargo-mvl-cli-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

fn compliant_fixture() -> PathBuf {
    write_fixture("compliant.rs", "#[mvl::total]\nfn f() {}\n")
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cargo-mvl"))
        .args(args)
        .output()
        .expect("failed to spawn cargo-mvl")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn check_prints_a_summary_line_on_success() {
    let path = compliant_fixture();
    let path = path.to_str().unwrap();
    let output = run(&["check", path, path]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(
        stderr(&output),
        "mvl check: 2 files · limit total refine effect ifc · ok\n"
    );
    assert!(output.stdout.is_empty());
}

#[test]
fn a_single_tool_prints_its_own_summary_line() {
    let path = compliant_fixture();
    let output = run(&["limit", path.to_str().unwrap()]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(stderr(&output), "mvl limit: 1 file · ok\n");
}

#[test]
fn quiet_suppresses_the_summary_line() {
    let path = compliant_fixture();
    for args in [["check", "-q"], ["check", "--quiet"], ["ifc", "-q"]] {
        let output = run(&[args[0], args[1], path.to_str().unwrap()]);
        assert!(output.status.success(), "{args:?}: {}", stderr(&output));
        assert!(output.stderr.is_empty(), "{args:?}: {}", stderr(&output));
    }
}

#[test]
fn a_failing_check_prints_diagnostics_and_no_summary_line() {
    let path = write_fixture("violating.rs", "#[mvl::total]\nfn f() { unsafe {} }\n");
    let output = run(&["check", path.to_str().unwrap()]);

    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr(&output);
    assert!(stderr.contains("--- limit ---"), "stderr: {stderr}");
    assert!(!stderr.contains("· ok"), "stderr: {stderr}");
}

#[test]
fn every_subcommand_prints_its_usage_on_help() {
    let subcommands = [
        "check",
        "limit",
        "total",
        "refine",
        "effect",
        "ifc",
        "prove",
        "test",
        "assurance",
        "mcdc",
    ];
    for subcommand in subcommands {
        for flag in ["-h", "--help"] {
            let output = run(&[subcommand, flag]);
            assert!(
                output.status.success(),
                "`{subcommand} {flag}` must exit 0, stderr: {}",
                stderr(&output)
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                stdout.starts_with(&format!("usage: cargo mvl {subcommand}")),
                "`{subcommand} {flag}` stdout: {stdout}"
            );
        }
    }
}

#[test]
fn top_level_help_prints_usage_and_exits_zero() {
    let output = run(&["--help"]);

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("usage: cargo mvl <SUBCOMMAND>"));
}

#[test]
fn an_unknown_option_is_rejected_not_read_as_a_file() {
    let path = compliant_fixture();
    for args in [
        ["check", "--bogus"],
        ["limit", "-x"],
        ["prove", "-q"],
        ["assurance", "--quiet"],
        ["mcdc", "--bogus"],
    ] {
        let output = run(&[args[0], args[1], path.to_str().unwrap()]);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let stderr = stderr(&output);
        assert!(
            stderr.contains(&format!("unknown option `{}`", args[1])),
            "{args:?}: {stderr}"
        );
        assert!(!stderr.contains("failed to read"), "{args:?}: {stderr}");
        assert!(
            output.stdout.is_empty(),
            "{args:?}: no JSON on a usage error"
        );
    }
}
