//! Git provenance. Empty fields when there is no git or no repository.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::Tracker;

#[derive(Debug, Default)]
pub struct Vcs {
    pub commit: String,
    pub dirty: bool,
    pub describe: String,
    pub commit_time: Option<i64>,
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn probe(t: &mut Tracker, dir: &Path) -> Vcs {
    let Some(commit) = git(dir, &["rev-parse", "HEAD"]) else {
        return Vcs::default();
    };
    // Rerun when HEAD moves, the branch ref moves, or the index changes
    // (commits, checkouts, staging).
    if let Some(git_dir) = git(dir, &["rev-parse", "--absolute-git-dir"]) {
        let git_dir = PathBuf::from(git_dir);
        t.file(&git_dir.join("HEAD"));
        t.file(&git_dir.join("index"));
        let common = git(dir, &["rev-parse", "--git-common-dir"])
            .map(|c| {
                let p = PathBuf::from(c);
                if p.is_absolute() {
                    p
                } else {
                    dir.join(p)
                }
            })
            .unwrap_or(git_dir);
        if let Some(r) = git(dir, &["symbolic-ref", "-q", "HEAD"]) {
            t.file(&common.join(r));
        }
        t.file(&common.join("packed-refs"));
    }
    let dirty = git(dir, &["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
    Vcs {
        describe: git(dir, &["describe", "--always", "--tags", "--dirty"]).unwrap_or_default(),
        commit_time: git(dir, &["log", "-1", "--format=%ct"]).and_then(|s| s.parse().ok()),
        commit,
        dirty,
    }
}
