//! `cargo mvl verify`: check a binary against its signed provenance.
//!
//! [`verify`] gathers the inputs (artifact digest, embedded record,
//! cargo-auditable data, both attestations, the lockfile at the signed
//! commit); [`evaluate`] is the pure decision over them, so every failure
//! case is testable without a network or a signed artifact.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::auditable::{Audit, Dep};
use crate::provenance::{self, Verified, BUILD_ENV, SLSA};
use crate::record::Record;

/// What the artifact must have been built by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// `owner/repo`.
    pub repo: String,
    /// Workflow path in the repo, e.g. `.github/workflows/attest.yml`.
    pub workflow: String,
    /// Allowed source ref, `*` as a wildcard, e.g. `refs/tags/v*`.
    pub ref_pattern: String,
}

/// Everything [`evaluate`] decides on. Each input is the result of reading
/// it, so a missing input fails its check rather than aborting the report.
#[derive(Debug, Clone)]
pub struct Inputs {
    pub artifact_sha256: String,
    pub record: Result<Record, String>,
    pub audit: Result<Audit, String>,
    pub slsa: Result<Verified, String>,
    pub build_env: Result<Verified, String>,
    /// The lockfile at the signed commit (text).
    pub lockfile: Result<String, String>,
}

/// One named check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

/// The verification result: every check, in a fixed order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub checks: Vec<Check>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.checks.iter().all(|c| c.ok)
    }

    pub fn failed(&self) -> Vec<&'static str> {
        self.checks
            .iter()
            .filter(|c| !c.ok)
            .map(|c| c.name)
            .collect()
    }
}

/// `*` matches any run of characters; everything else matches itself.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == text,
        Some((head, rest)) => {
            let Some(text) = text.strip_prefix(head) else {
                return false;
            };
            (0..=text.len()).any(|i| text.is_char_boundary(i) && glob_match(rest, &text[i..]))
        }
    }
}

fn check(name: &'static str, ok: bool, detail: impl Into<String>) -> Check {
    Check {
        name,
        ok,
        detail: detail.into(),
    }
}

fn from<T>(name: &'static str, r: &Result<T, String>, detail: impl FnOnce(&T) -> String) -> Check {
    match r {
        Ok(v) => check(name, true, detail(v)),
        Err(e) => check(name, false, e.clone()),
    }
}

/// Decide on the gathered inputs.
pub fn evaluate(i: &Inputs, p: &Policy) -> Report {
    let mut c = Vec::new();
    let repo_uri = format!("https://github.com/{}", p.repo);
    let signer_prefix = format!("{repo_uri}/{}@", p.workflow);

    // Signatures (verified by gh), for this exact artifact.
    c.push(from("signature", &i.slsa, |v| {
        format!(
            "SLSA provenance for sha256 {} ({})",
            i.artifact_sha256, v.run_uri
        )
    }));
    c.push(from("build-env", &i.build_env, |_| {
        "build-environment attestation present".to_string()
    }));

    // Policy on the signer identity, re-checked from the certificate.
    if let Ok(v) = &i.slsa {
        c.push(check(
            "repository",
            v.repo_uri.eq_ignore_ascii_case(&repo_uri),
            format!("{} (want {repo_uri})", v.repo_uri),
        ));
        c.push(check(
            "workflow",
            v.signer_uri.starts_with(&signer_prefix),
            format!("{} (want {}…)", v.signer_uri, signer_prefix),
        ));
        c.push(check(
            "ref",
            glob_match(&p.ref_pattern, &v.source_ref),
            format!("{} (want {})", v.source_ref, p.ref_pattern),
        ));
    }

    // The embedded manifest record.
    c.push(from("manifest-record", &i.record, |r| {
        format!("{} v{}", r.get("app"), r.get("app_version"))
    }));
    if let Ok(r) = &i.record {
        c.push(check(
            "clean-build",
            r.get("vcs_dirty") == "false",
            format!("vcs_dirty={}", r.get("vcs_dirty")),
        ));
    }

    // cargo-auditable data, and the record agreeing that it was used.
    let rec_auditable = i.record.as_ref().map(|r| r.get("auditable") == "true");
    c.push(match (&i.audit, rec_auditable) {
        (Err(e), _) => check("auditable", false, e.clone()),
        (Ok(_), Ok(false)) => check("auditable", false, "manifest record says auditable=false"),
        (Ok(a), _) => check("auditable", true, format!("{} packages", a.packages.len())),
    });

    // Commit: manifest == certificate == SLSA resolvedDependencies.
    if let (Ok(r), Ok(v)) = (&i.record, &i.slsa) {
        let slsa = provenance::slsa_commit(&v.predicate);
        let manifest = r.get("vcs_commit");
        c.push(check(
            "commit",
            !manifest.is_empty() && manifest == v.commit && manifest == slsa,
            format!(
                "manifest {manifest}, certificate {}, provenance {slsa}",
                v.commit
            ),
        ));
    }

    // Lockfile: at the signed commit == manifest == build-env predicate.
    let lock_set: Option<BTreeSet<Dep>> = i
        .lockfile
        .as_ref()
        .ok()
        .and_then(|t| crate::lockfile::packages(t).ok());
    match (&i.lockfile, &i.record) {
        (Err(e), _) => c.push(check("lockfile", false, e.clone())),
        (Ok(text), Ok(r)) => {
            let at_commit = crate::sha256_hex(text.as_bytes());
            let signed = i
                .build_env
                .as_ref()
                .map(|v| {
                    v.predicate["lockfile_digest"]
                        .as_str()
                        .unwrap_or("")
                        .to_string()
                })
                .unwrap_or_default();
            let manifest = r.get("lockfile_digest");
            c.push(check(
                "lockfile",
                at_commit == manifest && at_commit == signed,
                format!("at commit {at_commit}, manifest {manifest}, build-env {signed}"),
            ));
        }
        (Ok(_), Err(_)) => c.push(check("lockfile", false, "no manifest record to compare")),
    }

    // Dependencies: every embedded package is in the signed commit's lockfile,
    // and the embedded data is the data the build-env predicate signed.
    if let Ok(a) = &i.audit {
        let missing: Vec<String> = match &lock_set {
            Some(set) => a
                .packages
                .iter()
                .filter(|d| !set.contains(d))
                .map(|d| format!("{}@{} ({})", d.name, d.version, d.source))
                .collect(),
            None => vec!["(no lockfile to compare)".to_string()],
        };
        let signed = i
            .build_env
            .as_ref()
            .map(|v| {
                v.predicate["auditable_digest"]
                    .as_str()
                    .unwrap_or("")
                    .to_string()
            })
            .unwrap_or_default();
        let digest_ok = signed == a.digest;
        c.push(check(
            "dependencies",
            missing.is_empty() && digest_ok,
            if !missing.is_empty() {
                format!(
                    "not in the lockfile at the signed commit: {}",
                    missing.join(", ")
                )
            } else if !digest_ok {
                format!(
                    "auditable data {} differs from the signed {signed}",
                    a.digest
                )
            } else {
                format!("{} packages match the lockfile", a.packages.len())
            },
        ));
    }

    Report { checks: c }
}

/// Where to look things up.
#[derive(Debug, Clone)]
pub struct Options {
    pub artifact: PathBuf,
    pub policy: Policy,
    /// A local checkout to read the lockfile at the signed commit from;
    /// otherwise the GitHub API is used.
    pub source: Option<PathBuf>,
    /// Repository-relative lockfile path; defaults to the build-env
    /// predicate's `lockfile_path`, then `Cargo.lock`.
    pub lockfile: Option<String>,
    /// The `gh` executable (keyless mode, and the lockfile fetch without
    /// `--source`).
    pub gh: PathBuf,
    /// Key/KMS mode instead of keyless; see [`KeyMode`].
    pub key: Option<KeyMode>,
}

/// Key/KMS mode (ADR-0014 amendment): signatures by a pinned key, verified
/// with `cosign`, instead of keyless GitHub attestations. The identity checks
/// then read the signed SLSA predicate, which the key holder vouches for.
#[derive(Debug, Clone)]
pub struct KeyMode {
    /// A public key file or a KMS URI (`awskms://…`, `gcpkms://…`, `hashivault://…`).
    pub key: String,
    /// The `cosign` executable.
    pub cosign: PathBuf,
    /// Bundle of the SLSA provenance attestation.
    pub slsa_bundle: PathBuf,
    /// Bundle of the build-environment attestation.
    pub build_env_bundle: PathBuf,
    /// Require a transparency-log entry (the default). Off only for
    /// air-gapped signing, where there is no log to check.
    pub tlog: bool,
}

impl KeyMode {
    /// Default bundle paths next to the artifact: `<artifact>.slsa.bundle`
    /// and `<artifact>.build-env.bundle`.
    pub fn bundles_next_to(artifact: &Path) -> (PathBuf, PathBuf) {
        let with = |ext: &str| {
            let mut s = artifact.as_os_str().to_owned();
            s.push(ext);
            PathBuf::from(s)
        };
        (with(".slsa.bundle"), with(".build-env.bundle"))
    }
}

/// Gather the inputs and evaluate them.
pub fn verify(o: &Options) -> Report {
    let p = &o.policy;
    let bytes = std::fs::read(&o.artifact);
    let artifact_sha256 = bytes
        .as_ref()
        .map(|b| crate::sha256_hex(b))
        .unwrap_or_default();
    let attested = |pt: &str| match &o.key {
        None => provenance::gh_verify(&o.gh, &o.artifact, &p.repo, &p.workflow, pt)
            .and_then(|r| provenance::select(&r, &artifact_sha256)),
        Some(k) => {
            let bundle = if pt == SLSA {
                &k.slsa_bundle
            } else {
                &k.build_env_bundle
            };
            provenance::cosign_verify(&k.cosign, &o.artifact, &k.key, bundle, pt, k.tlog)
                .and_then(|st| provenance::from_statement(&st, None, &artifact_sha256))
        }
    };
    let slsa = attested(SLSA);
    let build_env = attested(BUILD_ENV);
    let path = o.lockfile.clone().unwrap_or_else(|| {
        build_env
            .as_ref()
            .ok()
            .and_then(|v| v.predicate["lockfile_path"].as_str().map(str::to_string))
            .unwrap_or_else(|| "Cargo.lock".to_string())
    });
    let lockfile = match &slsa {
        Ok(v) => crate::lockfile::at_commit(o.source.as_deref(), &o.gh, &p.repo, &v.commit, &path),
        Err(_) => Err("no verified commit to read the lockfile at".to_string()),
    };
    let inputs = Inputs {
        record: bytes
            .as_ref()
            .map_err(|e| format!("cannot read {}: {e}", o.artifact.display()))
            .and_then(|b| crate::record::find(b)),
        audit: crate::auditable::read(&o.artifact),
        artifact_sha256,
        slsa,
        build_env,
        lockfile,
    };
    evaluate(&inputs, p)
}

/// Shorthand used by the CLI and tests.
pub fn default_gh() -> PathBuf {
    Path::new("gh").to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provenance::fixtures::{result, slsa_predicate};
    use serde_json::json;

    const REPO: &str = "mvl-lang/mvl-rust";
    const WF: &str = ".github/workflows/attest.yml";
    const REF: &str = "refs/heads/main";
    const COMMIT: &str = "1111111111111111111111111111111111111111";
    const SHA: &str = "aaaa";
    const LOCK: &str = "version = 4\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"log\"\nversion = \"0.4.33\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n";

    fn policy() -> Policy {
        Policy {
            repo: REPO.into(),
            workflow: WF.into(),
            ref_pattern: "refs/heads/*".into(),
        }
    }

    fn audit() -> Audit {
        crate::auditable::parse(r#"{"packages":[{"name":"demo","version":"0.1.0","source":"local","root":true},{"name":"log","version":"0.4.33","source":"crates.io"}]}"#).unwrap()
    }

    fn record(commit: &str, lock_digest: &str, dirty: bool, auditable: bool) -> Record {
        let text = mvl_manifest::record::render(&[
            ("app", "demo"),
            ("app_version", "0.1.0"),
            ("vcs_commit", commit),
            ("vcs_dirty", if dirty { "true" } else { "false" }),
            ("lockfile_digest", lock_digest),
            ("auditable", if auditable { "true" } else { "false" }),
        ]);
        crate::record::find(text.as_bytes()).unwrap()
    }

    fn attestation(
        repo: &str,
        wf: &str,
        git_ref: &str,
        pt: &str,
        predicate: serde_json::Value,
    ) -> Verified {
        let r = result(&[SHA], repo, wf, git_ref, COMMIT, pt, predicate);
        provenance::select(&r, SHA).unwrap()
    }

    /// Inputs for an artifact that passes every check.
    fn good() -> Inputs {
        let lock_digest = crate::sha256_hex(LOCK.as_bytes());
        Inputs {
            artifact_sha256: SHA.into(),
            record: Ok(record(COMMIT, &lock_digest, false, true)),
            audit: Ok(audit()),
            slsa: Ok(attestation(
                REPO,
                WF,
                REF,
                SLSA,
                slsa_predicate(REPO, REF, COMMIT),
            )),
            build_env: Ok(attestation(
                REPO,
                WF,
                REF,
                BUILD_ENV,
                json!({"lockfile_path": "Cargo.lock", "lockfile_digest": lock_digest, "auditable_digest": audit().digest}),
            )),
            lockfile: Ok(LOCK.into()),
        }
    }

    #[test]
    fn genuine_artifact_passes_every_check() {
        let r = evaluate(&good(), &policy());
        assert!(r.ok(), "{:?}", r.failed());
        let names: Vec<&str> = r.checks.iter().map(|c| c.name).collect();
        assert_eq!(
            names,
            [
                "signature",
                "build-env",
                "repository",
                "workflow",
                "ref",
                "manifest-record",
                "clean-build",
                "auditable",
                "commit",
                "lockfile",
                "dependencies"
            ]
        );
    }

    #[test]
    fn modified_binary_fails() {
        // A changed byte changes the digest: no attestation subject matches.
        let mut i = good();
        let r = result(
            &[SHA],
            REPO,
            WF,
            REF,
            COMMIT,
            SLSA,
            slsa_predicate(REPO, REF, COMMIT),
        );
        i.artifact_sha256 = "bbbb".into();
        i.slsa = provenance::select(&r, "bbbb");
        i.build_env = Err("no attestation subject matches".into());
        let rep = evaluate(&i, &policy());
        assert!(rep.failed().contains(&"signature"), "{:?}", rep.failed());
        assert!(!rep.ok());
    }

    #[test]
    fn wrong_repo_fails() {
        let mut i = good();
        i.slsa = Ok(attestation(
            "someone/fork",
            WF,
            REF,
            SLSA,
            slsa_predicate("someone/fork", REF, COMMIT),
        ));
        assert!(evaluate(&i, &policy()).failed().contains(&"repository"));
    }

    #[test]
    fn wrong_workflow_fails() {
        let mut i = good();
        i.slsa = Ok(attestation(
            REPO,
            ".github/workflows/other.yml",
            REF,
            SLSA,
            slsa_predicate(REPO, REF, COMMIT),
        ));
        assert_eq!(evaluate(&i, &policy()).failed(), ["workflow"]);
    }

    #[test]
    fn wrong_ref_fails() {
        let mut i = good();
        let git_ref = "refs/pull/7/merge";
        i.slsa = Ok(attestation(
            REPO,
            WF,
            git_ref,
            SLSA,
            slsa_predicate(REPO, git_ref, COMMIT),
        ));
        assert_eq!(evaluate(&i, &policy()).failed(), ["ref"]);
    }

    #[test]
    fn commit_mismatch_between_manifest_and_provenance_fails() {
        let mut i = good();
        let lock_digest = crate::sha256_hex(LOCK.as_bytes());
        i.record = Ok(record(
            "2222222222222222222222222222222222222222",
            &lock_digest,
            false,
            true,
        ));
        assert_eq!(evaluate(&i, &policy()).failed(), ["commit"]);
    }

    #[test]
    fn lockfile_mismatch_between_manifest_and_signed_commit_fails() {
        let mut i = good();
        i.record = Ok(record(COMMIT, "ffff", false, true));
        assert_eq!(evaluate(&i, &policy()).failed(), ["lockfile"]);
    }

    #[test]
    fn dependency_not_in_signed_lockfile_fails() {
        let mut i = good();
        let a = crate::auditable::parse(
            r#"{"packages":[{"name":"evil","version":"6.6.6","source":"crates.io"}]}"#,
        )
        .unwrap();
        i.audit = Ok(a);
        let rep = evaluate(&i, &policy());
        assert!(rep.failed().contains(&"dependencies"));
        let d = rep
            .checks
            .iter()
            .find(|c| c.name == "dependencies")
            .unwrap();
        assert!(d.detail.contains("evil@6.6.6"), "{}", d.detail);
    }

    #[test]
    fn binary_built_without_cargo_auditable_fails() {
        let mut i = good();
        i.audit = Err("no cargo-auditable data in demo".into());
        assert_eq!(evaluate(&i, &policy()).failed(), ["auditable"]);
        // The record's own claim must agree too.
        let mut i = good();
        let lock_digest = crate::sha256_hex(LOCK.as_bytes());
        i.record = Ok(record(COMMIT, &lock_digest, false, false));
        assert_eq!(evaluate(&i, &policy()).failed(), ["auditable"]);
    }

    #[test]
    fn dirty_build_fails() {
        let mut i = good();
        let lock_digest = crate::sha256_hex(LOCK.as_bytes());
        i.record = Ok(record(COMMIT, &lock_digest, true, true));
        assert_eq!(evaluate(&i, &policy()).failed(), ["clean-build"]);
    }

    #[test]
    fn unsigned_artifact_fails() {
        let mut i = good();
        i.slsa = Err("gh attestation verify failed: no attestations found".into());
        i.build_env = Err("gh attestation verify failed: no attestations found".into());
        i.lockfile = Err("no verified commit to read the lockfile at".into());
        let f = evaluate(&i, &policy()).failed();
        assert!(
            f.contains(&"signature") && f.contains(&"build-env"),
            "{f:?}"
        );
    }

    #[test]
    fn key_mode_bundles_default_next_to_the_artifact() {
        let (s, e) = KeyMode::bundles_next_to(Path::new("dist/app"));
        assert_eq!(s, Path::new("dist/app.slsa.bundle"));
        assert_eq!(e, Path::new("dist/app.build-env.bundle"));
    }

    fn slsa_predicate_with_workflow(
        repo: &str,
        wf: &str,
        git_ref: &str,
        commit: &str,
    ) -> serde_json::Value {
        json!({"buildDefinition": {
            "externalParameters": {"workflow": {
                "repository": format!("https://github.com/{repo}"),
                "path": wf,
                "ref": git_ref,
            }},
            "resolvedDependencies": [{"digest": {"gitCommit": commit}}],
        }})
    }

    #[test]
    fn key_mode_cross_checks_are_the_same_as_keyless() {
        // Identity from the signed SLSA predicate instead of a certificate:
        // the same evaluate() decides, so a commit mismatch fails the same way.
        let mut i = good();
        let st = json!({
            "subject": [{"digest": {"sha256": SHA}}],
            "predicate": slsa_predicate_with_workflow(REPO, WF, REF, COMMIT),
        });
        i.slsa = provenance::from_statement(&st, None, SHA);
        let r = evaluate(&i, &policy());
        assert!(r.ok(), "{:?}", r.failed());
        let lock_digest = crate::sha256_hex(LOCK.as_bytes());
        i.record = Ok(record(
            "2222222222222222222222222222222222222222",
            &lock_digest,
            false,
            true,
        ));
        assert_eq!(evaluate(&i, &policy()).failed(), ["commit"]);
    }

    #[test]
    fn glob_matches_refs() {
        assert!(glob_match("refs/tags/v*", "refs/tags/v0.12.1"));
        assert!(glob_match("refs/heads/main", "refs/heads/main"));
        assert!(!glob_match("refs/heads/main", "refs/heads/mainx"));
        assert!(glob_match("*", "anything"));
        assert!(!glob_match("refs/tags/v*", "refs/heads/v1"));
    }
}
