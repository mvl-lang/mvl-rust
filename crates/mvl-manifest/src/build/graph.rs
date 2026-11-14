//! Package graph from `cargo metadata`: runtime vs build packages, sources,
//! checksums and purls.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use super::{run, Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnedSource {
    Registry { url: String },
    Git { url: String, commit: String },
    Path,
    Workspace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedPackage {
    pub name: String,
    pub version: String,
    pub purl: String,
    /// `runtime`, `build` or `proc-macro`.
    pub kind: &'static str,
    pub source: OwnedSource,
    pub checksum: String,
}

/// A package whose sources are on disk and therefore go into `source_digest`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalPackage {
    pub name: String,
    pub manifest_path: PathBuf,
    /// A member of the app's workspace (the app itself included). Only
    /// members are listed with `cargo package --list`: for a path dependency
    /// in another workspace, that would resolve the other workspace's whole
    /// graph, which the outer build never fetched.
    pub member: bool,
}

#[derive(Debug, Default)]
pub struct Graph {
    pub workspace_root: PathBuf,
    pub packages: Vec<OwnedPackage>,
    pub build_packages: Vec<OwnedPackage>,
    /// The app crate plus every reached path dependency.
    pub local_packages: Vec<LocalPackage>,
    /// The app crate's enabled features.
    pub features: Vec<String>,
}

fn metadata(cargo: &str, manifest: &Path, extra: &[String]) -> Result<Value> {
    let mut cmd = Command::new(cargo);
    cmd.args(["metadata", "--format-version", "1", "--offline", "--frozen"])
        .arg("--manifest-path")
        .arg(manifest)
        .args(extra);
    let out = run(&mut cmd)?;
    serde_json::from_str(&out).map_err(|e| Error::new(format!("cargo metadata output: {e}")))
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn find_package<'a>(meta: &'a Value, manifest: &Path) -> Option<&'a Value> {
    meta["packages"].as_array()?.iter().find(|p| {
        p["manifest_path"]
            .as_str()
            .is_some_and(|m| same_path(Path::new(m), manifest))
    })
}

/// Map `CARGO_FEATURE_*` back to declared feature names.
pub fn enabled_features(declared: &[String], env: &[(String, String)]) -> Vec<String> {
    let set: BTreeSet<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
    let mut out: Vec<String> = declared
        .iter()
        .filter(|f| {
            let key = format!("CARGO_FEATURE_{}", f.to_uppercase().replace('-', "_"));
            set.contains(key.as_str())
        })
        .cloned()
        .collect();
    out.sort();
    out
}

pub fn resolve(
    cargo: &str,
    manifest: &Path,
    target: &str,
    features_env: &[(String, String)],
) -> Result<Graph> {
    let shallow = metadata(cargo, manifest, &["--no-deps".to_string()])?;
    let root = find_package(&shallow, manifest)
        .ok_or_else(|| Error::new("app package not found in cargo metadata"))?;
    let declared: Vec<String> = root["features"]
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    let features = enabled_features(&declared, features_env);

    let mut extra = vec![
        "--filter-platform".to_string(),
        target.to_string(),
        "--no-default-features".to_string(),
    ];
    if !features.is_empty() {
        extra.push("--features".to_string());
        extra.push(features.join(","));
    }
    let meta = metadata(cargo, manifest, &extra)?;
    let workspace_root = PathBuf::from(meta["workspace_root"].as_str().unwrap_or("."));
    let lock = std::fs::read_to_string(workspace_root.join("Cargo.lock"))
        .ok()
        .and_then(|s| s.parse::<toml::Table>().ok());
    let linked = runtime_tree(cargo, manifest, target, &features)?;
    let mut g = classify(&meta, manifest, lock.as_ref(), Some(&linked))?;
    g.workspace_root = workspace_root;
    g.features = features;
    Ok(g)
}

/// Packages linked into the target, from `cargo tree`.
///
/// `cargo metadata` has one node per package with the union of all features,
/// so a crate used as both a dependency and a build-dependency (with
/// different features) pulls its build-only optional deps into the runtime
/// walk. `cargo tree` applies the real resolver-2 feature split per context.
fn runtime_tree(
    cargo: &str,
    manifest: &Path,
    target: &str,
    features: &[String],
) -> Result<BTreeSet<(String, String)>> {
    let mut cmd = Command::new(cargo);
    cmd.args([
        "tree",
        "--offline",
        "--frozen",
        "--prefix",
        "none",
        "--charset",
        "ascii",
    ])
    .args([
        "-e",
        "normal,no-proc-macro",
        "--target",
        target,
        "-f",
        "{p}",
    ])
    .arg("--manifest-path")
    .arg(manifest)
    .arg("--no-default-features");
    if !features.is_empty() {
        cmd.arg("--features").arg(features.join(","));
    }
    Ok(parse_tree(&run(&mut cmd)?))
}

/// `name vX.Y.Z [(source)] [(*)]` lines → `(name, version)`.
pub fn parse_tree(out: &str) -> BTreeSet<(String, String)> {
    out.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let name = it.next()?;
            let version = it.next()?.strip_prefix('v')?;
            Some((name.to_string(), version.to_string()))
        })
        .collect()
}

/// `(name, version, source)` → checksum, from a parsed `Cargo.lock`.
fn lock_checksums(lock: Option<&toml::Table>) -> HashMap<(String, String, String), String> {
    let mut m = HashMap::new();
    let Some(pkgs) = lock
        .and_then(|l| l.get("package"))
        .and_then(|p| p.as_array())
    else {
        return m;
    };
    for p in pkgs {
        let s = |k: &str| p.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        if !s("checksum").is_empty() {
            m.insert((s("name"), s("version"), s("source")), s("checksum"));
        }
    }
    m
}

pub fn parse_source(src: Option<&str>) -> OwnedSource {
    match src {
        None => OwnedSource::Path,
        Some(s) => {
            if let Some(rest) = s.strip_prefix("git+") {
                let (url, commit) = rest.split_once('#').unwrap_or((rest, ""));
                OwnedSource::Git {
                    url: url.to_string(),
                    commit: commit.to_string(),
                }
            } else if let Some(rest) = s.strip_prefix("registry+") {
                OwnedSource::Registry {
                    url: rest.to_string(),
                }
            } else {
                // `sparse+…` and anything newer: keep the protocol prefix.
                OwnedSource::Registry { url: s.to_string() }
            }
        }
    }
}

/// Percent-encode a purl qualifier value.
fn pct(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~:/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn purl(name: &str, version: &str, source: &OwnedSource) -> String {
    let base = format!("pkg:cargo/{name}@{version}");
    match source {
        OwnedSource::Git { url, commit } => {
            let bare = url.split('?').next().unwrap_or(url);
            format!("{base}?vcs_url={}", pct(&format!("git+{bare}@{commit}")))
        }
        _ => base,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Edge {
    Normal,
    Build,
}

fn edges(node: &Value) -> Vec<(String, Vec<Edge>)> {
    let mut out = Vec::new();
    for d in node["deps"].as_array().into_iter().flatten() {
        let id = d["pkg"].as_str().unwrap_or("").to_string();
        let kinds: Vec<Edge> = d["dep_kinds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|k| match k["kind"].as_str() {
                None => Some(Edge::Normal),
                Some("build") => Some(Edge::Build),
                _ => None, // dev-deps are excluded
            })
            .collect();
        if !kinds.is_empty() {
            out.push((id, kinds));
        }
    }
    out
}

/// Split the resolved graph into runtime-linked packages and packages that
/// only run during the build (build-deps, proc-macros and their deps).
///
/// `linked`, when given, is the authoritative runtime set from `cargo tree`;
/// the runtime walk does not leave it.
pub fn classify(
    meta: &Value,
    manifest: &Path,
    lock: Option<&toml::Table>,
    linked: Option<&BTreeSet<(String, String)>>,
) -> Result<Graph> {
    let root = find_package(meta, manifest)
        .ok_or_else(|| Error::new("app package not found in cargo metadata"))?;
    let root_id = root["id"].as_str().unwrap_or("").to_string();
    let pkgs: HashMap<&str, &Value> = meta["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| Some((p["id"].as_str()?, p)))
        .collect();
    let nodes: HashMap<&str, &Value> = meta["resolve"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|n| Some((n["id"].as_str()?, n)))
        .collect();
    let in_linked = |id: &str| {
        let Some(set) = linked else { return true };
        pkgs.get(id).is_some_and(|p| {
            let key = (
                p["name"].as_str().unwrap_or("").to_string(),
                p["version"].as_str().unwrap_or("").to_string(),
            );
            set.contains(&key)
        })
    };
    let is_proc_macro = |id: &str| {
        pkgs.get(id).is_some_and(|p| {
            p["targets"].as_array().into_iter().flatten().any(|t| {
                t["kind"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|k| k == "proc-macro")
            })
        })
    };

    // Runtime: normal edges from the root, stopping at proc-macros.
    let mut runtime: BTreeSet<String> = BTreeSet::new();
    let mut build: BTreeSet<String> = BTreeSet::new();
    let mut build_roots: Vec<String> = Vec::new();
    let mut queue: VecDeque<String> = VecDeque::from([root_id.clone()]);
    let mut seen: BTreeSet<String> = BTreeSet::from([root_id.clone()]);
    while let Some(id) = queue.pop_front() {
        let Some(node) = nodes.get(id.as_str()) else {
            continue;
        };
        for (dep, kinds) in edges(node) {
            if kinds.contains(&Edge::Build) {
                build_roots.push(dep.clone());
            }
            if kinds.contains(&Edge::Normal) {
                if is_proc_macro(&dep) {
                    build_roots.push(dep);
                } else if in_linked(&dep) && seen.insert(dep.clone()) {
                    runtime.insert(dep.clone());
                    queue.push_back(dep);
                }
            }
        }
    }
    // Build: everything reachable from a build root, over any edge.
    let mut queue: VecDeque<String> = build_roots.into_iter().collect();
    while let Some(id) = queue.pop_front() {
        if !build.insert(id.clone()) {
            continue;
        }
        if let Some(node) = nodes.get(id.as_str()) {
            for (dep, _) in edges(node) {
                queue.push_back(dep);
            }
        }
    }

    let checksums = lock_checksums(lock);
    let members: BTreeSet<&str> = meta["workspace_members"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m.as_str())
        .collect();
    let to_pkg = |id: &str, kind: &'static str| -> Option<OwnedPackage> {
        let p = pkgs.get(id)?;
        let name = p["name"].as_str()?.to_string();
        let version = p["version"].as_str()?.to_string();
        let raw = p["source"].as_str();
        let source = match parse_source(raw) {
            OwnedSource::Path if members.contains(id) => OwnedSource::Workspace,
            other => other,
        };
        let checksum = checksums
            .get(&(name.clone(), version.clone(), raw.unwrap_or("").to_string()))
            .cloned()
            .unwrap_or_default();
        Some(OwnedPackage {
            purl: purl(&name, &version, &source),
            name,
            version,
            kind,
            source,
            checksum,
        })
    };
    let sorted = |mut v: Vec<OwnedPackage>| {
        v.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
        v
    };
    let packages = sorted(
        runtime
            .iter()
            .filter_map(|id| to_pkg(id, "runtime"))
            .collect(),
    );
    let build_packages = sorted(
        build
            .iter()
            .filter(|id| **id != root_id)
            .filter_map(|id| {
                to_pkg(
                    id,
                    if is_proc_macro(id) {
                        "proc-macro"
                    } else {
                        "build"
                    },
                )
            })
            .collect(),
    );

    // Local packages: the root plus every reached package without a source.
    let mut local: BTreeMap<String, LocalPackage> = BTreeMap::new();
    for id in std::iter::once(&root_id)
        .chain(runtime.iter())
        .chain(build.iter())
    {
        let Some(p) = pkgs.get(id.as_str()) else {
            continue;
        };
        if p["source"].is_null() {
            let name = p["name"].as_str().unwrap_or("").to_string();
            local.insert(
                name.clone(),
                LocalPackage {
                    name,
                    manifest_path: PathBuf::from(p["manifest_path"].as_str().unwrap_or("")),
                    member: *id == root_id || members.contains(id.as_str()),
                },
            );
        }
    }

    Ok(Graph {
        workspace_root: PathBuf::new(),
        packages,
        build_packages,
        local_packages: local.into_values().collect(),
        features: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";

    fn pkg(name: &str, source: Option<&str>, proc_macro: bool) -> Value {
        let kind = if proc_macro { "proc-macro" } else { "lib" };
        json!({
            "id": format!("{name}-id"),
            "name": name,
            "version": "1.0.0",
            "source": source,
            "manifest_path": format!("/ws/{name}/Cargo.toml"),
            "targets": [{"kind": [kind]}],
        })
    }

    fn dep(name: &str, kind: Option<&str>) -> Value {
        json!({"pkg": format!("{name}-id"), "dep_kinds": [{"kind": kind}]})
    }

    fn fixture() -> Value {
        json!({
            "packages": [
                pkg("app", None, false),
                pkg("log", Some(CRATES_IO), false),
                pkg("cc", Some(CRATES_IO), false),
                pkg("derive", None, true),
                pkg("syn", Some(CRATES_IO), false),
                pkg("testonly", Some(CRATES_IO), false),
                pkg("gitdep", Some("git+https://example.com/g.git?branch=main#c0ffee"), false),
            ],
            "resolve": {"nodes": [
                {"id": "app-id", "deps": [
                    dep("log", None), dep("cc", Some("build")), dep("derive", None),
                    dep("testonly", Some("dev")), dep("gitdep", None),
                ]},
                {"id": "derive-id", "deps": [dep("syn", None)]},
                {"id": "log-id", "deps": []},
                {"id": "cc-id", "deps": []},
                {"id": "syn-id", "deps": []},
                {"id": "gitdep-id", "deps": []},
            ]},
        })
    }

    fn lock() -> toml::Table {
        format!(
            "[[package]]\nname = \"log\"\nversion = \"1.0.0\"\nsource = \"{CRATES_IO}\"\nchecksum = \"abc\"\n"
        )
        .parse()
        .unwrap()
    }

    fn names(v: &[OwnedPackage]) -> Vec<(&str, &str)> {
        v.iter().map(|p| (p.name.as_str(), p.kind)).collect()
    }

    #[test]
    fn splits_runtime_and_build_and_drops_dev() {
        let g = classify(
            &fixture(),
            Path::new("/ws/app/Cargo.toml"),
            Some(&lock()),
            None,
        )
        .unwrap();
        assert_eq!(
            names(&g.packages),
            [("gitdep", "runtime"), ("log", "runtime")]
        );
        assert_eq!(
            names(&g.build_packages),
            [("cc", "build"), ("derive", "proc-macro"), ("syn", "build")]
        );
        let local: Vec<(&str, bool)> = g
            .local_packages
            .iter()
            .map(|p| (p.name.as_str(), p.member))
            .collect();
        // The fixture declares no workspace members: only the app is one.
        assert_eq!(local, [("app", true), ("derive", false)]);
    }

    #[test]
    fn registry_package_carries_lock_checksum() {
        let g = classify(
            &fixture(),
            Path::new("/ws/app/Cargo.toml"),
            Some(&lock()),
            None,
        )
        .unwrap();
        let log = g.packages.iter().find(|p| p.name == "log").unwrap();
        assert_eq!(log.checksum, "abc");
        assert_eq!(log.purl, "pkg:cargo/log@1.0.0");
        assert_eq!(
            log.source,
            OwnedSource::Registry {
                url: "https://github.com/rust-lang/crates.io-index".into()
            }
        );
    }

    #[test]
    fn git_package_records_commit_and_vcs_url() {
        let g = classify(&fixture(), Path::new("/ws/app/Cargo.toml"), None, None).unwrap();
        let gd = g.packages.iter().find(|p| p.name == "gitdep").unwrap();
        assert_eq!(
            gd.source,
            OwnedSource::Git {
                url: "https://example.com/g.git?branch=main".into(),
                commit: "c0ffee".into()
            }
        );
        assert_eq!(
            gd.purl,
            "pkg:cargo/gitdep@1.0.0?vcs_url=git%2Bhttps://example.com/g.git%40c0ffee"
        );
        assert_eq!(gd.checksum, "");
    }

    #[test]
    fn linked_set_prunes_feature_union_leaks() {
        // `cc` reached through a normal edge in the metadata union, but cargo
        // tree says it is not linked for the target.
        let mut meta = fixture();
        meta["resolve"]["nodes"][2]["deps"] = json!([dep("cc", None)]);
        let linked: BTreeSet<(String, String)> = ["gitdep", "log"]
            .iter()
            .map(|n| (n.to_string(), "1.0.0".to_string()))
            .collect();
        let g = classify(&meta, Path::new("/ws/app/Cargo.toml"), None, Some(&linked)).unwrap();
        assert_eq!(
            names(&g.packages),
            [("gitdep", "runtime"), ("log", "runtime")]
        );
    }

    #[test]
    fn parses_cargo_tree_lines() {
        let out =
            "app v0.1.0 (/ws/app)\nlog v0.4.33\ngitdep v0.1.0 (file:///r#abc)\nlog v0.4.33 (*)\n";
        let set = parse_tree(out);
        assert!(set.contains(&("log".to_string(), "0.4.33".to_string())));
        assert!(set.contains(&("gitdep".to_string(), "0.1.0".to_string())));
        assert_eq!(set.len(), 3);
    }

    #[test]
    fn features_map_back_from_env() {
        let declared = vec!["default".into(), "fancy-io".into(), "off".into()];
        let env = vec![
            ("CARGO_FEATURE_DEFAULT".into(), "1".into()),
            ("CARGO_FEATURE_FANCY_IO".into(), "1".into()),
        ];
        assert_eq!(enabled_features(&declared, &env), ["default", "fancy-io"]);
    }

    #[test]
    fn sparse_registry_keeps_protocol() {
        assert_eq!(
            parse_source(Some("sparse+https://index.crates.io/")),
            OwnedSource::Registry {
                url: "sparse+https://index.crates.io/".into()
            }
        );
    }
}
