//! The build-environment predicate (`https://mvl-lang.org/attest/build-env/v1`).
//!
//! These are the values ADR-0013 decision 6 keeps out of the binary because
//! they depend on the build environment, not on the build inputs. CI signs
//! them with the same workflow identity as the SLSA provenance.

use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};

fn first_line(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .map(str::to_string)
        })
        .unwrap_or_default()
}

/// The URL of the current GitHub Actions run, if running in one.
fn run_url() -> String {
    let v = |k: &str| std::env::var(k).unwrap_or_default();
    if v("GITHUB_RUN_ID").is_empty() {
        return String::new();
    }
    format!(
        "{}/{}/actions/runs/{}/attempts/{}",
        v("GITHUB_SERVER_URL"),
        v("GITHUB_REPOSITORY"),
        v("GITHUB_RUN_ID"),
        v("GITHUB_RUN_ATTEMPT")
    )
}

/// Build the predicate for `binary`. `lockfile` is the repository-relative
/// path of the lockfile the binary was built from (read from the working
/// directory).
pub fn generate(binary: &Path, lockfile: &str) -> Result<Value, String> {
    let audit = crate::auditable::read(binary)?;
    let lockfile_digest = crate::sha256_file(Path::new(lockfile))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| mvl_manifest::time::rfc3339_utc(d.as_secs() as i64))
        .unwrap_or_default();
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    Ok(json!({
        "lockfile_path": lockfile,
        "lockfile_digest": lockfile_digest,
        "auditable_digest": audit.digest,
        "rustc": first_line(&rustc, &["-V"]),
        "host_os": first_line("uname", &["-srm"]),
        "build_time": now,
        "run_url": run_url(),
    }))
}

/// Inputs for an SLSA predicate built by mvl-attest itself (key/KMS mode,
/// where `actions/attest-build-provenance` cannot sign). Unset fields come
/// from the GitHub Actions environment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlsaParams {
    /// `owner/repo`.
    pub repo: Option<String>,
    /// Workflow path, e.g. `.github/workflows/release.yml`.
    pub workflow: Option<String>,
    /// Source ref, e.g. `refs/tags/v1.0.0`.
    pub git_ref: Option<String>,
    /// Source commit.
    pub commit: Option<String>,
}

/// `GITHUB_WORKFLOW_REF` (`owner/repo/<path>@<ref>`) → `<path>`.
fn workflow_path(workflow_ref: &str, repo: &str) -> String {
    let w = workflow_ref.split('@').next().unwrap_or("");
    w.strip_prefix(&format!("{repo}/")).unwrap_or(w).to_string()
}

/// SLSA v1 provenance in the shape GitHub's own uses, so the same
/// cross-checks read it. Signed with the KMS key, the identity fields are the
/// key holder's claim, not a CI-platform certificate (ADR-0014 amendment).
pub fn slsa(p: &SlsaParams) -> Result<Value, String> {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let need = |v: Option<String>, what: &str| {
        v.ok_or(format!("{what} unknown: pass it, or run in GitHub Actions"))
    };
    let repo = need(
        p.repo.clone().or_else(|| env("GITHUB_REPOSITORY")),
        "--repo",
    )?;
    let workflow = need(
        p.workflow
            .clone()
            .or_else(|| env("GITHUB_WORKFLOW_REF").map(|w| workflow_path(&w, &repo))),
        "--workflow",
    )?;
    let git_ref = need(p.git_ref.clone().or_else(|| env("GITHUB_REF")), "--ref")?;
    let commit = need(p.commit.clone().or_else(|| env("GITHUB_SHA")), "--commit")?;
    let server = env("GITHUB_SERVER_URL").unwrap_or_else(|| "https://github.com".into());
    let repo_uri = format!("{server}/{repo}");
    Ok(json!({
        "buildDefinition": {
            "buildType": "https://mvl-lang.org/attest/buildtypes/cargo/v1",
            "externalParameters": {"workflow": {
                "repository": repo_uri,
                "path": workflow,
                "ref": git_ref,
            }},
            "resolvedDependencies": [{
                "uri": format!("git+{repo_uri}@{git_ref}"),
                "digest": {"gitCommit": commit},
            }],
        },
        "runDetails": {
            "builder": {"id": format!("{repo_uri}/{workflow}@{git_ref}")},
            "metadata": {"invocationId": run_url()},
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slsa_predicate_round_trips_through_the_cross_check_reader() {
        let p = SlsaParams {
            repo: Some("o/r".into()),
            workflow: Some(".github/workflows/release.yml".into()),
            git_ref: Some("refs/tags/v1.0.0".into()),
            commit: Some("c0ffee".into()),
        };
        let pred = slsa(&p).unwrap();
        let st = json!({"subject": [{"digest": {"sha256": "ab"}}], "predicate": pred});
        let v = crate::provenance::from_statement(&st, None, "ab").unwrap();
        assert_eq!(v.repo_uri, "https://github.com/o/r");
        assert_eq!(v.commit, "c0ffee");
        assert_eq!(
            v.signer_uri,
            "https://github.com/o/r/.github/workflows/release.yml@refs/tags/v1.0.0"
        );
    }

    #[test]
    fn workflow_path_from_github_workflow_ref() {
        assert_eq!(
            workflow_path("o/r/.github/workflows/attest.yml@refs/pull/7/merge", "o/r"),
            ".github/workflows/attest.yml"
        );
    }

    #[test]
    fn binary_without_auditable_data_has_no_predicate() {
        let exe = std::env::current_exe().unwrap();
        assert!(generate(&exe, "Cargo.lock")
            .unwrap_err()
            .contains("no cargo-auditable data"));
    }
}
