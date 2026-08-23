use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use mvl_rust_core::assurance::report::build_prove_report;
use mvl_rust_core::diagnostics::Level;
use mvl_rust_core::solver::Obligation;
use rust_refine::checks;

/// **ADR-0011's resolved-pure closure licence assumes `rust-limit` and
/// `rust-effect` already gated this file** (`cargo mvl check`'s fixed
/// order: `limit → total → refine → effect → ifc`). Standalone invocation
/// of this binary on source that hasn't itself passed both is not covered
/// by that licence and must not be trusted to apply it soundly — see
/// `rust_refine::checks`'s module doc.
fn main() -> ExitCode {
    let mut args: Vec<String> = env::args().skip(1).collect();

    // `cargo mvl-refine <paths>` re-invokes this binary as
    // `cargo-mvl-refine mvl-refine <paths>` — drop the re-inserted
    // subcommand name so direct invocation and `cargo mvl-refine` behave
    // identically.
    if args.first().map(String::as_str) == Some("mvl-refine") {
        args.remove(0);
    }

    let emit_verification_json = args.iter().any(|arg| arg == "--emit-verification-json");
    args.retain(|arg| arg != "--emit-verification-json");

    if args.is_empty() {
        eprintln!("usage: cargo mvl-refine [--emit-verification-json] <FILE>...");
        return ExitCode::from(2);
    }

    if emit_verification_json {
        run_verification_mode(&args)
    } else {
        run_gate_mode(&args)
    }
}

/// Unlike `rust-limit`/`rust-total` (which only ever emit `Level::Error`
/// diagnostics), `rust-refine` reports every obligation's outcome,
/// including informational `Level::Note`s for `Proven`/`Runtime` (spec
/// Requirement 3's "report which layer discharged it" UX) — so Gate mode
/// only fails the build on an actual `Level::Error` (a genuine
/// violation), not merely because diagnostics were emitted at all.
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

        had_violations |= diagnostics.iter().any(|d| d.level == Level::Error);

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

/// Assurance mode is a *view* of the same obligations Gate mode
/// discharges — not a re-run, not a different code path — and never fails
/// the build: even a read/parse error surfaces as an empty obligation
/// list in the emitted report rather than aborting (mirrors spec
/// Requirement 14's contract for `rust-limit`/`rust-total`).
fn run_verification_mode(args: &[String]) -> ExitCode {
    let mut results = Vec::new();

    for arg in args {
        let path = PathBuf::from(arg);
        let source = match std::fs::read_to_string(&path) {
            Ok(source) => source,
            Err(err) => {
                eprintln!("error: failed to read {}: {err}", path.display());
                continue;
            }
        };

        match checks::find_obligations(&source) {
            Ok(found) => {
                for f in &found {
                    let discharge_result = f.discharge();
                    let warrant = f.warrant();
                    let start = f.span.start();
                    let obligation = Obligation {
                        id: f.id(),
                        predicate: f.predicate_text(),
                        provenance: format!("{arg}:{}:{}", start.line, start.column + 1),
                        kind: f.class(),
                    };
                    results.push((obligation, discharge_result, warrant));
                }
            }
            Err(err) => {
                eprintln!("error: {err}");
            }
        }
    }

    let report = build_prove_report("rust-refine", current_timestamp(), &results);
    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("AssuranceReport always serializes")
    );
    ExitCode::SUCCESS
}

/// Epoch seconds as a plain string — a real RFC 3339 timestamp would need
/// a datetime-formatting crate this workspace doesn't otherwise depend on.
/// Known simplification, not a final design (same as `rust-limit`/
/// `rust-total`'s own `current_timestamp`).
fn current_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string()
}
