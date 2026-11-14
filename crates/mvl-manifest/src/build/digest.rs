//! `source_digest`: one SHA-256 over every build input on disk.
//!
//! Each file contributes `<key>\0<sha256 hex>\n`, sorted by key. Keys are
//! location-independent (`<package>/<path in package>`, `workspace/…`,
//! `cargo-home/…`), so the same sources give the same digest wherever they
//! are checked out.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use super::graph::LocalPackage;
use super::{run, Error, Result, Tracker};

pub fn file_sha256(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(format!("{:x}", Sha256::digest(&bytes)))
}

/// Files `cargo package --list` reports that Cargo generates rather than reads.
fn is_generated(rel: &str) -> bool {
    matches!(
        rel,
        "Cargo.lock" | "Cargo.toml.orig" | ".cargo_vcs_info.json"
    )
}

/// Files Cargo would package for `pkg`, as paths relative to its directory.
fn package_files(cargo: &str, pkg: &LocalPackage) -> Result<Vec<String>> {
    let out = run(Command::new(cargo)
        .args(["package", "--list", "--offline", "--allow-dirty"])
        .arg("--manifest-path")
        .arg(&pkg.manifest_path))
    .map_err(|e| Error::new(format!("listing files of {}: {e}", pkg.name)))?;
    Ok(out
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !is_generated(l))
        .map(str::to_string)
        .collect())
}

/// Digest a sorted `key → file hash` map.
pub fn combine(entries: &BTreeMap<String, String>) -> String {
    let mut h = Sha256::new();
    for (k, v) in entries {
        h.update(k.as_bytes());
        h.update([0u8]);
        h.update(v.as_bytes());
        h.update(b"\n");
    }
    format!("{:x}", h.finalize())
}

/// The first directory component of `rel`, if it is inside a subdirectory.
fn top_dir(rel: &str) -> Option<String> {
    let mut comps = Path::new(rel).components();
    match (comps.next(), comps.next()) {
        (Some(Component::Normal(d)), Some(_)) => Some(d.to_string_lossy().into_owned()),
        _ => None,
    }
}

pub fn source_digest(
    t: &mut Tracker,
    cargo: &str,
    packages: &[LocalPackage],
    extra: &[(String, PathBuf)],
) -> Result<String> {
    let mut entries = BTreeMap::new();
    for pkg in packages {
        let dir = pkg.manifest_path.parent().unwrap_or(Path::new("."));
        for rel in package_files(cargo, pkg)? {
            let path = dir.join(&rel);
            let Some(hash) = file_sha256(&path) else {
                continue;
            };
            t.file(&path);
            // Watching the containing top-level directory (src/, tests/, …)
            // makes Cargo notice files added there, not only edits to known
            // ones. The package root itself is not watched: it usually holds
            // target/, which changes on every build.
            if let Some(d) = top_dir(&rel) {
                t.file(&dir.join(d));
            }
            entries.insert(format!("{}/{rel}", pkg.name), hash);
        }
    }
    for (key, path) in extra {
        if let Some(hash) = file_sha256(path) {
            t.file(path);
            entries.insert(key.clone(), hash);
        }
    }
    Ok(combine(&entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_depends_on_keys_and_contents_not_order() {
        let a: BTreeMap<String, String> = [
            ("p/src/a.rs".into(), "1".into()),
            ("p/src/b.rs".into(), "2".into()),
        ]
        .into();
        let b: BTreeMap<String, String> = [
            ("p/src/b.rs".into(), "2".into()),
            ("p/src/a.rs".into(), "1".into()),
        ]
        .into();
        assert_eq!(combine(&a), combine(&b));
        let c: BTreeMap<String, String> = [
            ("p/src/a.rs".into(), "1".into()),
            ("p/src/b.rs".into(), "3".into()),
        ]
        .into();
        assert_ne!(combine(&a), combine(&c));
    }

    #[test]
    fn generated_package_entries_are_skipped() {
        assert!(is_generated("Cargo.toml.orig"));
        assert!(is_generated("Cargo.lock"));
        assert!(!is_generated("Cargo.toml"));
        assert!(!is_generated("src/main.rs"));
    }

    #[test]
    fn top_dir_only_for_nested_files() {
        assert_eq!(top_dir("src/main.rs").as_deref(), Some("src"));
        assert_eq!(top_dir("build.rs"), None);
    }
}
