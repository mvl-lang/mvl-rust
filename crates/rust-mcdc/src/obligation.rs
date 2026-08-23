//! The serializable obligation record -- the shared artifact both
//! discharge paths key off: [`crate::discharge`]'s mutation engine reads
//! [`crate::scanner::Decision`] directly, while [`crate::harvest`] reads
//! this record back out of an `obligations.json` file (scan and harvest
//! are separate process invocations by design -- steps 1 and 4 of the
//! scan → generate → run → harvest pipeline, issue #85).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObligationRecord {
    pub id: String,
    pub file: String,
    pub line: usize,
    pub decision: String,
    pub conditions: usize,
    pub vectors_required: usize,
    pub compiler_void: bool,
    /// `true` if this is an exhaustive `match` whose exhaustiveness relies
    /// on a `_`/catch-all arm rather than every variant being named --
    /// `syn` has no type info, so this flags any catch-all arm regardless
    /// of the scrutinee's actual type (issue #96).
    pub wildcard_risk: bool,
}

/// A stable, filesystem- and test-name-safe obligation id: the file's
/// stem plus its decision's line number, e.g. `delete_60` for
/// `btree/delete.rs:60` -- matches issue #85's worked example. Not
/// guaranteed unique across files with the same stem (e.g. two
/// `mod.rs`s); callers scanning a whole crate should qualify further if
/// that collision matters.
pub fn obligation_id(file: &str, line: usize) -> String {
    let stem = std::path::Path::new(file)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("obligation");
    let slug: String = stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("{slug}_{line}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obligation_id_uses_the_file_stem_and_line() {
        assert_eq!(obligation_id("src/btree/delete.rs", 60), "delete_60");
    }

    #[test]
    fn obligation_id_slugifies_non_alphanumeric_stem_characters() {
        assert_eq!(obligation_id("src/my-mod.name.rs", 1), "my_mod_name_1");
    }
}
