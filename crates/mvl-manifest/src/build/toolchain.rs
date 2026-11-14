//! Toolchain identity: rustc, cargo, the C compiler, the linker and rustc wrappers.

use std::path::Path;
use std::process::Command;

use super::config::ConfigFile;
use super::{run, Result, Tracker};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct RustcInfo {
    pub release: Option<String>,
    pub llvm: Option<String>,
    pub commit: String,
    pub channel: String,
}

/// Parse `rustc -vV`.
pub fn parse_rustc_vv(out: &str) -> RustcInfo {
    let field = |k: &str| {
        out.lines()
            .find_map(|l| l.strip_prefix(k))
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty() && v != "unknown")
    };
    let release = field("release:");
    let channel = match release.as_deref() {
        Some(r) if r.contains("-nightly") => "nightly",
        Some(r) if r.contains("-beta") => "beta",
        Some(r) if r.contains("-dev") => "dev",
        Some(_) => "stable",
        None => "",
    }
    .to_string();
    RustcInfo {
        release,
        llvm: field("LLVM version:"),
        commit: field("commit-hash:").unwrap_or_default(),
        channel,
    }
}

pub fn rustc_info(rustc: &str) -> Result<RustcInfo> {
    Ok(parse_rustc_vv(&run(Command::new(rustc).arg("-vV"))?))
}

pub fn cargo_version(cargo: &str) -> String {
    run(Command::new(cargo).arg("-V"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// `<tool>: <first line of --version>`, or just `<tool>` if it will not say.
fn identify(tool: &str) -> String {
    let out = Command::new(tool).arg("--version").output();
    let first = out
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .map(|l| l.trim().to_string())
        })
        .filter(|l| !l.is_empty());
    match first {
        Some(v) => format!("{tool}: {v}"),
        None => tool.to_string(),
    }
}

/// The C compiler the `cc` crate would pick for `target`.
pub fn cc(t: &mut Tracker, target: &str) -> String {
    let underscored = target.replace('-', "_");
    let tool = [
        format!("CC_{target}"),
        format!("CC_{underscored}"),
        "TARGET_CC".to_string(),
        "CC".to_string(),
    ]
    .iter()
    .find_map(|v| t.env(v))
    .unwrap_or_else(|| {
        if target.contains("msvc") {
            "cl.exe".to_string()
        } else {
            "cc".to_string()
        }
    });
    if tool == "cl.exe" {
        // cl.exe has no --version; it prints its banner on stderr.
        return tool;
    }
    identify(&tool)
}

fn linker_from_flags(flags: &[String]) -> Option<String> {
    let mut iter = flags.iter();
    while let Some(f) = iter.next() {
        let v = if f == "-C" {
            iter.next().map(String::as_str)
        } else {
            f.strip_prefix("-C")
        };
        if let Some(l) = v.and_then(|v| v.strip_prefix("linker=")) {
            return Some(l.to_string());
        }
    }
    None
}

/// The linker in effect: `RUSTC_LINKER` (Cargo's resolved linker),
/// `-C linker=`, `CARGO_TARGET_<T>_LINKER`, `target.<T>.linker` in config,
/// else the platform default.
pub fn linker(t: &mut Tracker, target: &str, flags: &[String], configs: &[ConfigFile]) -> String {
    let var = format!(
        "CARGO_TARGET_{}_LINKER",
        target.to_uppercase().replace(['-', '.'], "_")
    );
    let tool = t
        .env("RUSTC_LINKER")
        .or_else(|| linker_from_flags(flags))
        .or_else(|| t.env(&var))
        .or_else(|| {
            configs.iter().find_map(|c| {
                c.table
                    .get("target")?
                    .get(target)?
                    .get("linker")?
                    .as_str()
                    .map(str::to_string)
            })
        })
        .unwrap_or_else(|| {
            if target.contains("msvc") {
                "link.exe".to_string()
            } else {
                "cc".to_string()
            }
        });
    if tool.ends_with("link.exe") {
        return tool;
    }
    identify(&tool)
}

fn basename(s: &str) -> String {
    Path::new(s)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| s.to_string())
}

/// `(["RUSTC_WRAPPER=<basename>", …], built_under_cargo_auditable)`.
/// Basenames only: an absolute wrapper path would make the manifest depend
/// on the build machine.
pub fn wrappers(t: &mut Tracker) -> (Vec<String>, bool) {
    let mut out = Vec::new();
    let mut auditable = t.env("CARGO_AUDITABLE_ORIG_ARGS").is_some();
    for var in ["RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"] {
        if let Some(w) = t.env(var) {
            let name = basename(&w);
            auditable |= name.starts_with("cargo-auditable");
            out.push(format!("{var}={name}"));
        }
    }
    (out, auditable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rustc_vv() {
        let out = "rustc 1.85.0 (4d91de4e4 2025-02-17)\nbinary: rustc\ncommit-hash: 4d91de4e48198da2e33413efdcd9cd2cc0c46688\ncommit-date: 2025-02-17\nhost: aarch64-apple-darwin\nrelease: 1.85.0\nLLVM version: 19.1.7\n";
        assert_eq!(
            parse_rustc_vv(out),
            RustcInfo {
                release: Some("1.85.0".into()),
                llvm: Some("19.1.7".into()),
                commit: "4d91de4e48198da2e33413efdcd9cd2cc0c46688".into(),
                channel: "stable".into(),
            }
        );
        assert_eq!(
            parse_rustc_vv("release: 1.90.0-nightly\n").channel,
            "nightly"
        );
    }

    #[test]
    fn linker_flag_both_spellings() {
        let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            linker_from_flags(&v(&["-C", "linker=clang"])).as_deref(),
            Some("clang")
        );
        assert_eq!(
            linker_from_flags(&v(&["-Clinker=lld"])).as_deref(),
            Some("lld")
        );
        assert_eq!(linker_from_flags(&v(&["-C", "opt-level=3"])), None);
    }

    #[test]
    fn wrapper_paths_are_reduced_to_basenames() {
        assert_eq!(basename("/opt/bin/sccache"), "sccache");
    }
}
