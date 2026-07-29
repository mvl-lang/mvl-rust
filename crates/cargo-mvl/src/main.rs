use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use cargo_mvl::check::{self, ToolOutcome};
use mvl_rust_core::assurance::report::{build_prove_report, diagnostic_to_record};
use mvl_rust_core::assurance::schema::{
    AssuranceReport, CheckSection, DiagnosticRecord, ProveSection, ProvenObligationRecord,
};
use mvl_rust_core::diagnostics::Level;

/// Assurance subcommands with no implementation yet -- both need
/// `cargo-llvm-cov` (an external tool with its own install/toolchain
/// story), tracked by its own ticket (#15) for that reason.
const UNIMPLEMENTED_SUBCOMMANDS: &[&str] = &["mcdc", "coverage"];

fn main() -> ExitCode {
    let mut args: Vec<String> = env::args().skip(1).collect();

    // `cargo mvl <subcommand> <paths>` re-invokes this binary as
    // `cargo-mvl mvl <subcommand> <paths>` — drop the re-inserted "mvl"
    // so direct invocation and `cargo mvl` behave identically.
    if args.first().map(String::as_str) == Some("mvl") {
        args.remove(0);
    }

    let Some(subcommand) = args.first().cloned() else {
        print_usage();
        return ExitCode::from(2);
    };
    let rest = &args[1..];
    let files: Vec<PathBuf> = rest.iter().map(PathBuf::from).collect();

    match subcommand.as_str() {
        "check" => run_check(&files),
        "prove" => run_prove(&files),
        "test" => run_test(rest),
        "assurance" => run_assurance(&files),
        _ if check::TOOL_ORDER.contains(&subcommand.as_str()) => run_single(&subcommand, &files),
        _ if UNIMPLEMENTED_SUBCOMMANDS.contains(&subcommand.as_str()) => {
            eprintln!(
                "cargo mvl {subcommand}: not yet implemented -- tracked by #15 (needs cargo-llvm-cov)"
            );
            ExitCode::from(2)
        }
        other => {
            eprintln!("cargo mvl: unknown subcommand `{other}`");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn print_usage() {
    eprintln!("usage: cargo mvl <SUBCOMMAND> <FILE>...");
    eprintln!();
    eprintln!("Gate subcommands:");
    eprintln!("  check              run every tool");
    eprintln!("  limit|total|refine|effect|ifc   run a single tool");
    eprintln!();
    eprintln!("Assurance subcommands (emit assurance-JSON to stdout):");
    eprintln!("  prove <FILE>...    rust-refine's obligation trace");
    eprintln!("  test [-- ARGS]     runs `cargo test`, parses pass/fail/ignored");
    eprintln!("  assurance <FILE>...   aggregates check + prove + test");
    eprintln!("  mcdc|coverage      not yet implemented -- see #15 (needs cargo-llvm-cov)");
}

fn read_source(path: &PathBuf) -> Result<String, ExitCode> {
    std::fs::read_to_string(path).map_err(|err| {
        eprintln!("error: failed to read {}: {err}", path.display());
        ExitCode::from(2)
    })
}

fn error_record(origin: &str, message: &str) -> DiagnosticRecord {
    DiagnosticRecord {
        level: "error".to_string(),
        message: message.to_string(),
        provenance: origin.to_string(),
        label: None,
        suggestion: None,
    }
}

/// Epoch seconds as a plain string — a real RFC 3339 timestamp would need
/// a datetime-formatting crate this workspace doesn't otherwise depend on.
/// Known simplification, not a final design (same as every tool's own
/// `current_timestamp`).
fn current_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string()
}

fn run_check(files: &[PathBuf]) -> ExitCode {
    if files.is_empty() {
        print_usage();
        return ExitCode::from(2);
    }

    let mut had_diagnostics = false;

    for path in files {
        let source = match read_source(path) {
            Ok(source) => source,
            Err(code) => return code,
        };

        for result in check::check_source(&source) {
            match result.outcome {
                ToolOutcome::Ran(diagnostics) => {
                    if !diagnostics.is_empty() {
                        // Only an actual `Level::Error` fails the build --
                        // some tools (`rust-refine`) also report
                        // informational `Level::Note`s (e.g. "discharged
                        // at L2") that shouldn't.
                        had_diagnostics |= diagnostics.iter().any(|d| d.level == Level::Error);
                        eprintln!("--- {} ---", result.tool);
                        for diagnostic in &diagnostics {
                            diagnostic.emit(&source, &path.display().to_string());
                        }
                    }
                }
                ToolOutcome::Error(message) => {
                    eprintln!(
                        "error: {} failed on {}: {message}",
                        result.tool,
                        path.display()
                    );
                    return ExitCode::from(2);
                }
            }
        }
    }

    if had_diagnostics {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn run_single(tool: &str, files: &[PathBuf]) -> ExitCode {
    if files.is_empty() {
        print_usage();
        return ExitCode::from(2);
    }

    let mut had_diagnostics = false;

    for path in files {
        let source = match read_source(path) {
            Ok(source) => source,
            Err(code) => return code,
        };

        // TOOL_ORDER already validated this tool name via the caller's
        // `contains` check, so `check_single` always returns `Some` here.
        let result = check::check_single(tool, &source).expect("tool already validated");

        match result.outcome {
            ToolOutcome::Ran(diagnostics) => {
                had_diagnostics |= diagnostics.iter().any(|d| d.level == Level::Error);
                for diagnostic in &diagnostics {
                    diagnostic.emit(&source, &path.display().to_string());
                }
            }
            ToolOutcome::Error(message) => {
                eprintln!("error: {message}");
                return ExitCode::from(2);
            }
        }
    }

    if had_diagnostics {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Assurance mode never fails the build: a file that can't be read or
/// parsed is skipped (with a warning) rather than aborting the whole
/// report, mirroring every tool's own `--emit-verification-json` contract.
/// `ProveSection` has no `diagnostics` field to record the skip in (only
/// `obligations`), so the skip is only visible on stderr, not in the
/// emitted JSON.
fn run_prove(files: &[PathBuf]) -> ExitCode {
    if files.is_empty() {
        print_usage();
        return ExitCode::from(2);
    }

    let mut obligations = Vec::new();
    for path in files {
        let origin = path.display().to_string();
        match std::fs::read_to_string(path) {
            Ok(source) => match cargo_mvl::prove::prove_source(&origin, &source) {
                Ok(found) => obligations.extend(found),
                Err(err) => eprintln!("warning: {origin}: {err}"),
            },
            Err(err) => eprintln!("warning: failed to read {origin}: {err}"),
        }
    }

    let report = build_prove_report("cargo-mvl", current_timestamp(), &obligations);
    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("AssuranceReport always serializes")
    );
    ExitCode::SUCCESS
}

/// Failing to spawn `cargo test` at all (e.g. `cargo` not on `PATH`) is
/// an environment error and exits non-zero; an actual test failure is
/// captured in the emitted `TestSection` (`summary.failed > 0`) and still
/// exits 0, matching assurance mode's "never fails the build on findings"
/// contract.
fn run_test(extra_args: &[String]) -> ExitCode {
    match cargo_mvl::test::run_cargo_test(extra_args) {
        Ok(section) => {
            let mut report = AssuranceReport::new("cargo-mvl", current_timestamp());
            report.test = Some(section);
            println!(
                "{}",
                serde_json::to_string_pretty(&report).expect("AssuranceReport always serializes")
            );
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::from(2)
        }
    }
}

/// Aggregates `check` (every Gate-mode tool's diagnostics), `prove`
/// (`rust-refine`'s obligations), and `test` (`cargo test`) into one
/// report. The top-level `assurance` section (`claim`/`argument_tree`) is
/// left absent -- its semantics are still unresolved (#22), not something
/// to invent here.
fn run_assurance(files: &[PathBuf]) -> ExitCode {
    if files.is_empty() {
        print_usage();
        return ExitCode::from(2);
    }

    let mut check_diagnostics = Vec::new();
    let mut obligations = Vec::new();

    for path in files {
        let origin = path.display().to_string();
        match std::fs::read_to_string(path) {
            Ok(source) => {
                for result in check::check_source(&source) {
                    match result.outcome {
                        ToolOutcome::Ran(diagnostics) => {
                            check_diagnostics.extend(
                                diagnostics.iter().map(|d| diagnostic_to_record(d, &origin)),
                            );
                        }
                        ToolOutcome::Error(message) => {
                            check_diagnostics.push(error_record(
                                &origin,
                                &format!("{}: {message}", result.tool),
                            ));
                        }
                    }
                }
                match cargo_mvl::prove::prove_source(&origin, &source) {
                    Ok(found) => obligations.extend(found),
                    Err(err) => check_diagnostics
                        .push(error_record(&origin, &format!("rust-refine: {err}"))),
                }
            }
            Err(err) => {
                check_diagnostics.push(error_record(&origin, &format!("failed to read: {err}")));
            }
        }
    }

    let test_section = match cargo_mvl::test::run_cargo_test(&[]) {
        Ok(section) => Some(section),
        Err(message) => {
            check_diagnostics.push(error_record("cargo test", &message));
            None
        }
    };

    let mut report = AssuranceReport::new("cargo-mvl", current_timestamp());
    report.check = Some(CheckSection {
        obligations: vec![],
        diagnostics: check_diagnostics,
    });
    report.prove = Some(ProveSection {
        obligations: obligations
            .iter()
            .map(|(o, r)| ProvenObligationRecord::new(o, r))
            .collect(),
    });
    report.test = test_section;

    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("AssuranceReport always serializes")
    );
    ExitCode::SUCCESS
}
