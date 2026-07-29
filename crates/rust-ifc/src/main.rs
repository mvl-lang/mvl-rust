use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use mvl_rust_core::assurance::report::{build_check_report, diagnostic_to_record};
use mvl_rust_core::assurance::schema::DiagnosticRecord;
use rust_ifc::checks;

fn main() -> ExitCode {
    let mut args: Vec<String> = env::args().skip(1).collect();

    // `cargo mvl-ifc <paths>` re-invokes this binary as
    // `cargo-mvl-ifc mvl-ifc <paths>` — drop the re-inserted subcommand
    // name so direct invocation and `cargo mvl-ifc` behave identically.
    if args.first().map(String::as_str) == Some("mvl-ifc") {
        args.remove(0);
    }

    let emit_verification_json = args.iter().any(|arg| arg == "--emit-verification-json");
    args.retain(|arg| arg != "--emit-verification-json");

    if args.is_empty() {
        eprintln!("usage: cargo mvl-ifc [--emit-verification-json] <FILE>...");
        return ExitCode::from(2);
    }

    if emit_verification_json {
        run_verification_mode(&args)
    } else {
        run_gate_mode(&args)
    }
}

fn run_gate_mode(args: &[String]) -> ExitCode {
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

        let diagnostics = match checks::check_source(&source) {
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
fn run_verification_mode(args: &[String]) -> ExitCode {
    let mut records = Vec::new();

    for arg in args {
        let path = PathBuf::from(arg);
        match std::fs::read_to_string(&path) {
            Ok(source) => match checks::check_source(&source) {
                Ok(diagnostics) => {
                    records.extend(diagnostics.iter().map(|d| diagnostic_to_record(d, arg)));
                }
                Err(err) => records.push(error_record(arg, &err.to_string())),
            },
            Err(err) => records.push(error_record(arg, &err.to_string())),
        }
    }

    let report = build_check_report("rust-ifc", current_timestamp(), records);
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
/// Known simplification, not a final design (same as the other tools'
/// `current_timestamp`).
fn current_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string()
}
