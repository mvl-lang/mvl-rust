//! Formatters: logfmt, JSON and block. Dependency-free.
//!
//! Shared fields keep mvl-lang's key names (`app`, `app_version`, `mvl`,
//! `runtime`, `backend`, `built`, `rustc`, `llvm`, `packages`, `ffi`,
//! `source` / `source_digest`); Rust-specific fields are added after them.

use crate::types::{BuildInfo, Manifest, PackageInfo, PackageSource, RuntimeEnv};

// ── logfmt ──────────────────────────────────────────────────────────────────

/// Render `key=value` pairs as one logfmt line. Values containing spaces,
/// quotes, `=` or control characters are quoted; empty values render as `""`.
pub fn logfmt(pairs: &[(&str, String)]) -> String {
    let mut out = String::new();
    for (i, (k, v)) in pairs.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(k);
        out.push('=');
        out.push_str(&logfmt_value(v));
    }
    out
}

fn logfmt_value(v: &str) -> String {
    let needs_quotes = v.is_empty()
        || v.chars()
            .any(|c| c == ' ' || c == '"' || c == '=' || c == '\\' || c.is_control());
    if !needs_quotes {
        return v.to_string();
    }
    let mut s = String::from("\"");
    for c in v.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            '\n' => s.push_str("\\n"),
            '\t' => s.push_str("\\t"),
            c if c.is_control() => s.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => s.push(c),
        }
    }
    s.push('"');
    s
}

fn join(items: &[&str]) -> String {
    items.join(",")
}

/// One package as `<purl> <source>[ sha256=<checksum>]`.
pub fn package_entry(p: &PackageInfo) -> String {
    let mut s = format!("{} {}", p.purl, p.source.describe());
    if !p.checksum.is_empty() {
        s.push_str(" sha256=");
        s.push_str(p.checksum);
    }
    s
}

/// The whole manifest as a single logfmt line (mvl-lang `manifest_to_logfmt`,
/// extended).
pub fn manifest_to_logfmt(m: &Manifest) -> String {
    // mvl-lang names the digest `source=` here and puts it last.
    let mut pairs: Vec<(&str, String)> = summary_pairs(m)
        .into_iter()
        .filter(|(k, _)| *k != "source_digest")
        .collect();
    let purls: Vec<&str> = m.packages.iter().map(|p| p.purl).collect();
    if !purls.is_empty() {
        pairs.push(("packages", join(&purls)));
    }
    let build_purls: Vec<&str> = m.build_packages.iter().map(|p| p.purl).collect();
    if !build_purls.is_empty() {
        pairs.push(("build_packages", join(&build_purls)));
    }
    let ffi: Vec<String> = m
        .ffi_bridges
        .iter()
        .map(|f| format!("{}({})", f.bridge_name, f.abi))
        .collect();
    if !ffi.is_empty() {
        pairs.push(("ffi", ffi.join(",")));
    }
    if !m.source_digest.is_empty() {
        pairs.push(("source", m.source_digest.to_string()));
    }
    logfmt(&pairs)
}

/// Record 1, `MVL Runtime Manifest`: identity plus the identifiers external
/// verification joins on.
pub fn summary_pairs(m: &Manifest) -> Vec<(&'static str, String)> {
    vec![
        ("app", m.app_name.to_string()),
        ("app_version", m.app_version.to_string()),
        ("mvl", m.mvl_version.to_string()),
        ("runtime", m.runtime_version.to_string()),
        ("backend", m.build.backend.to_string()),
        ("target", m.build.target.to_string()),
        ("profile", m.build.profile.to_string()),
        ("built", m.build.date.to_string()),
        ("vcs_commit", m.build.vcs_commit.to_string()),
        ("lockfile_digest", m.build.lockfile_digest.to_string()),
        ("source_digest", m.source_digest.to_string()),
    ]
}

/// Record 2, `MVL Build Provenance`: every `BuildInfo` field.
pub fn build_pairs(b: &BuildInfo) -> Vec<(&'static str, String)> {
    vec![
        ("backend", b.backend.to_string()),
        ("rustc", b.rustc_version.unwrap_or("").to_string()),
        ("llvm", b.llvm_version.unwrap_or("").to_string()),
        ("rustc_commit", b.rustc_commit.to_string()),
        ("rustc_channel", b.rustc_channel.to_string()),
        ("cargo_version", b.cargo_version.to_string()),
        ("target", b.target.to_string()),
        ("host", b.host.to_string()),
        ("profile", b.profile.to_string()),
        ("built", b.date.to_string()),
        ("opt_level", b.opt_level.to_string()),
        ("debug_assertions", b.debug_assertions.to_string()),
        ("overflow_checks", b.overflow_checks.to_string()),
        ("panic", b.panic.to_string()),
        ("target_features", join(b.target_features)),
        ("profile_overrides", join(b.profile_overrides)),
        ("features", join(b.features)),
        ("rustflags", b.rustflags.join(" ")),
        ("rustc_wrapper", join(b.rustc_wrapper)),
        ("cc_version", b.cc_version.to_string()),
        ("linker", b.linker.to_string()),
        ("substitutions", join(b.substitutions)),
        ("vcs_commit", b.vcs_commit.to_string()),
        ("vcs_dirty", b.vcs_dirty.to_string()),
        ("vcs_describe", b.vcs_describe.to_string()),
        ("lockfile_digest", b.lockfile_digest.to_string()),
        ("reproducible", b.reproducible.to_string()),
        ("auditable", b.auditable.to_string()),
    ]
}

/// Records 3 and 4: a package list.
pub fn packages_pairs(pkgs: &[PackageInfo]) -> Vec<(&'static str, String)> {
    let entries: Vec<String> = pkgs.iter().map(package_entry).collect();
    vec![
        ("count", pkgs.len().to_string()),
        ("packages", entries.join(", ")),
    ]
}

/// Record 5, `MVL Runtime Environment`.
pub fn env_pairs(e: &RuntimeEnv) -> Vec<(&'static str, String)> {
    let mut v = vec![
        ("os", e.os.clone()),
        ("arch", e.arch.clone()),
        ("os_version", e.os_version.clone()),
        ("pid", e.pid.to_string()),
        ("started_at", e.started_at.clone()),
        ("exe", e.exe_path.clone()),
    ];
    if let Some(libc) = &e.libc_version {
        v.push(("glibc", libc.clone()));
    }
    if let Some(d) = &e.exe_digest {
        v.push(("exe_sha256", d.clone()));
    }
    v.push((
        "loaded_libraries_count",
        e.loaded_libraries.len().to_string(),
    ));
    v.push(("loaded_libraries", e.loaded_libraries.join(",")));
    v
}

// ── JSON ────────────────────────────────────────────────────────────────────

/// Quote and escape a string as a JSON string literal.
pub fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_opt(s: Option<&str>) -> String {
    s.map(json_str).unwrap_or_else(|| "null".to_string())
}

fn json_list(items: &[&str]) -> String {
    let parts: Vec<String> = items.iter().map(|s| json_str(s)).collect();
    format!("[{}]", parts.join(","))
}

fn package_json(p: &PackageInfo) -> String {
    let source = match p.source {
        PackageSource::Registry { url } => {
            format!("{{\"kind\":\"registry\",\"url\":{}}}", json_str(url))
        }
        PackageSource::Git { url, commit } => format!(
            "{{\"kind\":\"git\",\"url\":{},\"commit\":{}}}",
            json_str(url),
            json_str(commit)
        ),
        PackageSource::Path => "{\"kind\":\"path\"}".to_string(),
    };
    format!(
        "{{\"name\":{},\"version\":{},\"purl\":{},\"license\":{},\"kind\":{},\"source\":{},\"checksum\":{}}}",
        json_str(p.name),
        json_str(p.version),
        json_str(p.purl),
        json_str(p.license),
        json_str(p.kind.as_str()),
        source,
        json_str(p.checksum)
    )
}

fn packages_json(pkgs: &[PackageInfo]) -> String {
    let parts: Vec<String> = pkgs.iter().map(package_json).collect();
    format!("[{}]", parts.join(","))
}

fn build_json(b: &BuildInfo) -> String {
    let fields: Vec<String> = vec![
        format!("\"backend\":{}", json_str(b.backend)),
        format!("\"rustc_version\":{}", json_opt(b.rustc_version)),
        format!("\"llvm_version\":{}", json_opt(b.llvm_version)),
        format!("\"target\":{}", json_str(b.target)),
        format!("\"profile\":{}", json_str(b.profile)),
        format!("\"date\":{}", json_str(b.date)),
        format!("\"host\":{}", json_str(b.host)),
        format!("\"rustc_commit\":{}", json_str(b.rustc_commit)),
        format!("\"rustc_channel\":{}", json_str(b.rustc_channel)),
        format!("\"cargo_version\":{}", json_str(b.cargo_version)),
        format!("\"cc_version\":{}", json_str(b.cc_version)),
        format!("\"linker\":{}", json_str(b.linker)),
        format!("\"opt_level\":{}", json_str(b.opt_level)),
        format!("\"debug_assertions\":{}", b.debug_assertions),
        format!("\"panic\":{}", json_str(b.panic)),
        format!("\"target_features\":{}", json_list(b.target_features)),
        format!("\"overflow_checks\":{}", b.overflow_checks),
        format!("\"profile_overrides\":{}", json_list(b.profile_overrides)),
        format!("\"features\":{}", json_list(b.features)),
        format!("\"rustflags\":{}", json_list(b.rustflags)),
        format!("\"rustc_wrapper\":{}", json_list(b.rustc_wrapper)),
        format!("\"substitutions\":{}", json_list(b.substitutions)),
        format!("\"vcs_commit\":{}", json_str(b.vcs_commit)),
        format!("\"vcs_dirty\":{}", b.vcs_dirty),
        format!("\"vcs_describe\":{}", json_str(b.vcs_describe)),
        format!("\"lockfile_digest\":{}", json_str(b.lockfile_digest)),
        format!("\"reproducible\":{}", b.reproducible),
        format!("\"auditable\":{}", b.auditable),
    ];
    format!("{{{}}}", fields.join(","))
}

/// The whole manifest as one JSON object (mvl-lang `manifest_to_json`,
/// extended). mvl-lang's keys come first, in mvl-lang's order.
pub fn manifest_to_json(m: &Manifest) -> String {
    let ffi: Vec<String> = m
        .ffi_bridges
        .iter()
        .map(|f| {
            format!(
                "{{\"abi\":{},\"name\":{},\"version\":{}}}",
                json_str(f.abi),
                json_str(f.bridge_name),
                json_str(f.bridge_version)
            )
        })
        .collect();
    let native: Vec<String> = m
        .native_deps
        .iter()
        .map(|d| {
            format!(
                "{{\"name\":{},\"version\":{},\"kind\":{},\"purl\":{},\"introduced_by\":{}}}",
                json_str(d.name),
                json_str(d.version),
                json_str(match d.kind {
                    crate::types::NativeKind::Rust => "rust",
                    crate::types::NativeKind::C => "c",
                }),
                json_str(d.purl),
                json_list(d.introduced_by)
            )
        })
        .collect();
    let a = &m.assurance;
    let fields: Vec<String> = vec![
        format!("\"app\":{}", json_str(m.app_name)),
        format!("\"app_version\":{}", json_str(m.app_version)),
        format!("\"mvl\":{}", json_str(m.mvl_version)),
        format!("\"runtime\":{}", json_str(m.runtime_version)),
        format!("\"backend\":{}", json_str(m.build.backend)),
        format!("\"built\":{}", json_str(m.build.date)),
        format!("\"rustc\":{}", json_opt(m.build.rustc_version)),
        format!("\"llvm\":{}", json_opt(m.build.llvm_version)),
        format!("\"packages\":{}", packages_json(m.packages)),
        format!("\"ffi\":[{}]", ffi.join(",")),
        format!("\"source_digest\":{}", json_str(m.source_digest)),
        format!("\"build_packages\":{}", packages_json(m.build_packages)),
        format!("\"build\":{}", build_json(&m.build)),
        format!("\"licenses\":{}", json_list(m.licenses)),
        format!("\"native_deps\":[{}]", native.join(",")),
        format!(
            "\"assurance\":{{\"extern_ratio\":{},\"extern_count\":{},\"total_functions\":{},\"requirements_proven\":{}}}",
            a.extern_ratio, a.extern_count, a.total_functions, a.requirements_proven
        ),
    ];
    format!("{{{}}}", fields.join(","))
}

// ── Block ───────────────────────────────────────────────────────────────────

fn block_row(label: &str, value: &str) -> String {
    format!("│ {label:<11}{value}\n")
}

/// The manifest as a human-readable bordered block (mvl-lang
/// `manifest_to_block`, without the stdlib row).
pub fn manifest_to_block(m: &Manifest) -> String {
    let border = "┌─────────────────────────────────────────────────┐\n";
    let sep = "├─────────────────────────────────────────────────┤\n";
    let bottom = "└─────────────────────────────────────────────────┘";
    let mut s = String::new();
    s.push_str(border);
    s.push_str("│ MVL Runtime Manifest\n");
    s.push_str(sep);
    s.push_str(&block_row(
        "app:",
        &format!("{} v{}", m.app_name, m.app_version),
    ));
    s.push_str(&block_row("backend:", m.build.backend));
    s.push_str(&block_row("mvl:", m.mvl_version));
    s.push_str(&block_row("runtime:", m.runtime_version));
    s.push_str(&block_row("rustc:", m.build.rustc_version.unwrap_or("")));
    s.push_str(&block_row(
        "target:",
        &format!("{} ({})", m.build.target, m.build.profile),
    ));
    s.push_str(&block_row("built:", m.build.date));
    s.push_str(&block_row("commit:", m.build.vcs_commit));
    s.push_str(&block_row("lockfile:", m.build.lockfile_digest));
    s.push_str(&block_row("source:", m.source_digest));
    s.push_str(&block_row(
        "packages:",
        &format!(
            "{} runtime, {} build",
            m.packages.len(),
            m.build_packages.len()
        ),
    ));
    s.push_str(bottom);
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata::sample;
    use expect_test::expect;

    #[test]
    fn logfmt_quotes_only_when_needed() {
        let out = logfmt(&[
            ("a", "plain".into()),
            ("b", "".into()),
            ("c", "has space".into()),
            ("d", "q\"uote".into()),
        ]);
        assert_eq!(out, r#"a=plain b="" c="has space" d="q\"uote""#);
    }

    #[test]
    fn golden_logfmt() {
        expect![[r#"app=demo app_version=1.2.3 mvl=0.12.0 runtime=std-1.85.0 backend=mvl-rust target=x86_64-unknown-linux-gnu profile=release built=2026-11-14T00:00:00Z vcs_commit=0123456789abcdef0123456789abcdef01234567 lockfile_digest=aaaa packages="pkg:cargo/log@0.4.33,pkg:cargo/mvl@0.12.0,pkg:cargo/gitdep@0.1.0?vcs_url=git%2Bhttps://example.com/g.git%40c0ffee" build_packages=pkg:cargo/derive-demo@0.1.0 source=bbbb"#]]
        .assert_eq(&manifest_to_logfmt(&sample()));
    }

    #[test]
    fn golden_json() {
        expect![[r#"{"app":"demo","app_version":"1.2.3","mvl":"0.12.0","runtime":"std-1.85.0","backend":"mvl-rust","built":"2026-11-14T00:00:00Z","rustc":"1.85.0","llvm":"19.1.7","packages":[{"name":"log","version":"0.4.33","purl":"pkg:cargo/log@0.4.33","license":"","kind":"runtime","source":{"kind":"registry","url":"https://github.com/rust-lang/crates.io-index"},"checksum":"cccc"},{"name":"mvl","version":"0.12.0","purl":"pkg:cargo/mvl@0.12.0","license":"","kind":"runtime","source":{"kind":"path"},"checksum":""},{"name":"gitdep","version":"0.1.0","purl":"pkg:cargo/gitdep@0.1.0?vcs_url=git%2Bhttps://example.com/g.git%40c0ffee","license":"","kind":"runtime","source":{"kind":"git","url":"https://example.com/g.git","commit":"c0ffee"},"checksum":""}],"ffi":[],"source_digest":"bbbb","build_packages":[{"name":"derive-demo","version":"0.1.0","purl":"pkg:cargo/derive-demo@0.1.0","license":"","kind":"proc-macro","source":{"kind":"path"},"checksum":""}],"build":{"backend":"mvl-rust","rustc_version":"1.85.0","llvm_version":"19.1.7","target":"x86_64-unknown-linux-gnu","profile":"release","date":"2026-11-14T00:00:00Z","host":"x86_64-unknown-linux-gnu","rustc_commit":"4d91de4e48198da2e33413efdcd9cd2cc0c46688","rustc_channel":"stable","cargo_version":"cargo 1.85.0","cc_version":"cc: gcc 13.2.0","linker":"cc: gcc 13.2.0","opt_level":"3","debug_assertions":false,"panic":"unwind","target_features":["fxsr","sse","sse2"],"overflow_checks":false,"profile_overrides":[],"features":["default"],"rustflags":["--remap-path-prefix=<redacted>=/build"],"rustc_wrapper":[],"substitutions":[],"vcs_commit":"0123456789abcdef0123456789abcdef01234567","vcs_dirty":false,"vcs_describe":"v1.2.3","lockfile_digest":"aaaa","reproducible":true,"auditable":true},"licenses":[],"native_deps":[],"assurance":{"extern_ratio":0,"extern_count":0,"total_functions":0,"requirements_proven":0}}"#]]
        .assert_eq(&manifest_to_json(&sample()));
    }

    #[test]
    fn golden_block() {
        expect![[r#"
            ┌─────────────────────────────────────────────────┐
            │ MVL Runtime Manifest
            ├─────────────────────────────────────────────────┤
            │ app:       demo v1.2.3
            │ backend:   mvl-rust
            │ mvl:       0.12.0
            │ runtime:   std-1.85.0
            │ rustc:     1.85.0
            │ target:    x86_64-unknown-linux-gnu (release)
            │ built:     2026-11-14T00:00:00Z
            │ commit:    0123456789abcdef0123456789abcdef01234567
            │ lockfile:  aaaa
            │ source:    bbbb
            │ packages:  3 runtime, 1 build
            └─────────────────────────────────────────────────┘"#]]
        .assert_eq(&manifest_to_block(&sample()));
    }

    #[test]
    fn json_escapes_control_and_quote() {
        assert_eq!(json_str("a\"b\\c\n\u{1}"), r#""a\"b\\c\n\u0001""#);
    }

    #[test]
    fn package_records_show_source_and_checksum() {
        let m = sample();
        let pairs = packages_pairs(m.packages);
        assert_eq!(pairs[0], ("count", "3".to_string()));
        assert!(pairs[1]
            .1
            .starts_with("pkg:cargo/log@0.4.33 registry+https://github.com/rust-lang/crates.io-index sha256=cccc, "));
    }
}
