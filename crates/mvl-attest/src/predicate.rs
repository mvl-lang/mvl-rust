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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_without_auditable_data_has_no_predicate() {
        let exe = std::env::current_exe().unwrap();
        assert!(generate(&exe, "Cargo.lock")
            .unwrap_err()
            .contains("no cargo-auditable data"));
    }
}
