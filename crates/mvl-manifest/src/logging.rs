//! `log_manifest()` (feature `log`, ADR-0013 "Log records").
//!
//! All records go to target [`TARGET`]. When the installed logger would drop
//! an INFO record for that target (no logger, `max_level() == Off`, or a filter
//! below INFO), the records go to stderr instead: the manifest never
//! disappears silently.

use std::io::Write;

use crate::format::{build_pairs, env_pairs, logfmt, packages_pairs, summary_pairs};
use crate::types::{Manifest, RuntimeEnv};
use crate::warn::assurance_warnings;

/// The dedicated log target.
pub const TARGET: &str = "mvl_manifest";

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

/// The records `log_manifest` emits, in order: manifest, build provenance,
/// runtime packages, build packages, runtime environment, then one WARN per
/// assurance-weakening condition.
pub fn records(m: &Manifest, env: &RuntimeEnv) -> Vec<Record> {
    let info = |title, pairs: Vec<(&str, String)>| Record {
        level: log::Level::Info,
        title,
        fields: logfmt(&pairs),
    };
    let mut out = vec![
        info("MVL Runtime Manifest", summary_pairs(m)),
        info("MVL Build Provenance", build_pairs(&m.build)),
        info("MVL Runtime Packages", packages_pairs(m.packages)),
        info("MVL Build Packages", packages_pairs(m.build_packages)),
        info("MVL Runtime Environment", env_pairs(env)),
    ];
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
    let recs = records(m, env);
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

    #[test]
    fn emits_five_records_in_order_then_warnings() {
        let recs = records(&sample(), &env());
        let titles: Vec<&str> = recs.iter().map(|r| r.title).collect();
        assert_eq!(
            titles,
            [
                "MVL Runtime Manifest",
                "MVL Build Provenance",
                "MVL Runtime Packages",
                "MVL Build Packages",
                "MVL Runtime Environment",
                "MVL Assurance Warning",
            ]
        );
        assert_eq!(recs[5].level, log::Level::Warn);
        assert!(recs[5]
            .fields
            .starts_with("warning=path_or_git_dependency "));
    }

    #[test]
    fn manifest_record_starts_with_app_version() {
        let recs = records(&clean_release(), &env());
        assert!(recs[0]
            .message()
            .starts_with("MVL Runtime Manifest app=demo app_version=1.2.3 "));
        assert!(recs[0]
            .fields
            .contains(" vcs_commit=0123456789abcdef0123456789abcdef01234567 "));
        assert!(recs[0].fields.contains(" lockfile_digest=aaaa "));
        assert!(recs[0].fields.ends_with(" source_digest=bbbb"));
        assert_eq!(recs.len(), 5, "a clean release has no warnings");
    }

    #[test]
    fn environment_record_lists_loaded_libraries() {
        let recs = records(&clean_release(), &env());
        assert_eq!(
            recs[4].fields,
            "os=linux arch=x86_64 os_version=\"Linux 6.8.0\" pid=42 started_at=2026-11-14T00:00:00Z exe=/srv/demo glibc=2.39 loaded_libraries_count=1 loaded_libraries=/lib/libc.so.6"
        );
    }

    #[test]
    fn no_logger_installed_means_stderr_fallback() {
        // No logger is ever installed in this test binary.
        assert!(!logger_accepts());
        let mut buf = Vec::new();
        write_records(&mut buf, &records(&clean_release(), &env())).unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert!(
            text.starts_with("INFO mvl_manifest: MVL Runtime Manifest app=demo app_version=1.2.3 ")
        );
        assert_eq!(text.lines().count(), 5);
    }
}
