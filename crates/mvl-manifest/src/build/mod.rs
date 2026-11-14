//! The `build.rs` generator (feature `build`, ADR-0013 decision 2).
//!
//! ```no_run
//! // in build.rs `main()`:
//! mvl_manifest::build::generate().expect("mvl manifest");
//! ```
//!
//! Writes `$OUT_DIR/mvl_manifest.rs`, read back by [`crate::manifest!`].
//! Every embedded value is determined by the build inputs (decision 6), and
//! every input read is announced through `cargo:rerun-if-changed` /
//! `cargo:rerun-if-env-changed`. Emitting any `rerun-if-changed` stops Cargo
//! from re-running the script on other changes, so the list has to be complete.
//!
//! Known limits, all covered exactly by `cargo auditable` (decision 3):
//! `cargo metadata` approximates the real build graph (no cross-member
//! feature unification; build-deps are filtered for the target platform);
//! `.cargo/config.toml` files are discovered upward from the crate directory,
//! not from the directory Cargo was invoked in; `--config` CLI overrides are
//! invisible to build scripts.

mod codegen;
mod config;
mod digest;
mod graph;
mod rustflags;
mod toolchain;
mod vcs;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

pub use graph::{OwnedPackage, OwnedSource};

/// A generator failure. The message names the step that failed.
#[derive(Debug)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "mvl-manifest: {}", self.0)
    }
}

impl std::error::Error for Error {}

impl Error {
    pub(crate) fn new(msg: impl Into<String>) -> Self {
        Error(msg.into())
    }
}

pub(crate) type Result<T> = std::result::Result<T, Error>;

/// Owned build info, rendered into the `const` manifest by `codegen`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnedBuild {
    pub rustc_version: Option<String>,
    pub llvm_version: Option<String>,
    pub target: String,
    pub profile: String,
    pub date: String,
    pub host: String,
    pub rustc_commit: String,
    pub rustc_channel: String,
    pub cargo_version: String,
    pub cc_version: String,
    pub linker: String,
    pub opt_level: String,
    pub panic: String,
    pub target_features: Vec<String>,
    pub overflow_checks: bool,
    pub profile_overrides: Vec<String>,
    pub features: Vec<String>,
    pub rustflags: Vec<String>,
    pub rustc_wrapper: Vec<String>,
    pub substitutions: Vec<String>,
    pub vcs_commit: String,
    pub vcs_dirty: bool,
    pub vcs_describe: String,
    pub lockfile_digest: String,
    pub reproducible: bool,
    pub auditable: bool,
}

/// Owned manifest, the generator's output model.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnedManifest {
    pub app_name: String,
    pub app_version: String,
    pub mvl_version: String,
    pub runtime_version: String,
    pub packages: Vec<OwnedPackage>,
    pub build_packages: Vec<OwnedPackage>,
    pub build: OwnedBuild,
    pub source_digest: String,
}

/// Records what the generator read, so it can emit a complete rerun list.
#[derive(Default)]
pub(crate) struct Tracker {
    files: BTreeSet<PathBuf>,
    envs: BTreeSet<String>,
    /// Test-only replacement for the process environment.
    fixed_env: Option<BTreeMap<String, String>>,
}

impl Tracker {
    pub(crate) fn env(&mut self, name: &str) -> Option<String> {
        self.envs.insert(name.to_string());
        let v = match &self.fixed_env {
            Some(m) => m.get(name).cloned(),
            None => std::env::var(name).ok(),
        };
        v.filter(|v| !v.is_empty())
    }

    /// Every `CARGO_FEATURE_*` variable. Cargo fingerprints features itself,
    /// so these need no rerun directive.
    fn feature_vars(&self) -> Vec<(String, String)> {
        let all: Vec<(String, String)> = match &self.fixed_env {
            Some(m) => m.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            None => std::env::vars().collect(),
        };
        all.into_iter()
            .filter(|(k, _)| k.starts_with("CARGO_FEATURE_"))
            .collect()
    }

    pub(crate) fn file(&mut self, path: &Path) {
        if path.exists() {
            self.files.insert(path.to_path_buf());
        }
    }

    fn emit(&self) -> String {
        let mut s = String::new();
        for f in &self.files {
            s.push_str(&format!("cargo:rerun-if-changed={}\n", f.display()));
        }
        for e in &self.envs {
            s.push_str(&format!("cargo:rerun-if-env-changed={e}\n"));
        }
        s
    }
}

fn required(t: &mut Tracker, name: &str) -> Result<String> {
    t.env(name)
        .ok_or_else(|| Error::new(format!("{name} is not set; call generate() from build.rs")))
}

pub(crate) fn run(cmd: &mut Command) -> Result<String> {
    let out = cmd
        .output()
        .map_err(|e| Error::new(format!("cannot run {cmd:?}: {e}")))?;
    if !out.status.success() {
        return Err(Error::new(format!(
            "{cmd:?} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Profile directory name from `OUT_DIR`. `PROFILE` only says debug or release;
/// the directory carries custom names.
///
/// Two layouts exist: `<profile>/build/<pkg>-<hash>/out` (older cargo) and
/// `<profile>/build/<pkg>/<hash>/out` (cargo 1.100+). The profile is the parent
/// of the `build` directory, which sits 2 to 4 levels above `out`. The highest
/// match wins, so a package named `build` does not hide the real one.
pub(crate) fn profile_dir_from_out_dir(out_dir: &Path) -> Option<String> {
    out_dir
        .ancestors()
        .enumerate()
        .filter(|(depth, p)| (2..=4).contains(depth) && p.file_name().is_some_and(|n| n == "build"))
        .last()
        .and_then(|(_, build)| build.parent())
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().into_owned())
}

/// The Rust runtime is the standard library, which ships with the compiler:
/// its version is the rustc release (`std-1.99.1`, `std-1.100.0-nightly`).
pub(crate) fn std_runtime(rustc_release: Option<&str>) -> String {
    rustc_release
        .map(|r| format!("std-{r}"))
        .unwrap_or_default()
}

/// Generate `$OUT_DIR/mvl_manifest.rs` and print the rerun directives.
pub fn generate() -> std::result::Result<(), Error> {
    let directives = generate_with(Tracker::default())?;
    print!("{directives}");
    Ok(())
}

/// [`generate`] without printing; returns the rerun directives.
fn generate_with(mut t: Tracker) -> Result<String> {
    let manifest = collect(&mut t)?;
    let out_dir = PathBuf::from(required(&mut t, "OUT_DIR")?);
    let src = codegen::render(&manifest);
    std::fs::write(out_dir.join("mvl_manifest.rs"), src)
        .map_err(|e| Error::new(format!("cannot write mvl_manifest.rs: {e}")))?;
    Ok(t.emit())
}

fn collect(t: &mut Tracker) -> Result<OwnedManifest> {
    let cargo = required(t, "CARGO")?;
    let rustc = t.env("RUSTC").unwrap_or_else(|| "rustc".to_string());
    let manifest_dir = PathBuf::from(required(t, "CARGO_MANIFEST_DIR")?);
    let target = required(t, "TARGET")?;
    let host = required(t, "HOST")?;
    let out_dir = PathBuf::from(required(t, "OUT_DIR")?);
    let profile = profile_dir_from_out_dir(&out_dir).unwrap_or_default();

    // Packages and the dependency graph.
    let manifest_path = manifest_dir.join("Cargo.toml");
    let features_env = t.feature_vars();
    let g = graph::resolve(&cargo, &manifest_path, &target, &features_env)?;
    let workspace_root = g.workspace_root.clone();

    // Config files, substitutions, profile.
    let configs = config::discover(t, &manifest_dir, &workspace_root);
    let root_manifest = config::read_toml(&workspace_root.join("Cargo.toml"));
    let mut substitutions = config::substitutions(root_manifest.as_ref(), &configs);
    substitutions.sort();
    substitutions.dedup();
    let cargo_profile = match profile.as_str() {
        "debug" => "dev".to_string(),
        other => other.to_string(),
    };
    let prof = config::resolve_profile(t, &cargo_profile, root_manifest.as_ref(), &configs);

    // Toolchain.
    let rustc_info = toolchain::rustc_info(&rustc)?;
    let cargo_version = toolchain::cargo_version(&cargo);
    let encoded = t.env("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default();
    let raw_flags: Vec<String> = if encoded.is_empty() {
        Vec::new()
    } else {
        encoded.split('\u{1f}').map(str::to_string).collect()
    };
    let has_remap = rustflags::has_remap(&raw_flags);
    let flags = rustflags::redact(&raw_flags);
    let cc_version = toolchain::cc(t, &target);
    let linker = toolchain::linker(t, &target, &raw_flags, &configs);
    let (wrappers, auditable) = toolchain::wrappers(t);

    // VCS and date.
    let vcs = vcs::probe(t, &manifest_dir);
    let date = match t.env("SOURCE_DATE_EPOCH") {
        Some(s) => s
            .trim()
            .parse::<i64>()
            .map(crate::time::rfc3339_utc)
            .map_err(|_| Error::new(format!("SOURCE_DATE_EPOCH is not an integer: {s}")))?,
        None => vcs
            .commit_time
            .map(crate::time::rfc3339_utc)
            .unwrap_or_default(),
    };
    let reproducible = t.env("SOURCE_DATE_EPOCH").is_some() && has_remap;

    // Digests.
    let lockfile = workspace_root.join("Cargo.lock");
    t.file(&lockfile);
    let lockfile_digest = digest::file_sha256(&lockfile).unwrap_or_default();
    let mut extra: Vec<(String, PathBuf)> = vec![
        (
            "workspace/Cargo.toml".into(),
            workspace_root.join("Cargo.toml"),
        ),
        ("workspace/Cargo.lock".into(), lockfile.clone()),
    ];
    extra.extend(configs.iter().map(|c| (c.key.clone(), c.path.clone())));
    if let Some((key, path)) = config::toolchain_file(&manifest_dir, &workspace_root) {
        extra.push((key, path));
    }
    let source_digest = digest::source_digest(t, &cargo, &g.local_packages, &extra)?;

    let target_features: Vec<String> = t
        .env("CARGO_CFG_TARGET_FEATURE")
        .map(|s| s.split(',').map(str::to_string).collect())
        .unwrap_or_default();
    let opt_level = t.env("OPT_LEVEL").unwrap_or_default();
    let panic = t.env("CARGO_CFG_PANIC").unwrap_or_default();

    let runtime_version = std_runtime(rustc_info.release.as_deref());

    Ok(OwnedManifest {
        app_name: required(t, "CARGO_PKG_NAME")?,
        app_version: required(t, "CARGO_PKG_VERSION")?,
        mvl_version: env!("CARGO_PKG_VERSION").to_string(),
        runtime_version,
        packages: g.packages,
        build_packages: g.build_packages,
        source_digest,
        build: OwnedBuild {
            rustc_version: rustc_info.release,
            llvm_version: rustc_info.llvm,
            target,
            profile,
            date,
            host,
            rustc_commit: rustc_info.commit,
            rustc_channel: rustc_info.channel,
            cargo_version,
            cc_version,
            linker,
            opt_level,
            panic,
            target_features,
            overflow_checks: prof.overflow_checks,
            profile_overrides: prof.overrides,
            features: g.features,
            rustflags: flags,
            rustc_wrapper: wrappers,
            substitutions,
            vcs_commit: vcs.commit,
            vcs_dirty: vcs.dirty,
            vcs_describe: vcs.describe,
            lockfile_digest,
            reproducible,
            auditable,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run the real generator in-process against this crate, with the
    /// environment Cargo would give its build script.
    #[test]
    fn generates_manifest_for_this_crate() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let scratch = std::env::temp_dir().join(format!("mvl-manifest-gen-{}", std::process::id()));
        let out = scratch.join("target/release-custom/build/mvl-manifest-0/out");
        std::fs::create_dir_all(&out).unwrap();
        let target = if cfg!(target_os = "macos") {
            "aarch64-apple-darwin"
        } else {
            "x86_64-unknown-linux-gnu"
        };
        let env: BTreeMap<String, String> = [
            (
                "CARGO",
                std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()),
            ),
            ("CARGO_MANIFEST_DIR", dir.display().to_string()),
            ("OUT_DIR", out.display().to_string()),
            ("TARGET", target.into()),
            ("HOST", target.into()),
            ("CARGO_PKG_NAME", "mvl-manifest".into()),
            ("CARGO_PKG_VERSION", env!("CARGO_PKG_VERSION").into()),
            ("CARGO_FEATURE_BUILD", "1".into()),
            (
                "CARGO_ENCODED_RUSTFLAGS",
                "--remap-path-prefix=/x=/build\u{1f}-Ctoken=s".into(),
            ),
            ("SOURCE_DATE_EPOCH", "1794614400".into()),
            ("RUSTC_WRAPPER", "/opt/bin/sccache".into()),
            ("OPT_LEVEL", "3".into()),
            ("CARGO_CFG_PANIC", "abort".into()),
            ("CARGO_CFG_TARGET_FEATURE", "neon,sse2".into()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let t = Tracker {
            fixed_env: Some(env),
            ..Tracker::default()
        };
        let directives = generate_with(t).expect("generate");
        let src = std::fs::read_to_string(out.join("mvl_manifest.rs")).unwrap();

        assert!(src.starts_with("// @generated"));
        assert!(src.contains("app_name: \"mvl-manifest\""));
        assert!(
            src.contains("runtime_version: \"std-1."),
            "runtime is the Rust std"
        );
        assert!(src.contains("profile: \"release-custom\""));
        assert!(src.contains("date: \"2026-11-14T00:00:00Z\""));
        assert!(src.contains("features: &[\"build\"]"));
        assert!(src.contains(
            "rustflags: &[\"--remap-path-prefix=<redacted>=/build\", \"-Ctoken=<redacted>\"]"
        ));
        assert!(src.contains("rustc_wrapper: &[\"RUSTC_WRAPPER=sccache\"]"));
        assert!(src.contains("reproducible: true"));
        assert!(src.contains("panic: \"abort\""));
        assert!(
            src.contains("name: \"sha2\""),
            "runtime deps of the build feature"
        );
        let git = std::process::Command::new("git")
            .current_dir(dir)
            .args(["rev-parse", "HEAD"])
            .output()
            .ok()
            .filter(|o| o.status.success());
        if let Some(o) = git {
            let head = String::from_utf8_lossy(&o.stdout).trim().to_string();
            assert!(src.contains(&format!("vcs_commit: \"{head}\"")));
        }

        // Rerun directives cover the hashed sources, the lockfile and env reads.
        let lock = dir.join("../../Cargo.lock").canonicalize().unwrap();
        let has_file = |p: &Path| {
            directives
                .lines()
                .filter_map(|l| l.strip_prefix("cargo:rerun-if-changed="))
                .any(|f| Path::new(f).canonicalize().ok().as_deref() == Some(p))
        };
        assert!(has_file(&lock), "{directives}");
        assert!(has_file(&dir.join("src").canonicalize().unwrap()));
        assert!(has_file(&dir.join("src/lib.rs").canonicalize().unwrap()));
        for var in [
            "SOURCE_DATE_EPOCH",
            "RUSTC_WRAPPER",
            "CC",
            "CARGO_ENCODED_RUSTFLAGS",
        ] {
            assert!(
                directives.contains(&format!("cargo:rerun-if-env-changed={var}\n")),
                "{var} missing"
            );
        }
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn runtime_is_the_rust_std_version() {
        assert_eq!(std_runtime(Some("1.99.1")), "std-1.99.1");
        assert_eq!(std_runtime(Some("1.100.0-nightly")), "std-1.100.0-nightly");
        assert_eq!(std_runtime(None), "");
    }

    #[test]
    fn missing_cargo_env_is_a_clear_error() {
        let t = Tracker {
            fixed_env: Some(BTreeMap::new()),
            ..Tracker::default()
        };
        let err = generate_with(t).unwrap_err().to_string();
        assert!(err.contains("CARGO is not set"), "{err}");
    }

    #[test]
    fn profile_name_comes_from_out_dir() {
        let p = Path::new("/t/target/x86_64-unknown-linux-gnu/release-lto/build/app-0123/out");
        assert_eq!(profile_dir_from_out_dir(p).as_deref(), Some("release-lto"));
        let p = Path::new("/t/target/debug/build/app-0123/out");
        assert_eq!(profile_dir_from_out_dir(p).as_deref(), Some("debug"));
        // cargo 1.100+: <profile>/build/<pkg>/<hash>/out
        let p = Path::new("/t/target/debug/build/app/bed09f59b07c2f1a/out");
        assert_eq!(profile_dir_from_out_dir(p).as_deref(), Some("debug"));
        let p = Path::new("/t/target/release-lto/build/build/0123/out");
        assert_eq!(profile_dir_from_out_dir(p).as_deref(), Some("release-lto"));
    }
}
