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
    results
        .iter()
        .find_map(|r| {
            let v = &r["verificationResult"];
            from_statement(
                &v["statement"],
                Some(&v["signature"]["certificate"]),
                sha256,
            )
            .ok()
        })
        .ok_or(format!(
            "no attestation subject matches the artifact's sha256 {sha256}"
        ))
}

/// A verified in-toto statement whose subject includes `sha256`.
///
/// With a Sigstore certificate (keyless mode) the identity fields come from
/// the certificate, which the CI platform's OIDC token vouches for. Without
/// one (key/KMS mode) they come from the signed SLSA predicate's
/// `externalParameters.workflow`, which only the key holder vouches for.
pub fn from_statement(st: &Value, cert: Option<&Value>, sha256: &str) -> Result<Verified, String> {
    let ours = st["subject"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|s| s["digest"]["sha256"].as_str() == Some(sha256));
    if !ours {
        return Err(format!(
            "no attestation subject matches the artifact's sha256 {sha256}"
        ));
    }
    let predicate = st["predicate"].clone();
    let v = match cert {
        Some(c) => {
            let s = |k: &str| c[k].as_str().unwrap_or("").to_string();
            Verified {
                repo_uri: s("sourceRepositoryURI"),
                source_ref: s("sourceRepositoryRef"),
                commit: s("sourceRepositoryDigest"),
                signer_uri: s("buildSignerURI"),
                run_uri: s("runInvocationURI"),
                predicate,
            }
        }
        None => {
            let w = &predicate["buildDefinition"]["externalParameters"]["workflow"];
            let s = |v: &Value| v.as_str().unwrap_or("").to_string();
            let (repo, path, git_ref) = (s(&w["repository"]), s(&w["path"]), s(&w["ref"]));
            Verified {
                signer_uri: if repo.is_empty() {
                    String::new()
                } else {
                    format!("{repo}/{path}@{git_ref}")
                },
                repo_uri: repo,
                source_ref: git_ref,
                commit: slsa_commit(&predicate),
                run_uri: s(&predicate["runDetails"]["metadata"]["invocationId"]),
                predicate,
            }
        }
    };
    Ok(v)
}

/// The in-toto statement inside a Sigstore bundle's DSSE envelope.
pub fn bundle_statement(bundle: &str) -> Result<Value, String> {
    let b: Value = serde_json::from_str(bundle).map_err(|e| format!("bundle is not JSON: {e}"))?;
    let payload = b["dsseEnvelope"]["payload"]
        .as_str()
        .ok_or("bundle has no DSSE envelope")?;
    let bytes = base64_decode(payload).ok_or("bundle payload is not base64")?;
    serde_json::from_slice(&bytes).map_err(|e| format!("bundle payload is not a statement: {e}"))
}

/// Standard base64 (with padding), enough for DSSE payloads.
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    };
    let bytes: Vec<u8> = s
        .bytes()
        .filter(|c| !c.is_ascii_whitespace() && *c != b'=')
        .collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            n |= val(*c)? << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}

/// Key/KMS mode: `cosign verify-blob-attestation` checks the bundle's
/// signature against `key` (a public key file or a KMS URI) and the
/// statement's subject against the artifact. On success, the bundle's
/// statement is returned for the cross-checks.
pub fn cosign_verify(
    cosign: &Path,
    artifact: &Path,
    key: &str,
    bundle: &Path,
    predicate_type: &str,
    tlog: bool,
) -> Result<Value, String> {
    let mut cmd = Command::new(cosign);
    cmd.args(["verify-blob-attestation", "--key", key])
        .arg("--bundle")
        .arg(bundle)
        .args(["--type", predicate_type]);
    if !tlog {
        cmd.arg("--insecure-ignore-tlog=true");
    }
    let out = cmd
        .arg(artifact)
        .output()
        .map_err(|e| format!("cannot run {}: {e}", cosign.display()))?;
    if !out.status.success() {
        return Err(format!(
            "cosign verify-blob-attestation ({predicate_type}) failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let text = std::fs::read_to_string(bundle)
        .map_err(|e| format!("cannot read {}: {e}", bundle.display()))?;
    bundle_statement(&text)
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
    fn statement_without_certificate_takes_identity_from_slsa_predicate() {
        let st = serde_json::json!({
            "subject": [{"name": "app", "digest": {"sha256": "abcd"}}],
            "predicate": {
                "buildDefinition": {
                    "externalParameters": {"workflow": {
                        "repository": "https://github.com/o/r",
                        "path": ".github/workflows/release.yml",
                        "ref": "refs/tags/v1.0.0",
                    }},
                    "resolvedDependencies": [{"digest": {"gitCommit": "c0ffee"}}],
                },
                "runDetails": {"metadata": {"invocationId": "https://ci/run/1"}},
            },
        });
        let v = from_statement(&st, None, "abcd").unwrap();
        assert_eq!(v.repo_uri, "https://github.com/o/r");
        assert_eq!(v.source_ref, "refs/tags/v1.0.0");
        assert_eq!(v.commit, "c0ffee");
        assert_eq!(
            v.signer_uri,
            "https://github.com/o/r/.github/workflows/release.yml@refs/tags/v1.0.0"
        );
        assert_eq!(v.run_uri, "https://ci/run/1");
        assert!(from_statement(&st, None, "ffff").is_err());
    }

    #[test]
    fn decodes_the_statement_in_a_cosign_bundle() {
        // The shape cosign v3 writes: a Sigstore v0.3 bundle around a DSSE envelope.
        let statement = r#"{"_type":"https://in-toto.io/Statement/v0.1","subject":[{"name":"blob","digest":{"sha256":"982b"}}],"predicateType":"x","predicate":{"lockfile_digest":"abc"}}"#;
        let bundle = serde_json::json!({
            "mediaType": "application/vnd.dev.sigstore.bundle.v0.3+json",
            "verificationMaterial": {"publicKey": {}},
            "dsseEnvelope": {
                "payload": encode(statement.as_bytes()),
                "payloadType": "application/vnd.in-toto+json",
                "signatures": [],
            },
        });
        let st = bundle_statement(&bundle.to_string()).unwrap();
        assert_eq!(st["predicate"]["lockfile_digest"], "abc");
        assert_eq!(st["subject"][0]["digest"]["sha256"], "982b");
        assert!(bundle_statement("{}")
            .unwrap_err()
            .contains("no DSSE envelope"));
    }

    /// Test-only base64 encoder, to build bundles.
    fn encode(data: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for c in data.chunks(3) {
            let n = (c[0] as u32) << 16
                | (*c.get(1).unwrap_or(&0) as u32) << 8
                | *c.get(2).unwrap_or(&0) as u32;
            for i in 0..4 {
                if i <= c.len() {
                    out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    #[test]
    fn base64_round_trips_all_lengths() {
        for s in ["", "a", "ab", "abc", "abcd", "hello binary\n"] {
            assert_eq!(base64_decode(&encode(s.as_bytes())).unwrap(), s.as_bytes());
        }
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
