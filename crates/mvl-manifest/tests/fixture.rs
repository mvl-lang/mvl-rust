//! End-to-end tests: a fixture workspace whose `build.rs` calls
//! `mvl_manifest::build::generate()`, built with the real cargo.
//!
//! The fixture has a registry dep (`log`), a path dep, a git dep served from a
//! local repository, a path proc-macro, a path build-dependency, a `[patch]`
//! and a `.cargo/config.toml` `paths` override. Its binary prints
//! `manifest_to_json(&manifest!())`, or logs the manifest when given `log`.

#![cfg(all(feature = "build", feature = "log"))]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

/// Cargo's per-package scratch space for integration tests (under target/).
const TMP: &str = env!("CARGO_TARGET_TMPDIR");

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(TMP).join(format!("fixture-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Target dir shared by tests that do not need a clean build; kept across
/// runs so the fixture's dependencies compile once.
fn shared_target() -> PathBuf {
    Path::new(TMP).join("fixture-target")
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn check(out: Output, what: &str) -> Output {
    assert!(
        out.status.success(),
        "{what} failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = check(
        Command::new("git")
            .current_dir(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
            .args(args)
            // Fixed dates keep commits identical across runs and clones.
            .env("GIT_AUTHOR_DATE", "2026-11-14T00:00:00Z")
            .env("GIT_COMMITTER_DATE", "2026-11-14T00:00:00Z")
            .output()
            .unwrap(),
        &format!("git {args:?}"),
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn cargo(dir: &Path) -> Command {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(dir);
    // Do not inherit the outer test build's flags or wrappers.
    for v in [
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_BUILD_RUSTFLAGS",
        "SOURCE_DATE_EPOCH",
        "CARGO_TARGET_DIR",
        "CARGO_BUILD_TARGET_DIR",
    ] {
        c.env_remove(v);
    }
    c
}

fn crate_toml(name: &str, extra: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{extra}")
}

/// A local git repository holding crate `gitdep`; returns its `file://` URL.
fn git_repo(root: &Path) -> String {
    let repo = root.join("gitdep-repo");
    write(&repo.join("Cargo.toml"), &crate_toml("gitdep", ""));
    write(&repo.join("src/lib.rs"), "pub fn g() -> u32 { 7 }\n");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "gitdep"]);
    format!("file://{}", repo.display())
}

/// Create the fixture as a committed git repository at `dir`.
fn fixture(dir: &Path, git_url: &str) {
    let mm = Path::new(MANIFEST_DIR).display().to_string();
    write(
        &dir.join("Cargo.toml"),
        &format!(
            r#"[package]
name = "app"
version = "0.1.0"
edition = "2021"
publish = false

[workspace]

[features]
default = ["fancy"]
fancy = []
other-thing = []

[dependencies]
log = "0.4"
pathdep = {{ path = "pathdep" }}
derive = {{ path = "derive" }}
gitdep = {{ git = "{git_url}" }}
mvl-manifest = {{ path = "{mm}", features = ["log"] }}

[build-dependencies]
builddep = {{ path = "builddep" }}
mvl-manifest = {{ path = "{mm}", features = ["build"] }}

[patch.crates-io]
unused-patched = {{ path = "vendor/unused-patched" }}
"#
        ),
    );
    write(
        &dir.join("build.rs"),
        "fn main() {\n    builddep::touch();\n    mvl_manifest::build::generate().expect(\"mvl manifest\");\n}\n",
    );
    write(
        &dir.join("src/main.rs"),
        r#"const MANIFEST: mvl_manifest::Manifest = mvl_manifest::manifest!();

fn main() {
    let _ = (pathdep::p(), gitdep::g(), log::max_level());
    if std::env::args().nth(1).as_deref() == Some("log") {
        mvl_manifest::log_manifest(&MANIFEST);
    } else {
        println!("{}", mvl_manifest::manifest_to_json(&MANIFEST));
    }
}
"#,
    );
    write(&dir.join("pathdep/Cargo.toml"), &crate_toml("pathdep", ""));
    write(&dir.join("pathdep/src/lib.rs"), "pub fn p() -> u32 { 1 }\n");
    write(
        &dir.join("derive/Cargo.toml"),
        &crate_toml("derive", "[lib]\nproc-macro = true\n"),
    );
    write(&dir.join("derive/src/lib.rs"), "");
    write(
        &dir.join("builddep/Cargo.toml"),
        &crate_toml("builddep", ""),
    );
    write(&dir.join("builddep/src/lib.rs"), "pub fn touch() {}\n");
    write(
        &dir.join("vendor/unused-patched/Cargo.toml"),
        &crate_toml("unused-patched", ""),
    );
    write(&dir.join("vendor/unused-patched/src/lib.rs"), "");
    write(
        &dir.join("overrides/override-me/Cargo.toml"),
        &crate_toml("override-me", ""),
    );
    write(&dir.join("overrides/override-me/src/lib.rs"), "");
    write(
        &dir.join(".cargo/config.toml"),
        "paths = [\"overrides/override-me\"]\n",
    );
    write(&dir.join(".gitignore"), "/target\n");
    // Seed the lockfile with this workspace's, so the fixture resolves to
    // crates already in the local cache.
    fs::copy(
        Path::new(MANIFEST_DIR).join("../../Cargo.lock"),
        dir.join("Cargo.lock"),
    )
    .unwrap();
    check(cargo(dir).arg("fetch").output().unwrap(), "cargo fetch");
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "fixture"]);
}

fn build(dir: &Path, target: &Path, extra: &[&str], envs: &[(&str, &str)]) -> PathBuf {
    let mut c = cargo(dir);
    c.args(["build", "--offline", "--quiet"])
        .args(extra)
        .env("CARGO_TARGET_DIR", target);
    for (k, v) in envs {
        c.env(k, v);
    }
    check(c.output().unwrap(), "cargo build");
    let profile = if extra.contains(&"--release") {
        "release"
    } else {
        "debug"
    };
    target
        .join(profile)
        .join(format!("app{}", std::env::consts::EXE_SUFFIX))
}

fn manifest(bin: &Path) -> Value {
    let out = check(Command::new(bin).output().unwrap(), "fixture binary");
    serde_json::from_slice(&out.stdout).expect("manifest JSON")
}

fn pkg<'a>(list: &'a Value, name: &str) -> &'a Value {
    list.as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == name)
        .unwrap_or_else(|| panic!("{name} missing from {list}"))
}

fn names(list: &Value) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn fixture_packages_sources_and_substitutions() {
    let root = scratch("graph");
    let url = git_repo(&root);
    let dir = root.join("app");
    fixture(&dir, &url);
    let m = manifest(&build(&dir, &shared_target(), &[], &[]));

    // App identity and features.
    assert_eq!(m["app"], "app");
    assert_eq!(m["app_version"], "0.1.0");
    assert_eq!(m["backend"], "mvl-rust");
    assert_eq!(
        m["build"]["features"],
        serde_json::json!(["default", "fancy"])
    );
    assert_eq!(m["build"]["profile"], "debug");

    // Runtime vs build packages.
    let runtime = names(&m["packages"]);
    for n in ["log", "pathdep", "gitdep", "mvl-manifest"] {
        assert!(runtime.contains(&n.to_string()), "{n} not in {runtime:?}");
    }
    for n in ["derive", "builddep", "sha2", "toml", "serde_json"] {
        assert!(
            !runtime.contains(&n.to_string()),
            "{n} leaked into runtime: {runtime:?}"
        );
    }
    let build = &m["build_packages"];
    assert_eq!(pkg(build, "derive")["kind"], "proc-macro");
    assert_eq!(pkg(build, "builddep")["kind"], "build");
    // The generator's own deps run only at build time.
    assert_eq!(pkg(build, "sha2")["kind"], "build");

    // Registry: purl, registry URL and Cargo.lock checksum.
    let log = pkg(&m["packages"], "log");
    assert_eq!(log["kind"], "runtime");
    assert_eq!(log["source"]["kind"], "registry");
    assert_eq!(log["checksum"].as_str().unwrap().len(), 64);
    assert_eq!(
        log["purl"],
        format!("pkg:cargo/log@{}", log["version"].as_str().unwrap())
    );

    // Path: no checksum (covered by source_digest).
    let p = pkg(&m["packages"], "pathdep");
    assert_eq!(p["source"]["kind"], "path");
    assert_eq!(p["checksum"], "");

    // Git: resolved commit, vcs_url purl.
    let g = pkg(&m["packages"], "gitdep");
    assert_eq!(g["source"]["kind"], "git");
    let commit = git(&root.join("gitdep-repo"), &["rev-parse", "HEAD"]);
    assert_eq!(g["source"]["commit"], commit.as_str());
    assert!(g["purl"]
        .as_str()
        .unwrap()
        .starts_with("pkg:cargo/gitdep@0.1.0?vcs_url=git%2Bfile://"));

    // Substitutions.
    let subs: Vec<&str> = m["build"]["substitutions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect();
    assert!(
        subs.contains(&"Cargo.toml: patch.crates-io.unused-patched -> path=vendor/unused-patched"),
        "{subs:?}"
    );
    assert!(
        subs.contains(&"workspace/.cargo/config.toml: paths -> overrides/override-me"),
        "{subs:?}"
    );

    // Provenance.
    assert_eq!(
        m["build"]["vcs_commit"],
        git(&dir, &["rev-parse", "HEAD"]).as_str()
    );
    assert_eq!(m["build"]["vcs_dirty"], false);
    assert_eq!(
        m["built"], "2026-11-14T00:00:00Z",
        "git commit time, not wall clock"
    );
    assert_eq!(m["source_digest"].as_str().unwrap().len(), 64);
    assert_eq!(m["build"]["lockfile_digest"].as_str().unwrap().len(), 64);
    assert_eq!(m["build"]["reproducible"], false);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn staleness_rebuild_picks_up_source_commit_lockfile_and_config_changes() {
    let root = scratch("stale");
    let url = git_repo(&root);
    let dir = root.join("app");
    fixture(&dir, &url);
    let target = shared_target();
    let m0 = manifest(&build(&dir, &target, &[], &[]));
    let digest = |m: &Value| m["source_digest"].as_str().unwrap().to_string();

    // 1. Editing a source file changes source_digest (and dirties the tree).
    let main = dir.join("src/main.rs");
    let mut src = fs::read_to_string(&main).unwrap();
    src.push_str("// edited\n");
    fs::write(&main, src).unwrap();
    let m1 = manifest(&build(&dir, &target, &[], &[]));
    assert_ne!(digest(&m0), digest(&m1), "source edit not picked up");
    assert_eq!(m1["build"]["vcs_dirty"], true);

    // 2. Committing changes vcs_commit and cleans the tree.
    git(&dir, &["commit", "-q", "-am", "edit"]);
    let m2 = manifest(&build(&dir, &target, &[], &[]));
    assert_ne!(m1["build"]["vcs_commit"], m2["build"]["vcs_commit"]);
    assert_eq!(
        m2["build"]["vcs_commit"],
        git(&dir, &["rev-parse", "HEAD"]).as_str()
    );
    assert_eq!(m2["build"]["vcs_dirty"], false);

    // 3. Changing Cargo.lock changes lockfile_digest and source_digest.
    // Add a local path dependency: Cargo.lock changes, and no download is needed.
    let cargo_toml = dir.join("Cargo.toml");
    let text = fs::read_to_string(&cargo_toml).unwrap();
    fs::write(
        &cargo_toml,
        text.replace(
            "[dependencies]\n",
            "[dependencies]\nextra2 = { path = \"extra2\" }\n",
        ),
    )
    .unwrap();
    write(&dir.join("extra2/Cargo.toml"), &crate_toml("extra2", ""));
    write(&dir.join("extra2/src/lib.rs"), "");
    let m3 = manifest(&build(&dir, &target, &[], &[]));
    assert_ne!(
        m2["build"]["lockfile_digest"],
        m3["build"]["lockfile_digest"]
    );
    assert_ne!(digest(&m2), digest(&m3));
    assert!(names(&m3["packages"]).contains(&"extra2".to_string()));

    // 4. Changing .cargo/config.toml changes source_digest.
    let cfg = dir.join(".cargo/config.toml");
    let mut cfg_text = fs::read_to_string(&cfg).unwrap();
    cfg_text.push_str("# changed\n");
    fs::write(&cfg, cfg_text).unwrap();
    let m4 = manifest(&build(&dir, &target, &["--locked"], &[]));
    assert_ne!(digest(&m3), digest(&m4), "config change not picked up");

    // 5. An unchanged rebuild is stable.
    let m5 = manifest(&build(&dir, &target, &["--locked"], &[]));
    assert_eq!(m4, m5);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn reproducible_builds_from_different_directories_are_byte_identical() {
    let root = scratch("repro");
    let url = git_repo(&root);
    let origin = root.join("origin");
    fixture(&origin, &url);

    let mut outputs = Vec::new();
    for name in ["checkout-a", "somewhere/else/checkout-b"] {
        let dir = root.join(name);
        fs::create_dir_all(dir.parent().unwrap()).unwrap();
        check(
            Command::new("git")
                .args(["clone", "-q"])
                .arg(&origin)
                .arg(&dir)
                .output()
                .unwrap(),
            "git clone",
        );
        let flags = format!("--remap-path-prefix={}=/build", dir.display());
        let bin = build(
            &dir,
            &dir.join("target"),
            &["--release", "--locked"],
            &[("SOURCE_DATE_EPOCH", "1794614400"), ("RUSTFLAGS", &flags)],
        );
        let m = manifest(&bin);
        assert_eq!(m["build"]["reproducible"], true);
        assert_eq!(m["build"]["vcs_dirty"], false);
        assert_eq!(m["built"], "2026-11-14T00:00:00Z");
        outputs.push((m, fs::read(&bin).unwrap()));
    }
    assert_eq!(outputs[0].0, outputs[1].0, "manifests differ");
    assert!(outputs[0].1 == outputs[1].1, "binaries differ");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn fixture_binary_falls_back_to_stderr_without_a_logger() {
    let root = scratch("fallback");
    let url = git_repo(&root);
    let dir = root.join("app");
    fixture(&dir, &url);
    let bin = build(&dir, &shared_target(), &[], &[]);
    let out = check(
        Command::new(&bin).arg("log").output().unwrap(),
        "fixture log",
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.starts_with("INFO mvl_manifest: MVL Runtime Manifest app=app app_version=0.1.0 "),
        "{stderr}"
    );
    for title in [
        "MVL Build Provenance",
        "MVL Runtime Packages",
        "MVL Build Packages",
        "MVL Runtime Environment",
    ] {
        assert!(stderr.contains(title), "{title} missing:\n{stderr}");
    }
    // A debug build warns about nothing but what applies to every build.
    assert!(!stderr.contains("not_auditable"), "{stderr}");

    let _ = fs::remove_dir_all(&root);
}
