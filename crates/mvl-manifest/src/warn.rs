//! Conditions that weaken assurance about what is running (ADR-0013, log record 6).
//! A pure function of the manifest, so the decision logic is testable without a logger.

use crate::types::{Manifest, PackageSource};

/// One assurance-weakening condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    DirtyTree,
    NightlyToolchain { channel: String },
    PathOrGitDependency { packages: Vec<String> },
    Substitutions { entries: Vec<String> },
    RustcWrapper { wrappers: Vec<String> },
    NotAuditable,
    NotReproducible,
    DebugAssertionsInRelease,
}

impl Warning {
    /// Stable machine-readable code, used as the `warning=` logfmt key.
    pub fn code(&self) -> &'static str {
        match self {
            Warning::DirtyTree => "dirty_tree",
            Warning::NightlyToolchain { .. } => "nightly_toolchain",
            Warning::PathOrGitDependency { .. } => "path_or_git_dependency",
            Warning::Substitutions { .. } => "substitutions",
            Warning::RustcWrapper { .. } => "rustc_wrapper",
            Warning::NotAuditable => "not_auditable",
            Warning::NotReproducible => "not_reproducible",
            Warning::DebugAssertionsInRelease => "debug_assertions_in_release",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Warning::DirtyTree => "built from a working tree with uncommitted changes".into(),
            Warning::NightlyToolchain { channel } => {
                format!("built with a non-stable toolchain ({channel})")
            }
            Warning::PathOrGitDependency { packages } => format!(
                "release build uses path/git dependencies: {}",
                packages.join(", ")
            ),
            Warning::Substitutions { entries } => format!(
                "release build has dependency substitutions not visible in Cargo.lock: {}",
                entries.join(", ")
            ),
            Warning::RustcWrapper { wrappers } => format!(
                "release build ran through a rustc wrapper: {}",
                wrappers.join(", ")
            ),
            Warning::NotAuditable => {
                "release build was not built with `cargo auditable build --locked`".into()
            }
            Warning::NotReproducible => {
                "release build is not reproducible (needs SOURCE_DATE_EPOCH and --remap-path-prefix)"
                    .into()
            }
            Warning::DebugAssertionsInRelease => "debug assertions are on in a release build".into(),
        }
    }
}

/// `true` when the wrapper entry is cargo-auditable itself, which is the
/// wrapper we require rather than one that weakens assurance.
fn is_auditable_wrapper(entry: &str) -> bool {
    let name = entry.rsplit('=').next().unwrap_or(entry);
    name.starts_with("cargo-auditable")
}

/// Every assurance-weakening condition present in `m`, in a fixed order.
pub fn assurance_warnings(m: &Manifest) -> Vec<Warning> {
    let b = &m.build;
    let mut out = Vec::new();
    if b.vcs_dirty {
        out.push(Warning::DirtyTree);
    }
    if !b.rustc_channel.is_empty() && b.rustc_channel != "stable" {
        out.push(Warning::NightlyToolchain {
            channel: b.rustc_channel.to_string(),
        });
    }
    if !b.is_release() {
        return out;
    }
    // Workspace members are the app's own code, covered by source_digest. A
    // crate used both at runtime and in build.rs is listed once.
    let mut local: Vec<String> = m
        .packages
        .iter()
        .chain(m.build_packages.iter())
        .filter(|p| matches!(p.source, PackageSource::Path | PackageSource::Git { .. }))
        .map(|p| format!("{}@{} ({})", p.name, p.version, p.source.kind_str()))
        .collect();
    local.sort();
    local.dedup();
    if !local.is_empty() {
        out.push(Warning::PathOrGitDependency { packages: local });
    }
    if !b.substitutions.is_empty() {
        out.push(Warning::Substitutions {
            entries: b.substitutions.iter().map(|s| s.to_string()).collect(),
        });
    }
    let wrappers: Vec<String> = b
        .rustc_wrapper
        .iter()
        .filter(|w| !is_auditable_wrapper(w))
        .map(|w| w.to_string())
        .collect();
    if !wrappers.is_empty() {
        out.push(Warning::RustcWrapper { wrappers });
    }
    if !b.auditable {
        out.push(Warning::NotAuditable);
    }
    if !b.reproducible {
        out.push(Warning::NotReproducible);
    }
    if b.debug_assertions {
        out.push(Warning::DebugAssertionsInRelease);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata::{clean_release, sample};
    use crate::types::{BuildInfo, Manifest};

    fn with_build(f: impl FnOnce(&mut BuildInfo)) -> Manifest {
        let mut m = clean_release();
        f(&mut m.build);
        m
    }

    fn codes(m: &Manifest) -> Vec<&'static str> {
        assurance_warnings(m).iter().map(Warning::code).collect()
    }

    #[test]
    fn clean_release_has_no_warnings() {
        assert!(assurance_warnings(&clean_release()).is_empty());
    }

    #[test]
    fn dirty_tree_warns() {
        assert_eq!(codes(&with_build(|b| b.vcs_dirty = true)), ["dirty_tree"]);
    }

    #[test]
    fn nightly_toolchain_warns() {
        assert_eq!(
            codes(&with_build(|b| b.rustc_channel = "nightly")),
            ["nightly_toolchain"]
        );
    }

    #[test]
    fn path_and_git_deps_in_release_warn() {
        let w = assurance_warnings(&sample());
        assert_eq!(
            w,
            [Warning::PathOrGitDependency {
                packages: vec![
                    "derive-demo@0.1.0 (path)".into(),
                    "gitdep@0.1.0 (git)".into(),
                    "mvl@0.12.0 (path)".into(),
                ]
            }]
        );
    }

    #[test]
    fn local_dependency_listed_once_and_workspace_members_skipped() {
        use crate::types::{PackageInfo, PackageKind, PackageSource};
        const SHARED: PackageInfo = PackageInfo {
            name: "mvl-manifest",
            version: "0.12.1",
            purl: "pkg:cargo/mvl-manifest@0.12.1",
            license: "",
            kind: PackageKind::Runtime,
            source: PackageSource::Git {
                url: "https://example.com/m.git",
                commit: "abc",
            },
            checksum: "",
        };
        const MEMBER: PackageInfo = PackageInfo {
            name: "frank-core",
            source: PackageSource::Workspace,
            ..SHARED
        };
        const BUILD: PackageInfo = PackageInfo {
            kind: PackageKind::Build,
            ..SHARED
        };
        let m = Manifest {
            packages: &[SHARED, MEMBER],
            build_packages: &[BUILD],
            ..clean_release()
        };
        assert_eq!(
            assurance_warnings(&m),
            [Warning::PathOrGitDependency {
                packages: vec!["mvl-manifest@0.12.1 (git)".into()]
            }]
        );
    }

    #[test]
    fn path_deps_in_debug_do_not_warn() {
        let mut m = sample();
        m.build.profile = "debug";
        m.build.auditable = false;
        m.build.reproducible = false;
        m.build.debug_assertions = true;
        assert!(assurance_warnings(&m).is_empty());
    }

    #[test]
    fn substitution_in_release_warns() {
        assert_eq!(
            codes(&with_build(
                |b| b.substitutions = &["patch.crates-io.log -> path:../log"]
            )),
            ["substitutions"]
        );
    }

    #[test]
    fn rustc_wrapper_in_release_warns() {
        assert_eq!(
            codes(&with_build(|b| b.rustc_wrapper = &["RUSTC_WRAPPER=sccache"])),
            ["rustc_wrapper"]
        );
    }

    #[test]
    fn auditable_wrapper_is_not_a_wrapper_warning() {
        assert!(codes(&with_build(|b| {
            b.rustc_wrapper = &["RUSTC_WORKSPACE_WRAPPER=cargo-auditable"]
        }))
        .is_empty());
    }

    #[test]
    fn not_auditable_warns() {
        assert_eq!(
            codes(&with_build(|b| b.auditable = false)),
            ["not_auditable"]
        );
    }

    #[test]
    fn not_reproducible_warns() {
        assert_eq!(
            codes(&with_build(|b| b.reproducible = false)),
            ["not_reproducible"]
        );
    }

    #[test]
    fn debug_assertions_in_release_warn() {
        assert_eq!(
            codes(&with_build(|b| b.debug_assertions = true)),
            ["debug_assertions_in_release"]
        );
    }
}
