//! The `Cargo.lock` at the signed commit.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use crate::auditable::Dep;

/// Map a `Cargo.lock` `source` to cargo-auditable's source names.
pub fn source_kind(source: Option<&str>) -> &'static str {
    match source {
        None => "local",
        Some(s)
            if s == "registry+https://github.com/rust-lang/crates.io-index"
                || s.starts_with("sparse+https://index.crates.io") =>
        {
            "crates.io"
        }
        Some(s) if s.starts_with("git+") => "git",
        Some(_) => "registry",
    }
}

/// Every package in a lockfile, as `(name, version, source kind)`.
pub fn packages(text: &str) -> Result<BTreeSet<Dep>, String> {
    let t: toml::Table = text
        .parse()
        .map_err(|e| format!("Cargo.lock does not parse: {e}"))?;
    Ok(t.get("package")
        .and_then(|p| p.as_array())
        .into_iter()
        .flatten()
        .map(|p| {
            let s = |k: &str| p.get(k).and_then(|v| v.as_str());
            Dep {
                name: s("name").unwrap_or("").to_string(),
                version: s("version").unwrap_or("").to_string(),
                source: source_kind(s("source")).to_string(),
            }
        })
        .collect())
}

/// The lockfile text at `commit`: from a local checkout (`git show`) when one
/// is given, otherwise from the GitHub API.
pub fn at_commit(
    source: Option<&Path>,
    gh: &Path,
    repo: &str,
    commit: &str,
    path: &str,
) -> Result<String, String> {
    let out = match source {
        Some(dir) => Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["show", &format!("{commit}:{path}")])
            .output(),
        None => Command::new(gh)
            .args([
                "api",
                "-H",
                "Accept: application/vnd.github.raw",
                &format!("repos/{repo}/contents/{path}?ref={commit}"),
            ])
            .output(),
    }
    .map_err(|e| format!("cannot fetch {path} at {commit}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cannot fetch {path} at {commit}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    String::from_utf8(out.stdout).map_err(|_| format!("{path} at {commit} is not UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_lockfile_sources_to_auditable_names() {
        assert_eq!(source_kind(None), "local");
        assert_eq!(
            source_kind(Some(
                "registry+https://github.com/rust-lang/crates.io-index"
            )),
            "crates.io"
        );
        assert_eq!(
            source_kind(Some("sparse+https://index.crates.io/")),
            "crates.io"
        );
        assert_eq!(source_kind(Some("git+https://example.com/g#abc")), "git");
        assert_eq!(source_kind(Some("sparse+https://my.registry/")), "registry");
    }

    #[test]
    fn parses_lockfile_packages() {
        let lock = r#"
version = 4
[[package]]
name = "demo"
version = "0.1.0"
[[package]]
name = "log"
version = "0.4.33"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "abc"
"#;
        let p = packages(lock).unwrap();
        assert!(p.contains(&Dep {
            name: "log".into(),
            version: "0.4.33".into(),
            source: "crates.io".into()
        }));
        assert!(p.contains(&Dep {
            name: "demo".into(),
            version: "0.1.0".into(),
            source: "local".into()
        }));
    }
}
