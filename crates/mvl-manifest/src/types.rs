//! Manifest types (ADR-0013). Dependency-free.
//!
//! Every embedded field is `&'static` so the generated manifest is a `const`
//! expression: no allocation at startup, and the bytes in the binary depend only
//! on the build inputs.

/// The embedded runtime manifest. Shared field names follow mvl-lang's
/// `std.runtime`; `stdlib_version` / `stdlib_profile` are dropped (ADR-0013).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Manifest {
    pub app_name: &'static str,
    pub app_version: &'static str,
    /// Version of `mvl-manifest`.
    pub mvl_version: &'static str,
    /// The Rust runtime: the standard library version, which is the rustc
    /// release (`std-1.99.1`). A linked `mvl` crate shows up in `packages`.
    pub runtime_version: &'static str,
    /// Runtime-linked crates (normal deps reachable for the target).
    pub packages: &'static [PackageInfo],
    /// Build-deps and proc-macros: they run during the build without being linked.
    pub build_packages: &'static [PackageInfo],
    /// Phase 2: always empty.
    pub ffi_bridges: &'static [FfiBridge],
    pub build: BuildInfo,
    /// Phase 2: always empty.
    pub licenses: &'static [&'static str],
    /// Phase 2: always zero.
    pub assurance: AssuranceInfo,
    /// SHA-256 tree digest over every build input (see ADR-0013).
    pub source_digest: &'static str,
    /// Phase 2: always empty.
    pub native_deps: &'static [NativeDep],
}

/// How a package takes part in the build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageKind {
    Runtime,
    Build,
    ProcMacro,
}

impl PackageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PackageKind::Runtime => "runtime",
            PackageKind::Build => "build",
            PackageKind::ProcMacro => "proc-macro",
        }
    }
}

/// Where a package came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageSource {
    Registry {
        url: &'static str,
    },
    Git {
        url: &'static str,
        commit: &'static str,
    },
    /// A path dependency outside the app's workspace.
    Path,
    /// A member of the app's own workspace (covered by `source_digest`).
    Workspace,
}

impl PackageSource {
    pub fn kind_str(self) -> &'static str {
        match self {
            PackageSource::Registry { .. } => "registry",
            PackageSource::Git { .. } => "git",
            PackageSource::Path => "path",
            PackageSource::Workspace => "workspace",
        }
    }

    /// `registry+<url>`, `git+<url>#<commit>` or `path`.
    pub fn describe(self) -> String {
        match self {
            PackageSource::Registry { url } => format!("registry+{url}"),
            PackageSource::Git { url, commit } => format!("git+{url}#{commit}"),
            PackageSource::Path => "path".to_string(),
            PackageSource::Workspace => "workspace".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackageInfo {
    pub name: &'static str,
    pub version: &'static str,
    /// `pkg:cargo/<name>@<ver>`; git deps add `?vcs_url=`.
    pub purl: &'static str,
    /// Phase 2: always empty.
    pub license: &'static str,
    pub kind: PackageKind,
    pub source: PackageSource,
    /// `Cargo.lock` sha256 for registry crates; empty otherwise.
    pub checksum: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FfiBridge {
    pub abi: &'static str,
    pub bridge_name: &'static str,
    pub bridge_version: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AssuranceInfo {
    pub extern_ratio: f64,
    pub extern_count: u64,
    pub total_functions: u64,
    pub requirements_proven: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeKind {
    Rust,
    C,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeDep {
    pub name: &'static str,
    pub version: &'static str,
    pub kind: NativeKind,
    pub purl: &'static str,
    pub introduced_by: &'static [&'static str],
}

/// Toolchain, configuration and provenance of the build. Only values fully
/// determined by the build inputs are embedded (ADR-0013 decision 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildInfo {
    /// Always `"mvl-rust"`.
    pub backend: &'static str,
    pub rustc_version: Option<&'static str>,
    pub llvm_version: Option<&'static str>,
    pub target: &'static str,
    /// Profile directory name: `debug`, `release` or a custom profile name.
    pub profile: &'static str,
    /// RFC 3339 UTC, from `SOURCE_DATE_EPOCH` or the git commit time; never the wall clock.
    pub date: &'static str,
    pub host: &'static str,
    pub rustc_commit: &'static str,
    pub rustc_channel: &'static str,
    pub cargo_version: &'static str,
    /// `<cc>: <first line of --version>`.
    pub cc_version: &'static str,
    /// `<linker>: <first line of --version>`.
    pub linker: &'static str,
    pub opt_level: &'static str,
    pub debug_assertions: bool,
    pub panic: &'static str,
    pub target_features: &'static [&'static str],
    pub overflow_checks: bool,
    pub profile_overrides: &'static [&'static str],
    pub features: &'static [&'static str],
    /// `CARGO_ENCODED_RUSTFLAGS`, secrets and remapped source paths redacted.
    pub rustflags: &'static [&'static str],
    /// `RUSTC_WRAPPER=<basename>` / `RUSTC_WORKSPACE_WRAPPER=<basename>`.
    pub rustc_wrapper: &'static [&'static str],
    /// `[patch]`, `[replace]`, `paths` overrides and `source` replacements.
    pub substitutions: &'static [&'static str],
    pub vcs_commit: &'static str,
    pub vcs_dirty: bool,
    pub vcs_describe: &'static str,
    pub lockfile_digest: &'static str,
    /// `SOURCE_DATE_EPOCH` and `--remap-path-prefix` both in effect.
    pub reproducible: bool,
    /// Built under `cargo auditable`.
    pub auditable: bool,
}

impl BuildInfo {
    /// Any profile other than the `debug` (dev/test) directory counts as release.
    pub fn is_release(&self) -> bool {
        self.profile != "debug"
    }
}

/// Facts about the running process, collected at startup (never embedded).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeEnv {
    pub os: String,
    pub arch: String,
    /// Kernel / OS version (`uname -sr` on Unix).
    pub os_version: String,
    pub pid: u32,
    /// RFC 3339 UTC.
    pub started_at: String,
    pub exe_path: String,
    /// Shared libraries actually loaded into the process.
    pub loaded_libraries: Vec<String>,
    /// glibc version, where applicable.
    pub libc_version: Option<String>,
    /// sha256 of the running executable (feature `exe-digest`). Evidence only.
    pub exe_digest: Option<String>,
}
