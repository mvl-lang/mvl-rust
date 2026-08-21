//! Step 4 of the scan → generate → run → harvest pipeline (issue #85): a
//! pure JSON join between `obligations.json` (step 1) and `cargo test`'s
//! own output (step 3) -- no mutation, no re-running anything, just
//! reading which *tagged* tests passed.
//!
//! **Tagging convention.** A generated test declares which obligation and
//! which of its `n + 1` required vectors it exercises by including
//! `mcdc__<obligation-id>__v<N>` anywhere in its (possibly
//! module-qualified) name, e.g.:
//!
//! ```text
//! #[test]
//! fn mcdc__delete_60__v1_leaf_a_true_leaf_b_false() { ... }
//! ```
//!
//! `harvest` trusts the tag -- it does not verify the test actually
//! exercises that vector (that empirical check is what
//! [`crate::discharge`]'s mutation engine is for; the two are independent
//! discharge paths over the same obligation, see the crate root doc).
//! An obligation is discharged once at least `vectors_required` distinct
//! vector numbers each have at least one passing tagged test.
//!
//! Parses stable libtest's plain-text `test <name> ... ok`/`FAILED`
//! output, not `--format json` (nightly-only), same choice
//! `cargo-mvl::test` already made.

use std::collections::{BTreeSet, HashMap};
use std::io;
use std::path::Path;
use std::process::Command;

use serde::Serialize;
use thiserror::Error;

use crate::obligation::ObligationRecord;

#[derive(Debug, Error)]
pub enum HarvestError {
    #[error("failed to spawn `cargo test`: {0}")]
    Spawn(#[source] io::Error),
}

#[derive(Debug, Clone, Serialize)]
pub struct TaggedTest {
    pub name: String,
    pub vector: u32,
    pub passed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DischargeRecord {
    pub id: String,
    pub file: String,
    pub line: usize,
    pub vectors_required: usize,
    pub compiler_void: bool,
    pub vectors_discharged: usize,
    pub discharged: bool,
    pub tagged_tests: Vec<TaggedTest>,
}

/// Extracts `(obligation_id, vector)` from a test name tagged
/// `mcdc__<id>__v<N>...`, `None` if untagged. Takes the *last* `__v`
/// occurrence so an id containing `__v` doesn't split early.
fn parse_tag(name: &str) -> Option<(String, u32)> {
    let start = name.find("mcdc__")? + "mcdc__".len();
    let rest = &name[start..];
    let marker = rest.rfind("__v")?;
    let id = &rest[..marker];
    let after = &rest[marker + "__v".len()..];
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    if id.is_empty() || digits.is_empty() {
        return None;
    }
    Some((
        id.to_string(),
        digits.parse().expect("all-digit string parses as u32"),
    ))
}

/// Runs `cargo test` in `run_dir` and groups every tagged test's outcome
/// by the obligation id in its tag.
fn run_tagged_tests(run_dir: &Path) -> Result<HashMap<String, Vec<TaggedTest>>, HarvestError> {
    let output = Command::new("cargo")
        .arg("test")
        .current_dir(run_dir)
        .output()
        .map_err(HarvestError::Spawn)?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut by_id: HashMap<String, Vec<TaggedTest>> = HashMap::new();
    for line in stdout.lines() {
        let Some(rest) = line.strip_prefix("test ") else {
            continue;
        };
        let Some((name, outcome)) = rest.rsplit_once(" ... ") else {
            continue;
        };
        let passed = match outcome.trim() {
            "ok" => true,
            "FAILED" => false,
            _ => continue,
        };
        if let Some((id, vector)) = parse_tag(name) {
            by_id.entry(id).or_default().push(TaggedTest {
                name: name.to_string(),
                vector,
                passed,
            });
        }
    }
    Ok(by_id)
}

/// Joins `obligations` against `cargo test`'s tagged output in `run_dir`,
/// producing one [`DischargeRecord`] per obligation.
pub fn harvest(
    obligations: &[ObligationRecord],
    run_dir: &Path,
) -> Result<Vec<DischargeRecord>, HarvestError> {
    let mut by_id = run_tagged_tests(run_dir)?;

    Ok(obligations
        .iter()
        .map(|o| {
            let tagged = by_id.remove(&o.id).unwrap_or_default();
            let vectors_discharged = tagged
                .iter()
                .filter(|t| t.passed)
                .map(|t| t.vector)
                .collect::<BTreeSet<_>>()
                .len();
            let discharged = o.compiler_void
                || (o.vectors_required > 0 && vectors_discharged >= o.vectors_required);
            DischargeRecord {
                id: o.id.clone(),
                file: o.file.clone(),
                line: o.line,
                vectors_required: o.vectors_required,
                compiler_void: o.compiler_void,
                vectors_discharged,
                discharged,
                tagged_tests: tagged,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tag_extracts_id_and_vector_ignoring_trailing_description() {
        assert_eq!(
            parse_tag("mcdc__delete_60__v1_leaf_a_true"),
            Some(("delete_60".to_string(), 1))
        );
    }

    #[test]
    fn parse_tag_handles_module_qualified_names() {
        assert_eq!(
            parse_tag("btree::tests::mcdc__delete_60__v2"),
            Some(("delete_60".to_string(), 2))
        );
    }

    #[test]
    fn parse_tag_rejects_untagged_names() {
        assert_eq!(parse_tag("delete_a_single_row"), None);
    }
}
