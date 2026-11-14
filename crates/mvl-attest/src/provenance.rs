//! Signature and provenance verification through `gh attestation verify`.
//!
//! `gh` checks the Sigstore signature, the transparency log, the certificate
//! chain and the signer identity. This module selects the result whose
//! subject is our artifact and extracts the fields the cross-checks join on.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// GitHub's SLSA build provenance (`actions/attest-build-provenance`).
pub const SLSA: &str = "https://slsa.dev/provenance/v1";
/// mvl's build-environment predicate (`cargo mvl attest`, `actions/attest`).
pub const BUILD_ENV: &str = "https://mvl-lang.org/attest/build-env/v1";

/// A verified attestation for the artifact.
#[derive(Debug, Clone, PartialEq)]
pub struct Verified {
    /// `sourceRepositoryURI`, e.g. `https://github.com/owner/repo`.
    pub repo_uri: String,
    /// `sourceRepositoryRef`, e.g. `refs/heads/main`.
    pub source_ref: String,
    /// `sourceRepositoryDigest`: the commit the workflow ran on.
    pub commit: String,
    /// `buildSignerURI`: `https://github.com/<repo>/<workflow>@<ref>`.
    pub signer_uri: String,
    /// `runInvocationURI`.
    pub run_uri: String,
    /// The statement's predicate.
    pub predicate: Value,
}

/// Run `gh attestation verify` for one predicate type and return its JSON.
/// `gh` enforces the repository and the signer workflow itself.
pub fn gh_verify(
    gh: &Path,
    artifact: &Path,
    repo: &str,
    workflow: &str,
    predicate_type: &str,
) -> Result<Value, String> {
    let out = Command::new(gh)
        .args(["attestation", "verify"])
        .arg(artifact)
        .args(["--repo", repo])
        .args(["--signer-workflow", &format!("{repo}/{workflow}")])
        .args(["--predicate-type", predicate_type])
        .args(["--format", "json"])
        .output()
        .map_err(|e| format!("cannot run {}: {e}", gh.display()))?;
    if !out.status.success() {
        return Err(format!(
            "gh attestation verify ({predicate_type}) failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("gh output is not JSON: {e}"))
}

/// The verified result whose subject digest is `sha256`. One attestation can
/// cover several subjects, so the first subject is not necessarily ours.
pub fn select(results: &Value, sha256: &str) -> Result<Verified, String> {
    let results = results.as_array().ok_or("gh output is not a list")?;
    for r in results {
        let v = &r["verificationResult"];
        let st = &v["statement"];
        let ours = st["subject"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|s| s["digest"]["sha256"].as_str() == Some(sha256));
        if !ours {
            continue;
        }
        let c = &v["signature"]["certificate"];
        let s = |k: &str| c[k].as_str().unwrap_or("").to_string();
        return Ok(Verified {
            repo_uri: s("sourceRepositoryURI"),
            source_ref: s("sourceRepositoryRef"),
            commit: s("sourceRepositoryDigest"),
            signer_uri: s("buildSignerURI"),
            run_uri: s("runInvocationURI"),
            predicate: st["predicate"].clone(),
        });
    }
    Err(format!(
        "no attestation subject matches the artifact's sha256 {sha256}"
    ))
}

/// The commit an SLSA predicate names in `resolvedDependencies`.
pub fn slsa_commit(predicate: &Value) -> String {
    predicate["buildDefinition"]["resolvedDependencies"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|d| d["digest"]["gitCommit"].as_str())
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
pub(crate) mod fixtures {
    use serde_json::{json, Value};

    /// A `gh attestation verify --format json` result, in the shape gh 2.103
    /// returns (captured from a real verification of a cli/cli release).
    pub fn result(
        subjects: &[&str],
        repo: &str,
        workflow: &str,
        git_ref: &str,
        commit: &str,
        predicate_type: &str,
        predicate: Value,
    ) -> Value {
        let subject: Vec<Value> = subjects
            .iter()
            .map(|d| json!({"name": "artifact", "digest": {"sha256": d}}))
            .collect();
        let signer = format!("https://github.com/{repo}/{workflow}@{git_ref}");
        json!([{
            "verificationResult": {
                "mediaType": "application/vnd.dev.sigstore.verificationresult+json;version=0.1",
                "statement": {
                    "_type": "https://in-toto.io/Statement/v1",
                    "subject": subject,
                    "predicateType": predicate_type,
                    "predicate": predicate,
                },
                "signature": {"certificate": {
                    "issuer": "https://token.actions.githubusercontent.com",
                    "subjectAlternativeName": signer,
                    "githubWorkflowRepository": repo,
                    "githubWorkflowRef": git_ref,
                    "buildSignerURI": signer,
                    "runnerEnvironment": "github-hosted",
                    "sourceRepositoryURI": format!("https://github.com/{repo}"),
                    "sourceRepositoryDigest": commit,
                    "sourceRepositoryRef": git_ref,
                    "runInvocationURI": format!("https://github.com/{repo}/actions/runs/1/attempts/1"),
                }},
            }
        }])
    }

    pub fn slsa_predicate(repo: &str, git_ref: &str, commit: &str) -> Value {
        json!({
            "buildDefinition": {
                "buildType": "https://actions.github.io/buildtypes/workflow/v1",
                "resolvedDependencies": [{
                    "uri": format!("git+https://github.com/{repo}@{git_ref}"),
                    "digest": {"gitCommit": commit},
                }],
            },
            "runDetails": {"metadata": {"invocationId": format!("https://github.com/{repo}/actions/runs/1/attempts/1")}},
        })
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    #[test]
    fn selects_the_subject_matching_the_artifact_not_the_first() {
        let r = result(
            &["0000", "abcd"],
            "o/r",
            ".github/workflows/attest.yml",
            "refs/heads/main",
            "c0ffee",
            SLSA,
            slsa_predicate("o/r", "refs/heads/main", "c0ffee"),
        );
        let v = select(&r, "abcd").unwrap();
        assert_eq!(v.commit, "c0ffee");
        assert_eq!(v.repo_uri, "https://github.com/o/r");
        assert_eq!(slsa_commit(&v.predicate), "c0ffee");
    }

    #[test]
    fn modified_artifact_matches_no_subject() {
        let r = result(
            &["abcd"],
            "o/r",
            "w.yml",
            "refs/heads/main",
            "c",
            SLSA,
            slsa_predicate("o/r", "refs/heads/main", "c"),
        );
        assert!(select(&r, "ffff")
            .unwrap_err()
            .contains("no attestation subject"));
    }
}
