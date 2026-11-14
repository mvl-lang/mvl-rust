//! A fixed manifest for golden and decision-logic tests.

use crate::types::*;

pub const REGISTRY: &str = "https://github.com/rust-lang/crates.io-index";

pub const PACKAGES: &[PackageInfo] = &[
    PackageInfo {
        name: "log",
        version: "0.4.33",
        purl: "pkg:cargo/log@0.4.33",
        license: "",
        kind: PackageKind::Runtime,
        source: PackageSource::Registry { url: REGISTRY },
        checksum: "cccc",
    },
    PackageInfo {
        name: "mvl",
        version: "0.12.0",
        purl: "pkg:cargo/mvl@0.12.0",
        license: "",
        kind: PackageKind::Runtime,
        source: PackageSource::Path,
        checksum: "",
    },
    PackageInfo {
        name: "gitdep",
        version: "0.1.0",
        purl: "pkg:cargo/gitdep@0.1.0?vcs_url=git%2Bhttps://example.com/g.git%40c0ffee",
        license: "",
        kind: PackageKind::Runtime,
        source: PackageSource::Git {
            url: "https://example.com/g.git",
            commit: "c0ffee",
        },
        checksum: "",
    },
];

pub const BUILD_PACKAGES: &[PackageInfo] = &[PackageInfo {
    name: "derive-demo",
    version: "0.1.0",
    purl: "pkg:cargo/derive-demo@0.1.0",
    license: "",
    kind: PackageKind::ProcMacro,
    source: PackageSource::Path,
    checksum: "",
}];

/// A release build that satisfies every assurance condition except having
/// path and git deps.
pub fn sample() -> Manifest {
    Manifest {
        app_name: "demo",
        app_version: "1.2.3",
        mvl_version: "0.12.0",
        runtime_version: "std-1.85.0",
        packages: PACKAGES,
        build_packages: BUILD_PACKAGES,
        ffi_bridges: &[],
        build: BuildInfo {
            backend: "mvl-rust",
            rustc_version: Some("1.85.0"),
            llvm_version: Some("19.1.7"),
            target: "x86_64-unknown-linux-gnu",
            profile: "release",
            date: "2026-11-14T00:00:00Z",
            host: "x86_64-unknown-linux-gnu",
            rustc_commit: "4d91de4e48198da2e33413efdcd9cd2cc0c46688",
            rustc_channel: "stable",
            cargo_version: "cargo 1.85.0",
            cc_version: "cc: gcc 13.2.0",
            linker: "cc: gcc 13.2.0",
            opt_level: "3",
            debug_assertions: false,
            panic: "unwind",
            target_features: &["fxsr", "sse", "sse2"],
            overflow_checks: false,
            profile_overrides: &[],
            features: &["default"],
            rustflags: &["--remap-path-prefix=<redacted>=/build"],
            rustc_wrapper: &[],
            substitutions: &[],
            vcs_commit: "0123456789abcdef0123456789abcdef01234567",
            vcs_dirty: false,
            vcs_describe: "v1.2.3",
            lockfile_digest: "aaaa",
            reproducible: true,
            auditable: true,
        },
        licenses: &[],
        assurance: AssuranceInfo::default(),
        source_digest: "bbbb",
        native_deps: &[],
    }
}

/// `sample()` with only registry packages: no WARN conditions at all.
pub fn clean_release() -> Manifest {
    Manifest {
        packages: &PACKAGES[..1],
        build_packages: &[],
        ..sample()
    }
}
