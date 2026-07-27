//! `cargo mvl test`: runs `cargo test` and parses its output into the
//! assurance-JSON `TestSection` shape (spec Requirement 15).
//!
//! Parses the stable, plain-text libtest output (`test <name> ... ok`),
//! not `--format json` -- that flag is nightly-only (`-Z
//! unstable-options`), and this workspace stays on stable/MSRV
//! throughout. Per-test duration isn't available from stable plain-text
//! output without `--report-time` (also unstable), so every
//! `TestRecord.duration_ms` is `0` -- a known simplification, not a final
//! design (same category as the other tools' `current_timestamp`).

use std::process::Command;

use mvl_rust_core::assurance::schema::{TestRecord, TestSection, TestSummary};

/// Spawns `cargo test <extra_args>` in the current working directory and
/// parses its output.
pub fn run_cargo_test(extra_args: &[String]) -> Result<TestSection, String> {
    let output = Command::new("cargo")
        .arg("test")
        .args(extra_args)
        .output()
        .map_err(|err| format!("failed to spawn `cargo test`: {err}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_test_output(&stdout))
}

fn parse_test_output(stdout: &str) -> TestSection {
    let mut tests = Vec::new();
    let mut summary = TestSummary::default();

    for line in stdout.lines() {
        let Some(rest) = line.strip_prefix("test ") else {
            continue;
        };
        let Some((name, outcome)) = rest.rsplit_once(" ... ") else {
            continue;
        };
        let outcome_str = match outcome.trim() {
            "ok" => {
                summary.passed += 1;
                "passed"
            }
            "FAILED" => {
                summary.failed += 1;
                "failed"
            }
            "ignored" => {
                summary.ignored += 1;
                "ignored"
            }
            // Unrecognized shape (benchmarks, doctests with a different
            // suffix, ...) -- skip rather than guess.
            _ => continue,
        };
        tests.push(TestRecord {
            name: name.to_string(),
            outcome: outcome_str.to_string(),
            duration_ms: 0,
        });
    }

    TestSection { tests, summary }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_passed_failed_and_ignored_lines() {
        let stdout = "running 3 tests\n\
                       test foo::bar ... ok\n\
                       test foo::baz ... FAILED\n\
                       test foo::qux ... ignored\n\
                       \n\
                       test result: FAILED. 1 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s\n";
        let section = parse_test_output(stdout);
        assert_eq!(section.tests.len(), 3);
        assert_eq!(section.summary.passed, 1);
        assert_eq!(section.summary.failed, 1);
        assert_eq!(section.summary.ignored, 1);
        assert_eq!(section.tests[0].name, "foo::bar");
        assert_eq!(section.tests[0].outcome, "passed");
        assert_eq!(section.tests[1].outcome, "failed");
        assert_eq!(section.tests[2].outcome, "ignored");
    }

    #[test]
    fn ignores_the_summary_line_itself() {
        let stdout = "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
        let section = parse_test_output(stdout);
        assert!(section.tests.is_empty());
    }

    #[test]
    fn empty_output_yields_an_empty_section() {
        let section = parse_test_output("");
        assert!(section.tests.is_empty());
        assert_eq!(section.summary.passed, 0);
        assert_eq!(section.summary.failed, 0);
        assert_eq!(section.summary.ignored, 0);
    }
}
