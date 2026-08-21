use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use mvl_rust_core::assurance::schema::{AssuranceReport, McdcCondition, McdcSection};
use rust_mcdc::discharge::{self, DecisionOutcome};
use rust_mcdc::harvest;
use rust_mcdc::obligation::ObligationRecord;
use rust_mcdc::scanner;

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
        "harvest" => run_harvest(&args[1..]),
        "generate" => run_generate(&args[1..]),
        _ => {
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn print_usage() {
    eprintln!("usage: cargo mvl-mcdc scan [-o FILE] <FILE>...");
    eprintln!("       cargo mvl-mcdc discharge [--run-dir=DIR] [--min-decisions=PCT] [--min-conditions=PCT] [--emit-mcdc-json] <FILE>...");
    eprintln!("       cargo mvl-mcdc harvest --obligations=FILE [--run-dir=DIR] [--min-decisions=PCT] [--emit-mcdc-json]");
    eprintln!("       cargo mvl-mcdc generate --obligations=FILE");
}

fn write_output(content: &str, output: Option<&Path>) -> ExitCode {
    match output {
        Some(path) => {
            if let Some(parent) = path.parent() {
                if let Err(err) = std::fs::create_dir_all(parent) {
                    eprintln!("error: failed to create {}: {err}", parent.display());
                    return ExitCode::from(2);
                }
            }
            if let Err(err) = std::fs::write(path, content) {
                eprintln!("error: failed to write {}: {err}", path.display());
                return ExitCode::from(2);
            }
        }
        None => println!("{content}"),
    }
    ExitCode::SUCCESS
}

/// `scan`: obligation extraction only (layer "a") -- deterministic, never
/// touches the file, safe to run anywhere. Always exits 0; obligations are
/// reported, not gated (gating happens after `discharge`/`harvest`).
fn run_scan(args: &[String]) -> ExitCode {
    let mut output = None;
    let mut files = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "-o" || arg == "--output" {
            match iter.next() {
                Some(path) => output = Some(PathBuf::from(path)),
                None => {
                    eprintln!("error: {arg} requires a path argument");
                    return ExitCode::from(2);
                }
            }
        } else if let Some(value) = arg.strip_prefix("-o=") {
            output = Some(PathBuf::from(value));
        } else {
            files.push(PathBuf::from(arg));
        }
    }

    if files.is_empty() {
        print_usage();
        return ExitCode::from(2);
    }

    let mut obligations: Vec<ObligationRecord> = Vec::new();
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
        let file = path.display().to_string();
        obligations.extend(decisions.iter().map(|d| d.to_record(&file)));
    }

    let json = serde_json::to_string_pretty(&obligations).expect("obligations always serialize");
    write_output(&json, output.as_deref())
}

/// `generate`: not implemented here -- writing the `n + 1` vectors as
/// actual test bodies is exactly the judgment call this workspace's
/// `/take` philosophy keeps out of an automated tool (issue #85 step 2:
/// "Claude / human"). This subcommand only prints the tagging convention
/// [`harvest`] expects, so a human or an LLM session driving `cargo
/// mvl-mcdc scan -o obligations.json` knows the contract without having to
/// read this crate's source.
fn run_generate(args: &[String]) -> ExitCode {
    let obligations_path = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--obligations="));
    let Some(obligations_path) = obligations_path else {
        eprintln!("error: --obligations=FILE is required");
        print_usage();
        return ExitCode::from(2);
    };

    let obligations = match load_obligations(Path::new(obligations_path)) {
        Ok(obligations) => obligations,
        Err(code) => return code,
    };

    println!("Tag each generated test with `mcdc__<id>__v<N>` (N = 1..=vectors_required), e.g.:");
    println!("  #[test]");
    println!("  fn mcdc__delete_60__v1_leaf_a_true() {{ /* ... */ }}");
    println!();
    for obligation in &obligations {
        if obligation.compiler_void {
            continue;
        }
        println!(
            "{} ({}:{}) -- {} condition(s), {} vectors needed: `{}`",
            obligation.id,
            obligation.file,
            obligation.line,
            obligation.conditions,
            obligation.vectors_required,
            obligation.decision
        );
    }
    ExitCode::SUCCESS
}

fn load_obligations(path: &Path) -> Result<Vec<ObligationRecord>, ExitCode> {
    let text = std::fs::read_to_string(path).map_err(|err| {
        eprintln!("error: failed to read {}: {err}", path.display());
        ExitCode::from(2)
    })?;
    serde_json::from_str(&text).map_err(|err| {
        eprintln!("error: failed to parse {} as obligations JSON: {err}", path.display());
        ExitCode::from(2)
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
        print_mutation_report(&all_outcomes, killed_conditions, total_conditions);
    } else {
        print_summary(&all_outcomes, total_decisions, complete_decisions, decisions_pct, conditions_pct);
    }

    if decisions_pct < options.min_decisions_pct || conditions_pct < options.min_conditions_pct {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// `harvest`: step 4 of the scan → generate → run → harvest pipeline --
/// joins a previously scanned `obligations.json` against tagged tests'
/// pass/fail outcomes, no mutation, no re-scanning.
fn run_harvest(args: &[String]) -> ExitCode {
    let mut obligations_path = None;
    let mut run_dir = PathBuf::from(".");
    let mut min_decisions_pct = 0.0;
    let mut emit_json = false;
    let mut output = None;

    for arg in args {
        if let Some(value) = arg.strip_prefix("--obligations=") {
            obligations_path = Some(PathBuf::from(value));
        } else if let Some(value) = arg.strip_prefix("--run-dir=") {
            run_dir = PathBuf::from(value);
        } else if let Some(value) = arg.strip_prefix("--min-decisions=") {
            match value.parse() {
                Ok(pct) => min_decisions_pct = pct,
                Err(_) => {
                    eprintln!("error: invalid --min-decisions value: {value}");
                    return ExitCode::from(2);
                }
            }
        } else if arg == "--emit-mcdc-json" {
            emit_json = true;
        } else if let Some(value) = arg.strip_prefix("-o=") {
            output = Some(PathBuf::from(value));
        } else {
            eprintln!("error: unrecognized argument: {arg}");
            print_usage();
            return ExitCode::from(2);
        }
    }

    let Some(obligations_path) = obligations_path else {
        eprintln!("error: --obligations=FILE is required");
        print_usage();
        return ExitCode::from(2);
    };

    let obligations = match load_obligations(&obligations_path) {
        Ok(obligations) => obligations,
        Err(code) => return code,
    };

    let discharges = match harvest::harvest(&obligations, &run_dir) {
        Ok(discharges) => discharges,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(2);
        }
    };

    let total = discharges.len();
    let complete = discharges.iter().filter(|d| d.discharged).count();
    let decisions_pct = percentage(complete, total);

    if emit_json {
        let conditions = discharges
            .iter()
            .map(|d| McdcCondition {
                id: d.id.clone(),
                covered: d.discharged,
            })
            .collect();
        let mut report = AssuranceReport::new("cargo-mvl-mcdc", current_timestamp());
        report.mcdc = Some(McdcSection {
            conditions,
            coverage_pct: decisions_pct,
        });
        let json = serde_json::to_string_pretty(&report).expect("AssuranceReport always serializes");
        write_output(&json, output.as_deref());
    } else {
        let json = serde_json::to_string_pretty(&discharges).expect("discharges always serialize");
        let code = write_output(&json, output.as_deref());
        if code != ExitCode::SUCCESS {
            return code;
        }
        eprintln!(
            "MC/DC harvest: {complete}/{total} obligations discharged ({decisions_pct:.1}%)"
        );
        for discharge in &discharges {
            if !discharge.discharged {
                eprintln!(
                    "  undischarged: {} ({}:{}) -- {}/{} vectors tagged & passing",
                    discharge.id,
                    discharge.file,
                    discharge.line,
                    discharge.vectors_discharged,
                    discharge.vectors_required
                );
            }
        }
    }

    if decisions_pct < min_decisions_pct {
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

fn print_mutation_report(
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
