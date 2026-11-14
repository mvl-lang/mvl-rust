//! `cargo mvl attest` and `cargo mvl verify` (wired in by `cargo-mvl`).

use std::path::PathBuf;
use std::process::ExitCode;

use crate::predicate::SlsaParams;
use crate::verify::{verify, KeyMode, Options, Policy};

pub const ATTEST_USAGE: &str = "\
usage: cargo mvl attest <BINARY> --lockfile <PATH>
       cargo mvl attest --slsa [--repo <OWNER/REPO>] [--workflow <PATH>] [--ref <REF>] [--commit <SHA>]

Without --slsa: prints the build-environment predicate
(https://mvl-lang.org/attest/build-env/v1) for BINARY as JSON, for CI to sign
next to the SLSA provenance. PATH is the repository-relative lockfile the
binary was built from.

With --slsa (key/KMS mode, where actions/attest-build-provenance cannot sign):
prints an SLSA v1 provenance predicate. Unset values come from the GitHub
Actions environment (GITHUB_REPOSITORY, GITHUB_WORKFLOW_REF, GITHUB_REF,
GITHUB_SHA).
";

pub const VERIFY_USAGE: &str = "\
usage: cargo mvl verify <BINARY> --repo <OWNER/REPO> --workflow <PATH> --ref <PATTERN>
                        [--source <DIR>] [--lockfile <PATH>] [--gh <GH>]
                        [--key <KEY> [--slsa-bundle <PATH>] [--build-env-bundle <PATH>]
                                     [--cosign <COSIGN>] [--no-tlog]]

Verifies BINARY against its signed provenance, then cross-checks the embedded
mvl-manifest record, the cargo-auditable data and the lockfile at the signed
commit. Exits 0 only if every check passes.

Keyless (default): signatures by the CI workflow's identity, checked with
`gh attestation verify`.

  --repo       repository that must have built it, e.g. mvl-lang/mvl-rust
  --workflow   workflow that must have signed it, e.g. .github/workflows/release.yml
  --ref        allowed source ref; * is a wildcard, e.g. refs/tags/v*
  --source     local checkout to read the lockfile from (default: GitHub API)
  --lockfile   lockfile path in the repo (default: from the build-env predicate)
  --gh         gh executable (default: gh)

Key/KMS mode (--key): signatures by a pinned key, checked with `cosign
verify-blob-attestation`. Repository, workflow and ref are then read from the
signed SLSA predicate: the key holder vouches for them.

  --key               public key file or KMS URI (awskms://, gcpkms://, hashivault://, azurekms://)
  --slsa-bundle       SLSA attestation bundle (default: <BINARY>.slsa.bundle)
  --build-env-bundle  build-environment bundle (default: <BINARY>.build-env.bundle)
  --cosign            cosign executable (default: cosign)
  --no-tlog           do not require a transparency-log entry (air-gapped signing only)
";

/// Positional arguments and `--key value` options (flags get `true`).
type Parsed = (Vec<String>, Vec<(String, String)>);

/// Split `args` into positionals, `--key value` options and `--flag`s.
fn parse(args: &[String], keys: &[&str], flags: &[&str]) -> Result<Parsed, String> {
    let mut pos = Vec::new();
    let mut opts = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let Some(k) = a.strip_prefix("--") else {
            pos.push(a.clone());
            continue;
        };
        if flags.contains(&k) {
            opts.push((k.to_string(), "true".to_string()));
            continue;
        }
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

fn print_json(v: Result<serde_json::Value, String>) -> ExitCode {
    match v {
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

/// `cargo mvl attest`.
pub fn attest_main(args: &[String]) -> ExitCode {
    if wants_help(args) {
        print!("{ATTEST_USAGE}");
        return ExitCode::SUCCESS;
    }
    let (pos, opts) = match parse(
        args,
        &["lockfile", "repo", "workflow", "ref", "commit"],
        &["slsa"],
    ) {
        Ok(p) => p,
        Err(e) => return usage_error(ATTEST_USAGE, &e),
    };
    if get(&opts, "slsa").is_some() {
        return print_json(crate::predicate::slsa(&SlsaParams {
            repo: get(&opts, "repo"),
            workflow: get(&opts, "workflow"),
            git_ref: get(&opts, "ref"),
            commit: get(&opts, "commit"),
        }));
    }
    let ([binary], Some(lockfile)) = (pos.as_slice(), get(&opts, "lockfile")) else {
        return usage_error(ATTEST_USAGE, "need BINARY and --lockfile (or --slsa)");
    };
    print_json(crate::predicate::generate(
        &PathBuf::from(binary),
        &lockfile,
    ))
}

/// `cargo mvl verify`.
pub fn verify_main(args: &[String]) -> ExitCode {
    if wants_help(args) {
        print!("{VERIFY_USAGE}");
        return ExitCode::SUCCESS;
    }
    let (pos, opts) = match parse(
        args,
        &[
            "repo",
            "workflow",
            "ref",
            "source",
            "lockfile",
            "gh",
            "key",
            "slsa-bundle",
            "build-env-bundle",
            "cosign",
        ],
        &["no-tlog"],
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
    let artifact = PathBuf::from(binary);
    let key = get(&opts, "key").map(|key| {
        let (slsa, env) = KeyMode::bundles_next_to(&artifact);
        KeyMode {
            key,
            cosign: get(&opts, "cosign")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("cosign")),
            slsa_bundle: get(&opts, "slsa-bundle").map(PathBuf::from).unwrap_or(slsa),
            build_env_bundle: get(&opts, "build-env-bundle")
                .map(PathBuf::from)
                .unwrap_or(env),
            tlog: get(&opts, "no-tlog").is_none(),
        }
    });
    let o = Options {
        artifact,
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
        key,
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
            &[],
        )
        .unwrap();
        assert_eq!(pos, ["bin"]);
        assert_eq!(get(&opts, "repo").as_deref(), Some("o/r"));
        assert_eq!(get(&opts, "ref").as_deref(), Some("refs/*"));
    }

    #[test]
    fn flags_take_no_value() {
        let (pos, opts) = parse(&v(&["--no-tlog", "bin"]), &[], &["no-tlog"]).unwrap();
        assert_eq!(pos, ["bin"]);
        assert_eq!(get(&opts, "no-tlog").as_deref(), Some("true"));
    }

    #[test]
    fn rejects_unknown_and_valueless_options() {
        assert!(parse(&v(&["--nope", "x"]), &["repo"], &[])
            .unwrap_err()
            .contains("unknown"));
        assert!(parse(&v(&["--repo"]), &["repo"], &[])
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

    #[test]
    fn attest_slsa_prints_a_predicate_from_explicit_values() {
        let code = attest_main(&v(&[
            "--slsa",
            "--repo",
            "o/r",
            "--workflow",
            ".github/workflows/release.yml",
            "--ref",
            "refs/tags/v1",
            "--commit",
            "c0ffee",
        ]));
        assert_eq!(code, ExitCode::SUCCESS);
    }
}
