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

/// The files of `pkg` that go into the digest, relative to its directory.
/// Workspace members: exactly what Cargo would package. Path dependencies
/// outside the workspace: their tracked and untracked-but-not-ignored files
/// (`git ls-files`), or a directory walk without git; always the same method
/// for the same package, so the digest stays deterministic.
fn package_files(cargo: &str, pkg: &LocalPackage) -> Result<Vec<String>> {
    if pkg.member {
        return cargo_package_files(cargo, pkg);
    }
    let dir = pkg.manifest_path.parent().unwrap_or(Path::new("."));
    Ok(tree_files(dir))
}

/// `git ls-files` under `dir`, or a walk when `dir` is not in a git checkout.
/// `target/` and hidden entries are skipped either way.
fn tree_files(dir: &Path) -> Vec<String> {
    let git = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .output()
        .ok()
        .filter(|o| o.status.success());
    let mut files: Vec<String> = match git {
        Some(o) => String::from_utf8_lossy(&o.stdout)
            .split('\0')
            .filter(|f| !f.is_empty())
            .map(str::to_string)
            .collect(),
        None => {
            let mut out = Vec::new();
            walk(dir, Path::new(""), &mut out);
            out
        }
    };
    files.retain(|f| {
        !Path::new(f).components().any(|c| {
            let c = c.as_os_str().to_string_lossy();
            c == "target" || c.starts_with('.')
        })
    });
    files.sort();
    files.dedup();
    files
}

fn walk(root: &Path, rel: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(root.join(rel)) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "target" {
            continue;
        }
        let path = rel.join(&name);
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(root, &path, out),
            Ok(t) if t.is_file() => out.push(path.to_string_lossy().into_owned()),
            _ => {}
        }
    }
}

/// What `cargo package --list` reports, minus files Cargo generates.
fn cargo_package_files(cargo: &str, pkg: &LocalPackage) -> Result<Vec<String>> {
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

    fn scratch(name: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("mvl-manifest-digest-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for f in ["Cargo.toml", "src/lib.rs", "target/debug/junk", ".hidden/x"] {
            let p = d.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, f).unwrap();
        }
        d
    }

    #[test]
    fn walk_skips_target_and_hidden_entries() {
        let d = scratch("walk");
        let mut out = Vec::new();
        walk(&d, Path::new(""), &mut out);
        out.sort();
        assert_eq!(out, ["Cargo.toml", "src/lib.rs"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn git_listing_respects_ignore_and_skips_target() {
        let d = scratch("git");
        std::fs::write(d.join(".gitignore"), "ignored.txt\n").unwrap();
        std::fs::write(d.join("ignored.txt"), "x").unwrap();
        std::fs::write(d.join("untracked.rs"), "x").unwrap();
        let ok = Command::new("git")
            .arg("-C")
            .arg(&d)
            .args(["init", "-q"])
            .status()
            .is_ok_and(|s| s.success());
        if ok {
            let _ = Command::new("git")
                .arg("-C")
                .arg(&d)
                .args(["add", "Cargo.toml", "src/lib.rs"])
                .status();
            assert_eq!(tree_files(&d), ["Cargo.toml", "src/lib.rs", "untracked.rs"]);
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn top_dir_only_for_nested_files() {
        assert_eq!(top_dir("src/main.rs").as_deref(), Some("src"));
        assert_eq!(top_dir("build.rs"), None);
    }
}
