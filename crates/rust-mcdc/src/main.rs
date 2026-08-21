use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use mvl_rust_core::assurance::schema::{AssuranceReport, McdcCondition, McdcSection};
use rust_mcdc::discharge::{self, DecisionOutcome};
use rust_mcdc::scanner::{self, Decision};

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();

    // `cargo mvl-mcdc <paths>` re-invokes this binary as
    // `cargo-mvl-mcdc mvl-mcdc <paths>` -- drop the re-inserted subcommand
    // name so direct invocation and `cargo mvl-mcdc` behave identically.
    if args.first().map(String::as_str) == Some("mvl-mcdc") {
        args.remove(0);
    }

    let Some(mode) = args.first().cloned() else {
        print_usage();
        return ExitCode::from(2);
    };

    match mode.as_str() {
        "scan" => run_scan(&args[1..]),
        "discharge" => run_discharge(&args[1..]),
        _ => {
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn print_usage() {
    eprintln!("usage: cargo mvl-mcdc scan <FILE>...");
    eprintln!("       cargo mvl-mcdc discharge [--run-dir=DIR] [--min-decisions=PCT] [--min-conditions=PCT] [--emit-mcdc-json] <FILE>...");
}

/// `scan`: obligation extraction only (layer "a") -- deterministic, never
/// touches the file, safe to run anywhere. Always exits 0; obligations are
/// reported, not gated (gating happens after `discharge`).
fn run_scan(args: &[String]) -> ExitCode {
    let files: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
    if files.is_empty() {
        print_usage();
        return ExitCode::from(2);
    }

    let mut obligations = Vec::new();
    for path in &files {
        let source = match std::fs::read_to_string(path) {
            Ok(source) => source,
            Err(err) => {
                eprintln!("error: failed to read {}: {err}", path.display());
                return ExitCode::from(2);
            }
        };
        let decisions = match scanner::scan_source(&source) {
            Ok(decisions) => decisions,
            Err(err) => {
                eprintln!("error: {}: {err}", path.display());
                return ExitCode::from(2);
            }
        };
        for decision in &decisions {
            obligations.push(obligation_json(path, decision));
        }
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&obligations).expect("obligations always serialize")
    );
    ExitCode::SUCCESS
}

fn obligation_json(path: &Path, decision: &Decision) -> serde_json::Value {
    serde_json::json!({
        "file": path.display().to_string(),
        "line": decision.site.start().line,
        "decision": decision.text,
        "conditions": decision.leaves.len(),
        "vectors_required": decision.vectors_required(),
        "compiler_void": decision.compiler_void,
    })
}

struct DischargeOptions {
    run_dir: PathBuf,
    min_decisions_pct: f64,
    min_conditions_pct: f64,
    emit_json: bool,
    files: Vec<PathBuf>,
}

fn parse_discharge_args(args: &[String]) -> Result<DischargeOptions, String> {
    let mut run_dir = PathBuf::from(".");
    let mut min_decisions_pct = 0.0;
    let mut min_conditions_pct = 0.0;
    let mut emit_json = false;
    let mut files = Vec::new();

    for arg in args {
        if let Some(value) = arg.strip_prefix("--run-dir=") {
            run_dir = PathBuf::from(value);
        } else if let Some(value) = arg.strip_prefix("--min-decisions=") {
            min_decisions_pct = value
                .parse()
                .map_err(|_| format!("invalid --min-decisions value: {value}"))?;
        } else if let Some(value) = arg.strip_prefix("--min-conditions=") {
            min_conditions_pct = value
                .parse()
                .map_err(|_| format!("invalid --min-conditions value: {value}"))?;
        } else if arg == "--emit-mcdc-json" {
            emit_json = true;
        } else {
            files.push(PathBuf::from(arg));
        }
    }

    if files.is_empty() {
        return Err("no files given".to_string());
    }

    Ok(DischargeOptions {
        run_dir,
        min_decisions_pct,
        min_conditions_pct,
        emit_json,
        files,
    })
}

/// `discharge`: mutation-tests every decision in every given file (layer
/// "c"), aggregates the results, and gates on `--min-decisions`/
/// `--min-conditions` (percentage thresholds, default 0 -- i.e. report
/// only, unless a threshold is explicitly requested).
fn run_discharge(args: &[String]) -> ExitCode {
    let options = match parse_discharge_args(args) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("error: {message}");
            print_usage();
            return ExitCode::from(2);
        }
    };

    let mut all_outcomes: Vec<(PathBuf, DecisionOutcome)> = Vec::new();
    for path in &options.files {
        let outcomes = match discharge::discharge_file(path, &options.run_dir) {
            Ok(outcomes) => outcomes,
            Err(err) => {
                eprintln!("error: {}: {err}", path.display());
                return ExitCode::from(2);
            }
        };
        all_outcomes.extend(outcomes.into_iter().map(|o| (path.clone(), o)));
    }

    let total_decisions = all_outcomes.len();
    let complete_decisions = all_outcomes.iter().filter(|(_, o)| o.discharged()).count();
    let total_conditions: usize = all_outcomes.iter().map(|(_, o)| o.mutants_total).sum();
    let killed_conditions: usize = all_outcomes.iter().map(|(_, o)| o.mutants_killed).sum();

    let decisions_pct = percentage(complete_decisions, total_decisions);
    let conditions_pct = percentage(killed_conditions, total_conditions);

    if options.emit_json {
        print_report(&all_outcomes, killed_conditions, total_conditions);
    } else {
        print_summary(&all_outcomes, total_decisions, complete_decisions, decisions_pct, conditions_pct);
    }

    if decisions_pct < options.min_decisions_pct || conditions_pct < options.min_conditions_pct {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn percentage(part: usize, total: usize) -> f64 {
    if total == 0 {
        100.0
    } else {
        (part as f64 / total as f64) * 100.0
    }
}

fn print_summary(
    outcomes: &[(PathBuf, DecisionOutcome)],
    total_decisions: usize,
    complete_decisions: usize,
    decisions_pct: f64,
    conditions_pct: f64,
) {
    println!("MC/DC discharge summary");
    println!("  decisions:  {complete_decisions}/{total_decisions} complete ({decisions_pct:.1}%)");
    println!("  conditions: {conditions_pct:.1}% mutants killed");
    for (path, outcome) in outcomes {
        if !outcome.discharged() {
            println!(
                "  undischarged: {}:{} `{}` ({}/{} mutants killed)",
                path.display(),
                outcome.line,
                outcome.decision,
                outcome.mutants_killed,
                outcome.mutants_total
            );
            for description in &outcome.survived_descriptions {
                println!("    survived: {description}");
            }
        }
    }
}

fn print_report(
    outcomes: &[(PathBuf, DecisionOutcome)],
    killed_conditions: usize,
    total_conditions: usize,
) {
    let conditions = outcomes
        .iter()
        .map(|(path, outcome)| McdcCondition {
            id: format!("{}:{}", path.display(), outcome.line),
            covered: outcome.discharged(),
        })
        .collect();

    let mut report = AssuranceReport::new("cargo-mvl-mcdc", current_timestamp());
    report.mcdc = Some(McdcSection {
        conditions,
        coverage_pct: percentage(killed_conditions, total_conditions),
    });

    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("AssuranceReport always serializes")
    );
}

fn current_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string()
}
