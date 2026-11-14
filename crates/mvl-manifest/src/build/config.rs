//! `.cargo/config.toml` discovery, dependency substitutions and profile
//! resolution.

use std::path::{Path, PathBuf};

use super::Tracker;

/// One Cargo config file in effect, highest priority first.
#[derive(Debug, Clone)]
pub struct ConfigFile {
    /// Location-independent key used in `source_digest`.
    pub key: String,
    pub path: PathBuf,
    pub table: toml::Table,
}

pub fn read_toml(path: &Path) -> Option<toml::Table> {
    std::fs::read_to_string(path).ok()?.parse().ok()
}

fn rel_key(path: &Path, workspace_root: &Path) -> String {
    if let Ok(rel) = path.strip_prefix(workspace_root) {
        return format!("workspace/{}", rel.display());
    }
    // An ancestor of the workspace: name it by how many levels up it is, so
    // the key does not depend on where the checkout lives.
    let mut levels = 0;
    let mut dir = workspace_root;
    while let Some(parent) = dir.parent() {
        levels += 1;
        if let Ok(rel) = path.strip_prefix(parent) {
            return format!("ancestor-{levels}/{}", rel.display());
        }
        dir = parent;
    }
    format!("external/{}", path.display())
}

/// Config files Cargo reads, nearest directory first, then `$CARGO_HOME`.
/// Discovery starts at the crate directory: the directory Cargo was invoked
/// from is not visible to a build script.
pub fn discover(t: &mut Tracker, manifest_dir: &Path, workspace_root: &Path) -> Vec<ConfigFile> {
    let mut out: Vec<ConfigFile> = Vec::new();
    let mut push = |key: String, path: PathBuf, t: &mut Tracker| {
        if out.iter().any(|c| c.path == path) {
            return;
        }
        if let Some(table) = read_toml(&path) {
            t.file(&path);
            out.push(ConfigFile { key, path, table });
        }
    };
    for dir in manifest_dir.ancestors() {
        for name in ["config.toml", "config"] {
            let p = dir.join(".cargo").join(name);
            if p.is_file() {
                push(rel_key(&p, workspace_root), p, t);
            }
        }
    }
    let home = t
        .env("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")));
    if let Some(home) = home {
        for name in ["config.toml", "config"] {
            let p = home.join(name);
            if p.is_file() {
                push(format!("cargo-home/{name}"), p, t);
            }
        }
    }
    out
}

/// `rust-toolchain.toml` / `rust-toolchain`, nearest first.
pub fn toolchain_file(manifest_dir: &Path, workspace_root: &Path) -> Option<(String, PathBuf)> {
    for dir in manifest_dir.ancestors() {
        for name in ["rust-toolchain.toml", "rust-toolchain"] {
            let p = dir.join(name);
            if p.is_file() {
                return Some((rel_key(&p, workspace_root), p));
            }
        }
    }
    None
}

/// Render a TOML value compactly and deterministically.
fn render(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        toml::Value::Table(t) => {
            let parts: Vec<String> = t
                .iter()
                .map(|(k, v)| format!("{k}={}", render(v)))
                .collect();
            parts.join(",")
        }
        toml::Value::Array(a) => {
            let parts: Vec<String> = a.iter().map(render).collect();
            format!("[{}]", parts.join(","))
        }
        other => other.to_string(),
    }
}

fn patches(table: &toml::Table, origin: &str, out: &mut Vec<String>) {
    if let Some(patch) = table.get("patch").and_then(|v| v.as_table()) {
        for (registry, entries) in patch {
            for (name, spec) in entries.as_table().into_iter().flatten() {
                out.push(format!(
                    "{origin}: patch.{registry}.{name} -> {}",
                    render(spec)
                ));
            }
        }
    }
}

/// Dependency swaps `Cargo.lock` does not show: `[patch]` and `[replace]` in
/// the workspace manifest; `[patch]`, `paths` and `source` replacement in
/// config files.
pub fn substitutions(root_manifest: Option<&toml::Table>, configs: &[ConfigFile]) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(m) = root_manifest {
        patches(m, "Cargo.toml", &mut out);
        if let Some(rep) = m.get("replace").and_then(|v| v.as_table()) {
            for (spec, to) in rep {
                out.push(format!("Cargo.toml: replace.{spec} -> {}", render(to)));
            }
        }
    }
    for c in configs {
        patches(&c.table, &c.key, &mut out);
        if let Some(paths) = c.table.get("paths").and_then(|v| v.as_array()) {
            for p in paths {
                out.push(format!("{}: paths -> {}", c.key, render(p)));
            }
        }
        if let Some(sources) = c.table.get("source").and_then(|v| v.as_table()) {
            for (name, def) in sources {
                if let Some(with) = def.get("replace-with").and_then(|v| v.as_str()) {
                    let target = sources
                        .get(with)
                        .map(render)
                        .unwrap_or_else(|| "?".to_string());
                    out.push(format!("{}: source.{name} -> {with} ({target})", c.key));
                }
            }
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub overflow_checks: bool,
    pub overrides: Vec<String>,
}

fn profile_table<'a>(t: &'a toml::Table, name: &str) -> Option<&'a toml::Table> {
    t.get("profile")?.get(name)?.as_table()
}

/// The profile chain: `name`, its `inherits`, down to `dev` or `release`.
fn chain(name: &str, sources: &[&toml::Table]) -> Vec<String> {
    let mut out = vec![name.to_string()];
    let mut cur = name.to_string();
    while out.len() < 16 {
        let next = match cur.as_str() {
            "dev" | "release" => break,
            "test" => "dev".to_string(),
            "bench" => "release".to_string(),
            _ => match sources
                .iter()
                .find_map(|s| profile_table(s, &cur)?.get("inherits")?.as_str())
            {
                Some(i) => i.to_string(),
                None => break,
            },
        };
        out.push(next.clone());
        cur = next;
    }
    out
}

/// Resolve `overflow-checks` and per-package overrides for `name`.
/// Priority per profile level: env (`CARGO_PROFILE_<P>_OVERFLOW_CHECKS`),
/// config files (nearest first), then the workspace manifest.
pub fn resolve_profile(
    t: &mut Tracker,
    name: &str,
    root_manifest: Option<&toml::Table>,
    configs: &[ConfigFile],
) -> Profile {
    let mut sources: Vec<&toml::Table> = configs.iter().map(|c| &c.table).collect();
    if let Some(m) = root_manifest {
        sources.push(m);
    }
    let chain = chain(name, &sources);
    let mut overflow: Option<bool> = None;
    for p in &chain {
        let var = format!(
            "CARGO_PROFILE_{}_OVERFLOW_CHECKS",
            p.to_uppercase().replace('-', "_")
        );
        let from_env = t.env(&var).and_then(|v| v.parse::<bool>().ok());
        if overflow.is_none() {
            overflow = from_env.or_else(|| {
                sources
                    .iter()
                    .find_map(|s| profile_table(s, p)?.get("overflow-checks")?.as_bool())
            });
        }
    }
    let base = chain.last().map(String::as_str).unwrap_or("dev");
    let overflow_checks = overflow.unwrap_or(base != "release");

    let mut overrides = Vec::new();
    for p in &chain {
        for s in &sources {
            let Some(pt) = profile_table(s, p) else {
                continue;
            };
            if let Some(pkgs) = pt.get("package").and_then(|v| v.as_table()) {
                for (pkg, settings) in pkgs {
                    for (k, v) in settings.as_table().into_iter().flatten() {
                        overrides.push(format!("profile.{p}.package.{pkg}.{k}={}", render(v)));
                    }
                }
            }
            if let Some(bo) = pt.get("build-override").and_then(|v| v.as_table()) {
                for (k, v) in bo {
                    overrides.push(format!("profile.{p}.build-override.{k}={}", render(v)));
                }
            }
        }
    }
    overrides.sort();
    overrides.dedup();
    Profile {
        overflow_checks,
        overrides,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(key: &str, src: &str) -> ConfigFile {
        ConfigFile {
            key: key.into(),
            path: PathBuf::from(key),
            table: src.parse().unwrap(),
        }
    }

    #[test]
    fn manifest_patch_and_replace_are_substitutions() {
        let m: toml::Table = r#"
            [patch.crates-io]
            log = { path = "../log" }
            [replace]
            "foo:1.0.0" = { git = "https://example.com/foo", rev = "abc" }
        "#
        .parse()
        .unwrap();
        assert_eq!(
            substitutions(Some(&m), &[]),
            [
                "Cargo.toml: patch.crates-io.log -> path=../log",
                "Cargo.toml: replace.foo:1.0.0 -> git=https://example.com/foo,rev=abc",
            ]
        );
    }

    #[test]
    fn config_paths_patch_and_source_replacement_are_substitutions() {
        let c = cfg(
            "workspace/.cargo/config.toml",
            r#"
            paths = ["../override"]
            [patch.crates-io]
            serde = { path = "../serde" }
            [source.crates-io]
            replace-with = "vendored"
            [source.vendored]
            directory = "vendor"
        "#,
        );
        assert_eq!(
            substitutions(None, &[c]),
            [
                "workspace/.cargo/config.toml: patch.crates-io.serde -> path=../serde",
                "workspace/.cargo/config.toml: paths -> ../override",
                "workspace/.cargo/config.toml: source.crates-io -> vendored (directory=vendor)",
            ]
        );
    }

    #[test]
    fn overflow_checks_follow_inherits_and_defaults() {
        let m: toml::Table = r#"
            [profile.release-checked]
            inherits = "release"
            overflow-checks = true
            [profile.release-lto]
            inherits = "release"
            [profile.release.package.foo]
            opt-level = 0
        "#
        .parse()
        .unwrap();
        let mut t = Tracker::default();
        assert!(resolve_profile(&mut t, "release-checked", Some(&m), &[]).overflow_checks);
        let lto = resolve_profile(&mut t, "release-lto", Some(&m), &[]);
        assert!(!lto.overflow_checks);
        assert_eq!(lto.overrides, ["profile.release.package.foo.opt-level=0"]);
        assert!(resolve_profile(&mut t, "dev", Some(&m), &[]).overflow_checks);
    }

    #[test]
    fn config_profile_beats_manifest() {
        let m: toml::Table = "[profile.release]\noverflow-checks = false\n"
            .parse()
            .unwrap();
        let c = cfg("c", "[profile.release]\noverflow-checks = true\n");
        let mut t = Tracker::default();
        assert!(resolve_profile(&mut t, "release", Some(&m), &[c]).overflow_checks);
    }

    #[test]
    fn config_keys_do_not_depend_on_checkout_location() {
        let ws = Path::new("/a/b/ws");
        assert_eq!(
            rel_key(Path::new("/a/b/ws/.cargo/config.toml"), ws),
            "workspace/.cargo/config.toml"
        );
        assert_eq!(
            rel_key(Path::new("/a/.cargo/config.toml"), ws),
            "ancestor-2/.cargo/config.toml"
        );
    }
}
