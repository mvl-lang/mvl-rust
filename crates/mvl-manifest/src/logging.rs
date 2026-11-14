//! `log_manifest()` (feature `log`, ADR-0013 "Log records").
//!
//! All records go to target [`TARGET`]. When the installed logger would drop
//! an INFO record for that target (no logger, `max_level() == Off`, or a filter
//! below INFO), the records go to stderr instead: the manifest never
//! disappears silently.
//!
//! Every record stays short enough for line-limited log paths (journald,
//! podman's 8 KiB partial entries): packages get one record each, and the
//! environment record lists only non-system libraries unless asked otherwise.

use std::io::Write;

use sha2::{Digest, Sha256};

use crate::format::{
    build_pairs, list, logfmt, package_pairs, packages_summary_pairs, summary_pairs,
};
use crate::types::{Manifest, RuntimeEnv};
use crate::warn::assurance_warnings;

/// The dedicated log target.
pub const TARGET: &str = "mvl_manifest";

/// Runtime switch: `MVL_MANIFEST_LIBRARIES=all` lists every loaded library,
/// system ones included.
pub const LIBRARIES_ENV: &str = "MVL_MANIFEST_LIBRARIES";

/// What `log_manifest` includes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LogOptions {
    /// List system libraries too (macOS frameworks can number in the hundreds).
    pub all_libraries: bool,
}

impl LogOptions {
    /// Options from the process environment ([`LIBRARIES_ENV`]).
    pub fn from_env() -> Self {
        LogOptions {
            all_libraries: std::env::var(LIBRARIES_ENV).is_ok_and(|v| v == "all"),
        }
    }
}

/// One emitted record: level, title and logfmt fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub level: log::Level,
    pub title: &'static str,
    pub fields: String,
}

impl Record {
    pub fn message(&self) -> String {
        if self.fields.is_empty() {
            self.title.to_string()
        } else {
            format!("{} {}", self.title, self.fields)
        }
    }
}

/// Libraries in the OS's own locations, left out of the default listing.
pub fn is_system_library(os: &str, path: &str) -> bool {
    match os {
        "macos" | "ios" => path.starts_with("/System/") || path.starts_with("/usr/lib/"),
        _ => false,
    }
}

/// Record 5, `MVL Runtime Environment`. Always carries the count and a digest
/// of the full sorted library list, so the set is pinned even when only the
/// non-system libraries are printed.
pub fn env_pairs(e: &RuntimeEnv, opts: &LogOptions) -> Vec<(&'static str, String)> {
    let mut sorted = e.loaded_libraries.clone();
    sorted.sort();
    let digest = if sorted.is_empty() {
        String::new()
    } else {
        format!("{:x}", Sha256::digest(sorted.join("\n").as_bytes()))
    };
    let shown: Vec<&String> = sorted
        .iter()
        .filter(|l| opts.all_libraries || !is_system_library(&e.os, l))
        .collect();
    let omitted = sorted.len() - shown.len();
    vec![
        ("os", e.os.clone()),
        ("arch", e.arch.clone()),
        ("os_version", e.os_version.clone()),
        ("pid", e.pid.to_string()),
        ("started_at", e.started_at.clone()),
        ("exe", e.exe_path.clone()),
        ("glibc", e.libc_version.clone().unwrap_or_default()),
        ("exe_sha256", e.exe_digest.clone().unwrap_or_default()),
        ("libraries_count", sorted.len().to_string()),
        ("libraries_sha256", digest),
        ("libraries", list(&shown)),
        (
            "system_libraries_omitted",
            if omitted > 0 {
                omitted.to_string()
            } else {
                String::new()
            },
        ),
    ]
}

/// The records `log_manifest` emits, in order:
/// 1. `MVL Runtime Manifest`
/// 2. `MVL Build Provenance`
/// 3. `MVL Runtime Packages` (count), then one `MVL Runtime Package` each
/// 4. `MVL Build Packages` (count), then one `MVL Build Package` each
/// 5. `MVL Runtime Environment`
/// 6. one `MVL Assurance Warning` (WARN) per assurance-weakening condition
pub fn records(m: &Manifest, env: &RuntimeEnv, opts: &LogOptions) -> Vec<Record> {
    let info = |title, pairs: Vec<(&str, String)>| Record {
        level: log::Level::Info,
        title,
        fields: logfmt(&pairs),
    };
    let mut out = vec![
        info("MVL Runtime Manifest", summary_pairs(m)),
        info("MVL Build Provenance", build_pairs(&m.build)),
        info("MVL Runtime Packages", packages_summary_pairs(m.packages)),
    ];
    out.extend(
        m.packages
            .iter()
            .map(|p| info("MVL Runtime Package", package_pairs(p))),
    );
    out.push(info(
        "MVL Build Packages",
        packages_summary_pairs(m.build_packages),
    ));
    out.extend(
        m.build_packages
            .iter()
            .map(|p| info("MVL Build Package", package_pairs(p))),
    );
    out.push(info("MVL Runtime Environment", env_pairs(env, opts)));
    for w in assurance_warnings(m) {
        out.push(Record {
            level: log::Level::Warn,
            title: "MVL Assurance Warning",
            fields: logfmt(&[("warning", w.code().to_string()), ("message", w.message())]),
        });
    }
    out
}

/// Whether the installed logger would record INFO for [`TARGET`].
pub fn logger_accepts() -> bool {
    log::max_level() >= log::LevelFilter::Info
        && log::logger().enabled(
            &log::Metadata::builder()
                .level(log::Level::Info)
                .target(TARGET)
                .build(),
        )
}

/// Write records in the stderr fallback format: `<LEVEL> mvl_manifest: <message>`.
pub fn write_records(w: &mut dyn Write, records: &[Record]) -> std::io::Result<()> {
    for r in records {
        writeln!(w, "{} {TARGET}: {}", r.level, r.message())?;
    }
    Ok(())
}

/// Log the manifest and the runtime environment at startup.
pub fn log_manifest(m: &Manifest) {
    log_manifest_with_env(m, &RuntimeEnv::collect());
}

/// [`log_manifest`] with a caller-supplied environment.
pub fn log_manifest_with_env(m: &Manifest, env: &RuntimeEnv) {
    let recs = records(m, env, &LogOptions::from_env());
    if logger_accepts() {
        for r in &recs {
            log::log!(target: TARGET, r.level, "{}", r.message());
        }
    } else {
        let _ = write_records(&mut std::io::stderr().lock(), &recs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata::{clean_release, sample};

    fn env() -> RuntimeEnv {
        RuntimeEnv {
            os: "linux".into(),
            arch: "x86_64".into(),
            os_version: "Linux 6.8.0".into(),
            pid: 42,
            started_at: "2026-11-14T00:00:00Z".into(),
            exe_path: "/srv/demo".into(),
            loaded_libraries: vec!["/lib/libc.so.6".into()],
            libc_version: Some("2.39".into()),
            exe_digest: None,
        }
    }

    fn mac_env(n_system: usize) -> RuntimeEnv {
        let mut libs: Vec<String> = (0..n_system)
            .map(|i| format!("/System/Library/Frameworks/F{i}.framework/Versions/A/F{i}"))
            .collect();
        libs.push("/usr/lib/libSystem.B.dylib".into());
        libs.push("/opt/homebrew/lib/libssl.3.dylib".into());
        RuntimeEnv {
            os: "macos".into(),
            loaded_libraries: libs,
            libc_version: None,
            ..env()
        }
    }

    fn titles(recs: &[Record]) -> Vec<&str> {
        recs.iter().map(|r| r.title).collect()
    }

    #[test]
    fn emits_records_in_order_then_warnings() {
        let recs = records(&sample(), &env(), &LogOptions::default());
        assert_eq!(
            titles(&recs),
            [
                "MVL Runtime Manifest",
                "MVL Build Provenance",
                "MVL Runtime Packages",
                "MVL Runtime Package",
                "MVL Runtime Package",
                "MVL Runtime Package",
                "MVL Build Packages",
                "MVL Build Package",
                "MVL Runtime Environment",
                "MVL Assurance Warning",
            ]
        );
        let last = recs.last().unwrap();
        assert_eq!(last.level, log::Level::Warn);
        assert!(last.fields.starts_with("warning=path_or_git_dependency "));
    }

    #[test]
    fn manifest_record_starts_with_app_version() {
        let recs = records(&clean_release(), &env(), &LogOptions::default());
        assert!(recs[0]
            .message()
            .starts_with("MVL Runtime Manifest app=demo app_version=1.2.3 "));
        assert!(recs[0]
            .fields
            .contains(" vcs_commit=0123456789abcdef0123456789abcdef01234567 "));
        assert!(recs[0].fields.contains(" lockfile_digest=aaaa "));
        assert!(recs[0].fields.ends_with(" source_digest=bbbb"));
        assert!(
            !recs.iter().any(|r| r.level == log::Level::Warn),
            "a clean release has no warnings"
        );
    }

    #[test]
    fn shared_fields_appear_in_one_record_only() {
        let recs = records(&clean_release(), &env(), &LogOptions::default());
        for key in [
            "backend=",
            "target=",
            "profile=",
            "built=",
            "vcs_commit=",
            "lockfile_digest=",
        ] {
            let n = recs
                .iter()
                .filter(|r| r.fields.split(' ').any(|f| f.starts_with(key)))
                .count();
            assert_eq!(n, 1, "{key} appears in {n} records");
        }
    }

    #[test]
    fn one_record_per_package_with_count_summary() {
        let recs = records(&sample(), &env(), &LogOptions::default());
        assert_eq!(recs[2].fields, "packages_count=3");
        assert_eq!(
            recs[3].fields,
            "purl=pkg:cargo/log@0.4.33 source=registry+https://github.com/rust-lang/crates.io-index sha256=cccc"
        );
        // Path packages have no checksum: the empty field is left out.
        assert_eq!(recs[4].fields, "purl=pkg:cargo/mvl@0.12.0 source=path");
        assert_eq!(recs[6].fields, "packages_count=1");
        assert_eq!(
            recs[7].fields,
            "purl=pkg:cargo/derive-demo@0.1.0 kind=proc-macro source=path"
        );
    }

    #[test]
    fn environment_record_lists_loaded_libraries() {
        let recs = records(&clean_release(), &env(), &LogOptions::default());
        let e = recs
            .iter()
            .find(|r| r.title == "MVL Runtime Environment")
            .unwrap();
        let digest = format!("{:x}", Sha256::digest(b"/lib/libc.so.6"));
        assert_eq!(
            e.fields,
            format!("os=linux arch=x86_64 os_version=\"Linux 6.8.0\" pid=42 started_at=2026-11-14T00:00:00Z exe=/srv/demo glibc=2.39 libraries_count=1 libraries_sha256={digest} libraries=/lib/libc.so.6")
        );
    }

    #[test]
    fn macos_environment_record_is_bounded() {
        let e = mac_env(345);
        let fields = logfmt(&env_pairs(&e, &LogOptions::default()));
        assert!(fields.contains(" libraries_count=347 "), "{fields}");
        assert!(fields.contains(" libraries=/opt/homebrew/lib/libssl.3.dylib "));
        assert!(fields.ends_with(" system_libraries_omitted=346"));
        assert!(fields.len() < 1024, "record is {} bytes", fields.len());

        // The digest covers the full list, so it is the same either way.
        let all = logfmt(&env_pairs(
            &e,
            &LogOptions {
                all_libraries: true,
            },
        ));
        assert!(all.len() > 10_000);
        assert!(!all.contains("system_libraries_omitted"));
        let digest = |s: &str| {
            s.split(' ')
                .find(|f| f.starts_with("libraries_sha256="))
                .unwrap()
                .to_string()
        };
        assert_eq!(digest(&fields), digest(&all));
    }

    #[test]
    fn static_binary_has_no_library_fields() {
        let e = RuntimeEnv {
            loaded_libraries: vec![],
            libc_version: None,
            ..env()
        };
        let fields = logfmt(&env_pairs(&e, &LogOptions::default()));
        assert!(fields.ends_with(" libraries_count=0"), "{fields}");
    }

    #[test]
    fn no_logger_installed_means_stderr_fallback() {
        // No logger is ever installed in this test binary.
        assert!(!logger_accepts());
        let recs = records(&clean_release(), &env(), &LogOptions::default());
        let mut buf = Vec::new();
        write_records(&mut buf, &recs).unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert!(
            text.starts_with("INFO mvl_manifest: MVL Runtime Manifest app=demo app_version=1.2.3 ")
        );
        assert_eq!(text.lines().count(), recs.len());
    }
}
