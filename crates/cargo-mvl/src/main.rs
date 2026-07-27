use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use cargo_mvl::check::{self, ToolOutcome};
use mvl_rust_core::diagnostics::Level;

const ASSURANCE_SUBCOMMANDS: &[&str] = &["prove", "test", "mcdc", "coverage", "assurance"];

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
    let files: Vec<PathBuf> = args[1..].iter().map(PathBuf::from).collect();

    match subcommand.as_str() {
        "check" => run_check(&files),
        _ if check::TOOL_ORDER.contains(&subcommand.as_str()) => run_single(&subcommand, &files),
        _ if ASSURANCE_SUBCOMMANDS.contains(&subcommand.as_str()) => {
            eprintln!(
                "cargo mvl {subcommand}: not yet implemented -- blocked on #13 (assurance-JSON schema) and #14 (per-tool assurance emission)"
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
    eprintln!("Assurance subcommands (not yet implemented -- see #13, #14):");
    eprintln!("  prove|test|mcdc|coverage|assurance");
}

fn read_source(path: &PathBuf) -> Result<String, ExitCode> {
    std::fs::read_to_string(path).map_err(|err| {
        eprintln!("error: failed to read {}: {err}", path.display());
        ExitCode::from(2)
    })
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
