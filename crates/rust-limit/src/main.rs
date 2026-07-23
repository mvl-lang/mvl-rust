use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use rust_limit::lints;

fn main() -> ExitCode {
    let mut args: Vec<String> = env::args().skip(1).collect();

    // `cargo mvl-limit <paths>` re-invokes this binary as
    // `cargo-mvl-limit mvl-limit <paths>` — drop the re-inserted subcommand
    // name so direct invocation and `cargo mvl-limit` behave identically.
    if args.first().map(String::as_str) == Some("mvl-limit") {
        args.remove(0);
    }

    if args.is_empty() {
        eprintln!("usage: cargo mvl-limit <FILE>...");
        return ExitCode::from(2);
    }

    let mut had_violations = false;

    for arg in &args {
        let path = PathBuf::from(arg);
        let source = match std::fs::read_to_string(&path) {
            Ok(source) => source,
            Err(err) => {
                eprintln!("error: failed to read {}: {err}", path.display());
                return ExitCode::from(2);
            }
        };

        let diagnostics = match lints::check_source(&source) {
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
