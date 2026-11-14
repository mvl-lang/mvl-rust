//! `cargo mvl attest` and `cargo mvl verify` (wired in by `cargo-mvl`).

use std::path::PathBuf;
use std::process::ExitCode;

use crate::verify::{verify, Options, Policy};

pub const ATTEST_USAGE: &str = "\
usage: cargo mvl attest <BINARY> --lockfile <PATH>

Prints the build-environment predicate (https://mvl-lang.org/attest/build-env/v1)
for BINARY as JSON, for CI to sign with actions/attest next to the SLSA
provenance. PATH is the repository-relative lockfile the binary was built from.
";

pub const VERIFY_USAGE: &str = "\
usage: cargo mvl verify <BINARY> --repo <OWNER/REPO> --workflow <PATH> --ref <PATTERN>
                        [--source <DIR>] [--lockfile <PATH>] [--gh <GH>]

Verifies BINARY against its signed provenance (gh attestation verify), then
cross-checks the embedded mvl-manifest record, the cargo-auditable data and the
lockfile at the signed commit. Exits 0 only if every check passes.

  --repo       repository that must have built it, e.g. mvl-lang/mvl-rust
  --workflow   workflow that must have signed it, e.g. .github/workflows/release.yml
  --ref        allowed source ref; * is a wildcard, e.g. refs/tags/v*
  --source     local checkout to read the lockfile from (default: GitHub API)
  --lockfile   lockfile path in the repo (default: from the build-env predicate)
  --gh         gh executable (default: gh)
";

/// Positional arguments and `--key value` options.
type Parsed = (Vec<String>, Vec<(String, String)>);

/// Split `args` into positionals and `--key value` options.
fn parse(args: &[String], keys: &[&str]) -> Result<Parsed, String> {
    let mut pos = Vec::new();
    let mut opts = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if let Some(k) = a.strip_prefix("--") {
            let (k, v) = match k.split_once('=') {
                Some((k, v)) => (k.to_string(), v.to_string()),
                None => (
                    k.to_string(),
                    it.next().ok_or(format!("--{k} needs a value"))?.clone(),
                ),
            };
            if !keys.contains(&k.as_str()) {
                return Err(format!("unknown option --{k}"));
            }
            opts.push((k, v));
        } else {
            pos.push(a.clone());
        }
    }
    Ok((pos, opts))
}

fn get(opts: &[(String, String)], k: &str) -> Option<String> {
    opts.iter()
        .rev()
        .find(|(key, _)| key == k)
        .map(|(_, v)| v.clone())
}

fn usage_error(usage: &str, msg: &str) -> ExitCode {
    eprintln!("error: {msg}\n\n{usage}");
    ExitCode::from(2)
}

fn wants_help(args: &[String]) -> bool {
    args.iter().any(|a| a == "-h" || a == "--help")
}

/// `cargo mvl attest`.
pub fn attest_main(args: &[String]) -> ExitCode {
    if wants_help(args) {
        print!("{ATTEST_USAGE}");
        return ExitCode::SUCCESS;
    }
    let (pos, opts) = match parse(args, &["lockfile"]) {
        Ok(p) => p,
        Err(e) => return usage_error(ATTEST_USAGE, &e),
    };
    let ([binary], Some(lockfile)) = (pos.as_slice(), get(&opts, "lockfile")) else {
        return usage_error(ATTEST_USAGE, "need BINARY and --lockfile");
    };
    match crate::predicate::generate(&PathBuf::from(binary), &lockfile) {
        Ok(v) => {
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("cargo mvl attest: {e}");
            ExitCode::from(1)
        }
    }
}

/// `cargo mvl verify`.
pub fn verify_main(args: &[String]) -> ExitCode {
    if wants_help(args) {
        print!("{VERIFY_USAGE}");
        return ExitCode::SUCCESS;
    }
    let (pos, opts) = match parse(
        args,
        &["repo", "workflow", "ref", "source", "lockfile", "gh"],
    ) {
        Ok(p) => p,
        Err(e) => return usage_error(VERIFY_USAGE, &e),
    };
    let ([binary], Some(repo), Some(workflow), Some(ref_pattern)) = (
        pos.as_slice(),
        get(&opts, "repo"),
        get(&opts, "workflow"),
        get(&opts, "ref"),
    ) else {
        return usage_error(VERIFY_USAGE, "need BINARY, --repo, --workflow and --ref");
    };
    let o = Options {
        artifact: PathBuf::from(binary),
        policy: Policy {
            repo,
            workflow,
            ref_pattern,
        },
        source: get(&opts, "source").map(PathBuf::from),
        lockfile: get(&opts, "lockfile"),
        gh: get(&opts, "gh")
            .map(PathBuf::from)
            .unwrap_or_else(crate::verify::default_gh),
    };
    let report = verify(&o);
    for c in &report.checks {
        println!(
            "{} {:<16} {}",
            if c.ok { "ok  " } else { "FAIL" },
            c.name,
            c.detail
        );
    }
    if report.ok() {
        println!("verified: {binary}");
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "NOT verified: {binary} (failed: {})",
            report.failed().join(", ")
        );
        ExitCode::from(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_both_option_spellings() {
        let (pos, opts) = parse(
            &v(&["bin", "--repo", "o/r", "--ref=refs/*"]),
            &["repo", "ref"],
        )
        .unwrap();
        assert_eq!(pos, ["bin"]);
        assert_eq!(get(&opts, "repo").as_deref(), Some("o/r"));
        assert_eq!(get(&opts, "ref").as_deref(), Some("refs/*"));
    }

    #[test]
    fn rejects_unknown_and_valueless_options() {
        assert!(parse(&v(&["--nope", "x"]), &["repo"])
            .unwrap_err()
            .contains("unknown"));
        assert!(parse(&v(&["--repo"]), &["repo"])
            .unwrap_err()
            .contains("needs a value"));
    }

    #[test]
    fn missing_arguments_are_a_usage_error() {
        assert_eq!(
            verify_main(&v(&["bin", "--repo", "o/r"])),
            ExitCode::from(2)
        );
        assert_eq!(attest_main(&v(&["bin"])), ExitCode::from(2));
    }
}
