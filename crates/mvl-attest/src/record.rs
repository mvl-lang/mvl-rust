//! Find the `mvl-manifest` verification record in a binary's bytes.

use std::collections::BTreeMap;

use mvl_manifest::record::{parse, END, MARKER};

/// The record's fields, by key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record(pub BTreeMap<String, String>);

impl Record {
    /// A field's value, or `""` when absent.
    pub fn get(&self, key: &str) -> &str {
        self.0.get(key).map(String::as_str).unwrap_or("")
    }
}

/// Upper bound on a record's length, so a stray marker cannot make the scan
/// read the whole binary as one record.
const MAX_LEN: usize = 4096;

/// Every complete record in `bytes`. A binary normally holds exactly one;
/// copies of the same record (e.g. from inlining) count once.
pub fn find(bytes: &[u8]) -> Result<Record, String> {
    let marker = MARKER.as_bytes();
    let end = END.as_bytes();
    let mut found: Vec<Record> = Vec::new();
    let mut i = 0;
    while let Some(pos) = find_from(bytes, marker, i) {
        let window = &bytes[pos..bytes.len().min(pos + MAX_LEN)];
        if let Some(e) = find_from(window, end, marker.len()) {
            if let Ok(text) = std::str::from_utf8(&window[..e + end.len()]) {
                if let Some(pairs) = parse(text) {
                    let rec = Record(pairs.into_iter().collect());
                    if !found.contains(&rec) {
                        found.push(rec);
                    }
                }
            }
        }
        i = pos + marker.len();
    }
    match found.len() {
        0 => Err(
            "no mvl-manifest verification record in the binary (it must call \
                  mvl_manifest::log_manifest, mvl-manifest >= 0.13.0)"
                .to_string(),
        ),
        1 => Ok(found.remove(0)),
        n => Err(format!(
            "{n} conflicting mvl-manifest records in the binary"
        )),
    }
}

fn find_from(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from >= hay.len() || needle.is_empty() {
        return None;
    }
    hay[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binary_with(records: &[&str]) -> Vec<u8> {
        let mut b = vec![0u8, 1, 2, 0x7f];
        b.extend_from_slice(MARKER.as_bytes()); // a stray marker, as in the library's own code
        b.extend_from_slice(b"\0\0junk");
        for r in records {
            b.extend_from_slice(r.as_bytes());
            b.extend_from_slice(b"\0padding\0");
        }
        b
    }

    const REC: &str = "mvl-manifest-record/v1;app=demo;vcs_commit=abc;vcs_dirty=false;end";

    #[test]
    fn finds_the_record_and_ignores_stray_markers() {
        let r = find(&binary_with(&[REC, REC])).unwrap();
        assert_eq!(r.get("app"), "demo");
        assert_eq!(r.get("vcs_commit"), "abc");
        assert_eq!(r.get("missing"), "");
    }

    #[test]
    fn no_record_is_an_error() {
        assert!(find(&binary_with(&[]))
            .unwrap_err()
            .contains("no mvl-manifest"));
    }

    #[test]
    fn conflicting_records_are_an_error() {
        let other = "mvl-manifest-record/v1;app=evil;end";
        assert!(find(&binary_with(&[REC, other]))
            .unwrap_err()
            .contains("conflicting"));
    }
}
