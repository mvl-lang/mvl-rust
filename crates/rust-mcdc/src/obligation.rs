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
/// module path, the enclosing `fn` name, and a short content hash of the
/// decision's source text, e.g. `btree_delete_remove_f97051a9` for the
/// `if a && b` inside `fn remove` in `src/btree/delete.rs` (issue #121).
/// A decision outside any `fn` (a `const` initializer, say) has no fn
/// segment: `btree_delete_f97051a9`.
///
/// Neither half depends on the line number, so inserting or deleting
/// code above a decision does not retag it; the id changes exactly when
/// the decision's own text changes, which is when its vectors need
/// revisiting anyway. Renaming or moving the enclosing `fn` does retag.
/// Two identical decisions in one `fn` get the same base id -- [`crate::scanner::to_records`] disambiguates those with an
/// occurrence suffix (`_2`, `_3`, ...) in source order.
///
/// The module path is every path component after the *last* `src`
/// component (or the whole path if there is none), with `mod.rs`
/// collapsing to its directory (`src/vfs/mod.rs` → `vfs`) and
/// `lib.rs`/`main.rs` kept as-is; each component is slugified to
/// `[A-Za-z0-9_]` and the components are joined with `_`. Production
/// module paths are unique within a crate, so two files under one `src`
/// tree can never share an id -- the earlier stem-only scheme collided on
/// any `vm/batch.rs` + `codegen/batch.rs` style layout.
pub fn obligation_id(file: &str, enclosing_fn: Option<&str>, decision: &str) -> String {
    match enclosing_fn {
        Some(name) => format!(
            "{}_{}_{}",
            module_slug(file),
            slugify(name),
            decision_hash(decision)
        ),
        None => format!("{}_{}", module_slug(file), decision_hash(decision)),
    }
}

fn slugify(part: &str) -> String {
    part.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// The module-path half of an [`obligation_id`]; see there for the rule.
pub fn module_slug(file: &str) -> String {
    let path = std::path::Path::new(file);
    let components: Vec<&str> = path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();
    let after_src = components
        .iter()
        .rposition(|c| *c == "src")
        .map_or(0, |i| i + 1);
    let mut parts: Vec<&str> = components[after_src..].to_vec();
    if let Some(last) = parts.pop() {
        let stem = std::path::Path::new(last)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(last);
        // `mod.rs` is its directory's module: `vfs/mod.rs` is `vfs`.
        // A bare `mod.rs` with no directory keeps its stem.
        if stem != "mod" || parts.is_empty() {
            parts.push(stem);
        }
    }
    let slug: String = parts
        .iter()
        .map(|part| slugify(part))
        .collect::<Vec<_>>()
        .join("_");
    if slug.is_empty() {
        "obligation".to_string()
    } else {
        slug
    }
}

/// The content half of an [`obligation_id`]: 8 hex chars of FNV-1a (64
/// bit) over the decision text with whitespace runs collapsed to one
/// space, so reformatting alone does not retag. FNV-1a is implemented
/// inline rather than via `std::hash` because `DefaultHasher`'s output is
/// not guaranteed stable across Rust releases and these ids live in
/// consumers' test names.
pub fn decision_hash(decision: &str) -> String {
    let normalized = decision.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in normalized.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:08x}", (hash >> 32) as u32 ^ hash as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_slug_uses_the_module_path() {
        assert_eq!(module_slug("src/btree/delete.rs"), "btree_delete");
        assert_eq!(module_slug("src/vm/batch.rs"), "vm_batch");
        assert_eq!(module_slug("src/codegen/batch.rs"), "codegen_batch");
        assert_eq!(
            module_slug("src/codegen/row/select/aggregate.rs"),
            "codegen_row_select_aggregate"
        );
    }

    #[test]
    fn module_slug_keeps_top_level_files_unqualified() {
        assert_eq!(module_slug("src/types.rs"), "types");
        assert_eq!(module_slug("src/lib.rs"), "lib");
        assert_eq!(module_slug("src/main.rs"), "main");
    }

    #[test]
    fn module_slug_collapses_mod_rs_to_its_directory() {
        assert_eq!(module_slug("src/storage/row/vfs/mod.rs"), "storage_row_vfs");
        assert_eq!(module_slug("src/mod.rs"), "mod");
    }

    #[test]
    fn module_slug_uses_the_last_src_component() {
        assert_eq!(module_slug("crates/db-core/src/vm/batch.rs"), "vm_batch");
        assert_eq!(module_slug("/abs/src/a/src/b/c.rs"), "b_c");
    }

    #[test]
    fn module_slug_uses_the_whole_path_without_a_src_component() {
        assert_eq!(module_slug("tests/it.rs"), "tests_it");
        assert_eq!(module_slug("examples/demo/run.rs"), "examples_demo_run");
        assert_eq!(module_slug("./benches/bench.rs"), "benches_bench");
    }

    #[test]
    fn module_slug_slugifies_non_alphanumeric_components() {
        assert_eq!(module_slug("src/my-mod.name.rs"), "my_mod_name");
        assert_eq!(module_slug("src/my-dir/x.rs"), "my_dir_x");
    }

    #[test]
    fn decision_hash_is_eight_hex_chars_and_deterministic() {
        let h = decision_hash("a && b");
        assert_eq!(h.len(), 8);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(h, decision_hash("a && b"));
        assert_ne!(h, decision_hash("a || b"));
    }

    #[test]
    fn decision_hash_ignores_whitespace_differences() {
        assert_eq!(decision_hash("a &&\n    b"), decision_hash("a && b"));
        assert_eq!(decision_hash("  a && b "), decision_hash("a && b"));
    }

    #[test]
    fn obligation_id_is_module_slug_fn_and_decision_hash() {
        assert_eq!(
            obligation_id("src/btree/delete.rs", Some("remove"), "a && b"),
            format!("btree_delete_remove_{}", decision_hash("a && b"))
        );
        assert_eq!(
            obligation_id("src/btree/delete.rs", None, "a && b"),
            format!("btree_delete_{}", decision_hash("a && b"))
        );
        // Raw identifiers slugify like everything else.
        assert_eq!(
            obligation_id("src/x.rs", Some("r#match"), "a"),
            format!("x_r_match_{}", decision_hash("a"))
        );
        // Same decision text in same-stem files: distinct ids.
        assert_ne!(
            obligation_id("src/vm/batch.rs", Some("f"), "x > 0"),
            obligation_id("src/codegen/batch.rs", Some("f"), "x > 0")
        );
        // Same decision text in different fns of one file: distinct ids.
        assert_ne!(
            obligation_id("src/vm/batch.rs", Some("f"), "x > 0"),
            obligation_id("src/vm/batch.rs", Some("g"), "x > 0")
        );
    }
}
