//! The verification record: one delimited string embedded in the binary, so a
//! verifier can read the manifest's join fields without running the program
//! (`mvl-attest`, ADR-0014). Dependency-free; the format is shared by the
//! generator (which writes it) and the verifier (which scans for it).
//!
//! ```text
//! mvl-manifest-record/v1;app=demo;app_version=1.2.3;vcs_commit=<hex>;vcs_dirty=false;
//!   lockfile_digest=<hex>;source_digest=<hex>;auditable=true;reproducible=true;end
//! ```
//! (one line; wrapped here for width). Keys and values never contain `;` or `=`.

/// Marks the start of a record in the binary's bytes.
pub const MARKER: &str = "mvl-manifest-record/v1;";
/// Marks the end of a record.
pub const END: &str = ";end";

/// The fields a record carries, in order.
pub const FIELDS: &[&str] = &[
    "app",
    "app_version",
    "vcs_commit",
    "vcs_dirty",
    "lockfile_digest",
    "source_digest",
    "auditable",
    "reproducible",
];

/// Render `(key, value)` pairs as a record. `;` and `=` in values are dropped,
/// since they cannot occur in the fields this format carries.
pub fn render(pairs: &[(&str, &str)]) -> String {
    let mut s = String::from(MARKER);
    for (k, v) in pairs {
        let v: String = v.chars().filter(|c| *c != ';' && *c != '=').collect();
        s.push_str(k);
        s.push('=');
        s.push_str(&v);
        s.push(';');
    }
    s.push_str(&END[1..]);
    s
}

/// Parse a record (starting at [`MARKER`]) into `(key, value)` pairs.
/// Returns `None` if it is not a complete record.
pub fn parse(s: &str) -> Option<Vec<(String, String)>> {
    let body = s.strip_prefix(MARKER)?;
    let end = body.find("end")?;
    let body = body[..end].strip_suffix(';').unwrap_or(&body[..end]);
    if body.is_empty() {
        return Some(Vec::new());
    }
    body.split(';')
        .map(|kv| {
            let (k, v) = kv.split_once('=')?;
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_and_parse_round_trip() {
        let r = render(&[("app", "demo"), ("vcs_dirty", "false"), ("x", "a;b=c")]);
        assert_eq!(
            r,
            "mvl-manifest-record/v1;app=demo;vcs_dirty=false;x=abc;end"
        );
        assert_eq!(
            parse(&r).unwrap(),
            [
                ("app".to_string(), "demo".to_string()),
                ("vcs_dirty".to_string(), "false".to_string()),
                ("x".to_string(), "abc".to_string())
            ]
        );
    }

    #[test]
    fn incomplete_record_does_not_parse() {
        assert!(parse("mvl-manifest-record/v1;app=demo;").is_none());
        assert!(parse("something else").is_none());
    }
}
