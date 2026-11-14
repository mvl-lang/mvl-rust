//! The dependency list `cargo auditable` embeds in a binary.

use std::path::Path;

use serde_json::Value;

/// One embedded package: name, version and source (`crates.io`, `git`,
/// `local`, `registry`, as cargo-auditable names them).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Dep {
    pub name: String,
    pub version: String,
    pub source: String,
}

/// The embedded audit data: its digest and its packages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Audit {
    /// sha256 of the embedded JSON, as extracted.
    pub digest: String,
    pub packages: Vec<Dep>,
}

/// Parse cargo-auditable's JSON.
pub fn parse(json: &str) -> Result<Audit, String> {
    let v: Value =
        serde_json::from_str(json).map_err(|e| format!("cargo-auditable data is not JSON: {e}"))?;
    let mut packages: Vec<Dep> = v["packages"]
        .as_array()
        .ok_or("cargo-auditable data has no package list")?
        .iter()
        .map(|p| Dep {
            name: p["name"].as_str().unwrap_or("").to_string(),
            version: p["version"].as_str().unwrap_or("").to_string(),
            source: p["source"].as_str().unwrap_or("").to_string(),
        })
        .collect();
    packages.sort();
    Ok(Audit {
        digest: crate::sha256_hex(json.as_bytes()),
        packages,
    })
}

/// Read the audit data from a binary. A binary built without
/// `cargo auditable` has none, which is an error here.
pub fn read(path: &Path) -> Result<Audit, String> {
    let json = auditable_info::json_from_file(path, Default::default()).map_err(|e| {
        format!(
            "no cargo-auditable data in {} ({e}); build it with `cargo auditable build --locked`",
            path.display()
        )
    })?;
    parse(&json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_packages_sorted() {
        let a = parse(r#"{"packages":[{"name":"log","version":"0.4.33","source":"crates.io"},{"name":"demo","version":"0.1.0","source":"local","root":true}]}"#).unwrap();
        assert_eq!(a.packages[0].name, "demo");
        assert_eq!(a.packages[1].source, "crates.io");
        assert_eq!(a.digest.len(), 64);
    }

    #[test]
    fn binary_without_auditable_data_is_rejected() {
        // This test executable is a real binary built without cargo auditable.
        let exe = std::env::current_exe().unwrap();
        let err = read(&exe).unwrap_err();
        assert!(err.contains("no cargo-auditable data"), "{err}");
    }
}
