use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use mvl_rust_core::assurance::report::{build_check_report, diagnostic_to_record};
use mvl_rust_core::assurance::schema::DiagnosticRecord;
use rust_total::checks;
use rust_total::checks::CheckSet;

fn main() -> ExitCode {
    let mut args: Vec<String> = env::args().skip(1).collect();

    // `cargo mvl-total <paths>` re-invokes this binary as
    // `cargo-mvl-total mvl-total <paths>` — drop the re-inserted subcommand
    // name so direct invocation and `cargo mvl-total` behave identically.
    if args.first().map(String::as_str) == Some("mvl-total") {
        args.remove(0);
    }

    let emit_verification_json = args.iter().any(|arg| arg == "--emit-verification-json");
    args.retain(|arg| arg != "--emit-verification-json");

    let check_flag = args.iter().find_map(|arg| arg.strip_prefix("--check="));
    let checks = match check_flag {
        Some(spec) => match CheckSet::parse(spec) {
            Ok(set) => set,
            Err(err) => {
                eprintln!("error: {err}");
                return ExitCode::from(2);
            }
        },
        None => CheckSet::ALL,
    };
    args.retain(|arg| !arg.starts_with("--check="));

    if args.is_empty() {
        eprintln!(
            "usage: cargo mvl-total [--emit-verification-json] [--check=panic,termination,swallow] <FILE>..."
        );
        return ExitCode::from(2);
    }

    if emit_verification_json {
        run_verification_mode(&args, checks)
    } else {
        run_gate_mode(&args, checks)
    }
}

fn run_gate_mode(args: &[String], checks: CheckSet) -> ExitCode {
    let mut had_violations = false;

    for arg in args {
        let path = PathBuf::from(arg);
        let source = match std::fs::read_to_string(&path) {
            Ok(source) => source,
            Err(err) => {
                eprintln!("error: failed to read {}: {err}", path.display());
                return ExitCode::from(2);
            }
        };

        let diagnostics = match checks::check_source_with(&source, checks) {
            Ok(diagnostics) => diagnostics,
            Err(err) => {
                eprintln!("error: {err}");
                return ExitCode::from(2);
            }
        };

        had_violations |= !diagnostics.is_empty();

        for diagnostic in &diagnostics {
            diagnostic.emit(&source, arg);
        }
    }

    if had_violations {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Assurance mode is a *view* of the same analysis Gate mode runs — not a
/// re-run, not a different code path — and never fails the build: even a
/// read/parse error is captured as a diagnostic in the emitted report
/// rather than aborting (spec Requirement 14's contract).
fn run_verification_mode(args: &[String], checks: CheckSet) -> ExitCode {
    let mut records = Vec::new();

    for arg in args {
        let path = PathBuf::from(arg);
        match std::fs::read_to_string(&path) {
            Ok(source) => match checks::check_source_with(&source, checks) {
                Ok(diagnostics) => {
                    records.extend(diagnostics.iter().map(|d| diagnostic_to_record(d, arg)));
                }
                Err(err) => records.push(error_record(arg, &err.to_string())),
            },
            Err(err) => records.push(error_record(arg, &err.to_string())),
        }
    }

    let report = build_check_report("rust-total", current_timestamp(), records);
    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("AssuranceReport always serializes")
    );
    ExitCode::SUCCESS
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
/// Known simplification, not a final design.
fn current_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string()
}
