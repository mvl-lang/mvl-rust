#![cfg(unix)]

use mvl_rust_core::solver::{
    DischargeResult, Layer, Obligation, ShellOutSolver, SolverBackend, SolverError,
};
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mvl-rust-core-solver-test-{}-{}",
        std::process::id(),
        name
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes an executable shell script standing in for the `mvl` binary: it
/// discards stdin (or captures it to `capture`, if given) and prints
/// `response_json` on stdout. `sync_all()`s before returning so the
/// subsequent spawn never races the write -- without it, spawning a
/// freshly-written executable can occasionally fail with ETXTBSY ("text
/// file busy") on some filesystems (observed in CI).
fn write_fake_solver(dir: &Path, response_json: &str, capture: Option<&Path>) -> PathBuf {
    let path = dir.join("fake-mvl.sh");
    let capture_line = match capture {
        Some(p) => format!("cat > '{}'\n", p.display()),
        None => "cat > /dev/null\n".to_string(),
    };
    let escaped = response_json.replace('\'', "'\\''");
    let script = format!("#!/bin/sh\n{capture_line}printf '%s' '{escaped}'\n");

    let mut file = fs::File::create(&path).unwrap();
    file.write_all(script.as_bytes()).unwrap();
    file.sync_all().unwrap();
    drop(file);

    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).unwrap();
    path
}

fn sample_obligation() -> Obligation {
    Obligation {
        id: "ob1".into(),
        predicate: "x >= 0".into(),
        provenance: "src/lib.rs:1:1".into(),
    }
}

/// Retries on ETXTBSY as a defense-in-depth backstop alongside the
/// `sync_all()` above -- belt and suspenders against the same race.
fn discharge_retrying_on_text_busy(
    solver: &ShellOutSolver,
    obligation: &Obligation,
) -> Result<DischargeResult, SolverError> {
    let mut last = None;
    for _ in 0..5 {
        match solver.discharge(obligation) {
            Err(SolverError::Spawn(e)) if e.raw_os_error() == Some(26) => {
                last = Some(SolverError::Spawn(e));
                std::thread::sleep(Duration::from_millis(20));
            }
            result => return result,
        }
    }
    Err(last.expect("loop runs at least once"))
}

#[test]
fn discharges_a_proven_obligation() {
    let dir = test_dir("proven");
    let path = write_fake_solver(&dir, r#"{"outcome":"proven","layer":"L2"}"#, None);
    let solver = ShellOutSolver::with_binary(path.to_str().unwrap());

    let result = discharge_retrying_on_text_busy(&solver, &sample_obligation()).unwrap();

    assert_eq!(result, DischargeResult::Proven { layer: Layer::L2 });
}

#[test]
fn discharges_a_violated_obligation_with_counterexample() {
    let dir = test_dir("violated");
    let path = write_fake_solver(
        &dir,
        r#"{"outcome":"violated","counterexample":"x = 5"}"#,
        None,
    );
    let solver = ShellOutSolver::with_binary(path.to_str().unwrap());

    let result = discharge_retrying_on_text_busy(&solver, &sample_obligation()).unwrap();

    assert_eq!(
        result,
        DischargeResult::Violated {
            counterexample: "x = 5".into()
        }
    );
}

#[test]
fn discharges_to_runtime_when_no_static_layer_closes_it() {
    let dir = test_dir("runtime");
    let path = write_fake_solver(&dir, r#"{"outcome":"runtime"}"#, None);
    let solver = ShellOutSolver::with_binary(path.to_str().unwrap());

    let result = discharge_retrying_on_text_busy(&solver, &sample_obligation()).unwrap();

    assert_eq!(result, DischargeResult::Runtime);
}

#[test]
fn writes_the_obligation_as_json_on_stdin() {
    let dir = test_dir("stdin-capture");
    let capture = dir.join("captured.json");
    let path = write_fake_solver(&dir, r#"{"outcome":"runtime"}"#, Some(&capture));
    let solver = ShellOutSolver::with_binary(path.to_str().unwrap());
    let obligation = sample_obligation();

    discharge_retrying_on_text_busy(&solver, &obligation).unwrap();

    let captured = fs::read_to_string(&capture).unwrap();
    let expected = serde_json::to_string(&obligation).unwrap();
    assert_eq!(captured, expected);
}

#[test]
fn missing_binary_yields_spawn_error() {
    let solver = ShellOutSolver::with_binary("/nonexistent/path/to/mvl-solver-binary");

    let err = discharge_retrying_on_text_busy(&solver, &sample_obligation()).unwrap_err();

    assert!(matches!(err, SolverError::Spawn(_)));
}

#[test]
fn non_json_output_yields_invalid_response_error() {
    let dir = test_dir("garbage");
    let path = write_fake_solver(&dir, "not valid json", None);
    let solver = ShellOutSolver::with_binary(path.to_str().unwrap());

    let err = discharge_retrying_on_text_busy(&solver, &sample_obligation()).unwrap_err();

    assert!(matches!(err, SolverError::InvalidResponse(_)));
}
