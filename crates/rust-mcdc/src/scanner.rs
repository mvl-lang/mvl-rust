//! Obligation extraction (design layer "a"): walks source with `syn`,
//! finds every *decision* (an `if`/`while` condition or a `match` guard)
//! and flattens it into its leaf conditions plus the `&&`/`||` operators
//! joining them (spec: issue #85).
//!
//! MC/DC's minimum vector count for a decision with `n` leaf conditions is
//! `n + 1`; a decision with a single leaf reduces to plain branch coverage
//! (`1 + 1 = 2`), so one formula covers both without a special case.
//!
//! `match` itself is never an obligation here: an exhaustive `match` is
//! stronger than per-arm MC/DC (every value is covered by construction),
//! so it's recorded as compiler-void — no test obligation, only bookkeeping
//! (`Obligation::compiler_void`).
//!
//! **Known scope limit:** a `let` pattern (bare `if let`/`while let`, or one
//! leaf of a stable-Rust `let`-chain joined by `&&`) is not decomposed into
//! its own sub-conditions — it's treated as an opaque single leaf, same as
//! `rust-limit`'s and `rust-total`'s own "only what a single
//! `syn::visit::Visit` pass can see" scope. It still counts as its own
//! decision/leaf toward `vectors_required`, same as any other leaf.

use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{BinOp, Expr, ExprIf, ExprMatch, ExprWhile};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ScanError {
    #[error("failed to parse source as Rust: {0}")]
    Parse(#[source] syn::Error),
}

/// One `&&`/`||` operator joining two leaves of a decision, in left-to-right
/// order, kept so [`crate::mutate`] can flip it in place.
#[derive(Debug, Clone, Copy)]
pub struct DecisionOp {
    pub span: Span,
    pub is_and: bool,
}

/// One decision site: either a boolean condition (with its flattened
/// leaves and joining operators) or a compiler-void exhaustive `match`.
#[derive(Debug, Clone)]
pub struct Decision {
    pub site: Span,
    pub text: String,
    /// Empty for a compiler-void decision (an exhaustive `match`).
    pub leaves: Vec<Span>,
    pub ops: Vec<DecisionOp>,
    pub compiler_void: bool,
    /// `true` if this compiler-void `match` is exhaustive because of a
    /// `_`/catch-all arm rather than every variant being named. Always
    /// `false` for a non-`match` decision.
    pub wildcard_risk: bool,
}

impl Decision {
    /// MC/DC's minimum required vector count: `n + 1` leaves, `0` for a
    /// compiler-void decision (nothing to discharge as a test obligation).
    pub fn vectors_required(&self) -> usize {
        if self.compiler_void {
            0
        } else {
            self.leaves.len() + 1
        }
    }

    pub fn leaf_texts<'s>(&self, source: &'s str) -> Vec<&'s str> {
        self.leaves
            .iter()
            .map(|span| slice(source, *span))
            .collect()
    }

    pub fn line(&self) -> usize {
        self.site.start().line
    }
}

/// The serializable [`crate::obligation::ObligationRecord`]s for every
/// decision scanned from one file, `file` as given by the caller (a
/// `Span` carries no filename of its own). Ids are
/// [`crate::obligation::obligation_id`]s; when two decisions in the file
/// have identical text (so identical base ids) the second and later ones
/// get an occurrence suffix in source order -- `vm_batch_f97051a9`,
/// `vm_batch_f97051a9_2`, ... -- so ids stay unique per file (#121).
pub fn to_records(file: &str, decisions: &[Decision]) -> Vec<crate::obligation::ObligationRecord> {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    decisions
        .iter()
        .map(|d| {
            let base = crate::obligation::obligation_id(file, &d.text);
            let n = seen.entry(base.clone()).or_insert(0);
            *n += 1;
            let id = if *n == 1 { base } else { format!("{base}_{n}") };
            crate::obligation::ObligationRecord {
                id,
                file: file.to_string(),
                line: d.line(),
                decision: d.text.clone(),
                conditions: d.leaves.len(),
                vectors_required: d.vectors_required(),
                compiler_void: d.compiler_void,
                wildcard_risk: d.wildcard_risk,
            }
        })
        .collect()
}

pub fn slice(source: &str, span: Span) -> &str {
    let range = span.byte_range();
    &source[range]
}

struct Collector<'s> {
    source: &'s str,
    decisions: Vec<Decision>,
}

/// Unwraps `Expr::Paren`/`Expr::Group` (grouping only, no semantic effect
/// on the flattening below).
fn unwrap_grouping(expr: &Expr) -> &Expr {
    match expr {
        Expr::Paren(p) => unwrap_grouping(&p.expr),
        Expr::Group(g) => unwrap_grouping(&g.expr),
        other => other,
    }
}

/// Recursively splits a boolean condition into its leaf sub-expressions and
/// the `&&`/`||` operators joining them, left to right.
fn flatten<'ast>(expr: &'ast Expr, leaves: &mut Vec<&'ast Expr>, ops: &mut Vec<DecisionOp>) {
    let unwrapped = unwrap_grouping(expr);
    if let Expr::Binary(bin) = unwrapped {
        if matches!(bin.op, BinOp::And(_) | BinOp::Or(_)) {
            flatten(&bin.left, leaves, ops);
            ops.push(DecisionOp {
                span: bin.op.span(),
                is_and: matches!(bin.op, BinOp::And(_)),
            });
            flatten(&bin.right, leaves, ops);
            return;
        }
    }
    leaves.push(unwrapped);
}

fn boolean_decision(source: &str, cond: &Expr) -> Option<Decision> {
    // A `let` pattern (bare `if let`, or one leaf of a `&&`-chain) isn't
    // decomposed into its own sub-conditions -- see module doc -- but it
    // still counts as a one-leaf decision, same as any other opaque leaf.
    let mut leaves = Vec::new();
    let mut ops = Vec::new();
    flatten(cond, &mut leaves, &mut ops);
    Some(Decision {
        site: cond.span(),
        text: slice(source, cond.span()).to_string(),
        leaves: leaves.iter().map(|e| e.span()).collect(),
        ops,
        compiler_void: false,
        wildcard_risk: false,
    })
}

/// A `_` arm makes a `match` exhaustive without naming every variant.
/// `syn` has no type info, so this can't distinguish an enum's remaining
/// variants from any other scrutinee type; it conservatively flags any `_`
/// arm regardless (see module doc).
///
/// Deliberately scoped to `Pat::Wild` only: an unguarded bound-name arm
/// (e.g. `other => ...`) is exhaustive the same way `_` is, but `syn`
/// parses *every* bare identifier pattern as `Pat::Ident` -- including a
/// unit enum variant matched by name (`None => ...`) -- so treating
/// `Pat::Ident` as catch-all would false-positive on ordinary, fully-named
/// exhaustive matches. Flagging only the unambiguous `_` case trades a
/// missed rare case (bound-name catch-alls) for no false positives on the
/// common one.
fn is_wildcard_arm(arm: &syn::Arm) -> bool {
    arm.guard.is_none() && matches!(arm.pat, syn::Pat::Wild(_))
}

impl<'ast> Visit<'ast> for Collector<'_> {
    fn visit_expr_if(&mut self, node: &'ast ExprIf) {
        if let Some(decision) = boolean_decision(self.source, &node.cond) {
            self.decisions.push(decision);
        }
        visit::visit_expr_if(self, node);
    }

    fn visit_expr_while(&mut self, node: &'ast ExprWhile) {
        if let Some(decision) = boolean_decision(self.source, &node.cond) {
            self.decisions.push(decision);
        }
        visit::visit_expr_while(self, node);
    }

    fn visit_expr_match(&mut self, node: &'ast ExprMatch) {
        // The `match` itself is compiler-void (exhaustive by construction);
        // any per-arm `if` guard is its own, independent boolean decision.
        self.decisions.push(Decision {
            site: node.match_token.span(),
            text: slice(self.source, node.span()).to_string(),
            leaves: Vec::new(),
            ops: Vec::new(),
            compiler_void: true,
            wildcard_risk: node.arms.iter().any(is_wildcard_arm),
        });
        for arm in &node.arms {
            if let Some((_, guard)) = &arm.guard {
                if let Some(decision) = boolean_decision(self.source, guard) {
                    self.decisions.push(decision);
                }
            }
        }
        visit::visit_expr_match(self, node);
    }
}

/// Scans already-loaded source text for every decision site.
pub fn scan_source(source: &str) -> Result<Vec<Decision>, ScanError> {
    let file: syn::File = syn::parse_str(source).map_err(ScanError::Parse)?;
    let mut collector = Collector {
        source,
        decisions: Vec::new(),
    };
    collector.visit_file(&file);
    Ok(collector.decisions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_condition_if_requires_two_vectors() {
        let source = "fn f(a: bool) { if a { } }";
        let decisions = scan_source(source).unwrap();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].leaves.len(), 1);
        assert_eq!(decisions[0].vectors_required(), 2);
        assert!(!decisions[0].compiler_void);
    }

    #[test]
    fn two_condition_or_requires_three_vectors() {
        let source = "fn f(a: bool, b: bool) { if a || b { } }";
        let decisions = scan_source(source).unwrap();
        assert_eq!(decisions[0].leaves.len(), 2);
        assert_eq!(decisions[0].vectors_required(), 3);
        assert_eq!(decisions[0].ops.len(), 1);
        assert!(!decisions[0].ops[0].is_and);
    }

    #[test]
    fn worked_example_delete_rs_decision() {
        // sqlite-rs btree/delete.rs:60 (issue #85's worked example).
        let source =
            "fn f(remaining: &[u8], ancestors: &[u8]) { if !remaining.is_empty() || ancestors.is_empty() { } }";
        let decisions = scan_source(source).unwrap();
        assert_eq!(decisions[0].leaves.len(), 2);
        assert_eq!(decisions[0].vectors_required(), 3);
    }

    #[test]
    fn while_condition_is_a_decision() {
        let source = "fn f(a: bool, b: bool) { while a && b { } }";
        let decisions = scan_source(source).unwrap();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].leaves.len(), 2);
        assert!(decisions[0].ops[0].is_and);
    }

    #[test]
    fn match_is_compiler_void_and_guard_is_its_own_decision() {
        let source = "fn f(x: i32, a: bool) { match x { n if a => n, _ => 0 } }";
        let decisions = scan_source(source).unwrap();
        assert_eq!(decisions.len(), 2);
        let match_decision = decisions.iter().find(|d| d.compiler_void).unwrap();
        assert_eq!(match_decision.vectors_required(), 0);
        assert!(match_decision.wildcard_risk);
        let guard_decision = decisions.iter().find(|d| !d.compiler_void).unwrap();
        assert_eq!(guard_decision.leaves.len(), 1);
    }

    #[test]
    fn fully_named_exhaustive_match_has_no_wildcard_risk() {
        let source = "fn f(x: Option<i32>) { match x { Some(n) => n, None => 0 } }";
        let decisions = scan_source(source).unwrap();
        assert_eq!(decisions.len(), 1);
        assert!(decisions[0].compiler_void);
        assert!(!decisions[0].wildcard_risk);
    }

    #[test]
    fn wildcard_arm_flags_totality_risk() {
        let source = "fn f(x: i32) { match x { 0 => 1, _ => 0 } }";
        let decisions = scan_source(source).unwrap();
        assert_eq!(decisions.len(), 1);
        assert!(decisions[0].compiler_void);
        assert!(decisions[0].wildcard_risk);
    }

    #[test]
    fn unguarded_catch_all_binding_is_not_detected_as_wildcard_risk() {
        // Known scope limit (see is_wildcard_arm doc): syn can't tell a
        // bound-name catch-all from a legitimately named unit variant, so
        // only the unambiguous `_` is flagged.
        let source = "fn f(x: i32) { match x { 0 => 1, other => other } }";
        let decisions = scan_source(source).unwrap();
        assert!(!decisions[0].wildcard_risk);
    }

    #[test]
    fn guarded_wildcard_arm_is_not_wildcard_risk_on_its_own() {
        let source = "fn f(x: i32, a: bool) { match x { 0 => 1, _ if a => 0, _ => 1 } }";
        let decisions = scan_source(source).unwrap();
        let match_decision = decisions.iter().find(|d| d.compiler_void).unwrap();
        // still flagged, but via the unconditional `_`, not the guarded one
        assert!(match_decision.wildcard_risk);
    }

    #[test]
    fn if_let_is_an_opaque_single_leaf_decision() {
        let source = "fn f(x: Option<i32>) { if let Some(n) = x { let _ = n; } }";
        let decisions = scan_source(source).unwrap();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].leaves.len(), 1);
        assert_eq!(decisions[0].vectors_required(), 2);
        assert!(!decisions[0].compiler_void);
    }

    #[test]
    fn let_chain_leaf_is_opaque_but_counted() {
        let source = "fn f(a: bool, x: Option<i32>) { if a && let Some(n) = x { let _ = n; } }";
        let decisions = scan_source(source).unwrap();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].leaves.len(), 2);
        assert_eq!(decisions[0].vectors_required(), 3);
        assert!(decisions[0].ops[0].is_and);
    }

    #[test]
    fn three_leaf_chain_flattens_left_to_right() {
        let source = "fn f(a: bool, b: bool, c: bool) { if a && b || c { } }";
        let decisions = scan_source(source).unwrap();
        assert_eq!(decisions[0].leaves.len(), 3);
        assert_eq!(decisions[0].vectors_required(), 4);
        assert_eq!(decisions[0].leaf_texts(source), vec!["a", "b", "c"]);
        assert!(decisions[0].ops[0].is_and);
        assert!(!decisions[0].ops[1].is_and);
    }

    #[test]
    fn to_records_suffixes_repeated_decisions_in_source_order() {
        let source = "fn f(a: bool) { if a { } if a { } if !a { } }";
        let decisions = scan_source(source).unwrap();
        let records = to_records("src/x.rs", &decisions);
        let ids: Vec<&str> = records.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            ids[0],
            &format!("x_{}", crate::obligation::decision_hash("a"))
        );
        assert_eq!(ids[1], format!("{}_2", ids[0]));
        assert_ne!(ids[2], ids[0]);
        assert!(!ids[2].ends_with("_2"));
    }

    #[test]
    fn to_records_ids_survive_line_shifts_and_reformatting() {
        let before = "fn f(a: bool, b: bool) { if a && b { } }";
        let after = "// new comment\n\nuse std::fmt;\n\nfn f(a: bool, b: bool) {\n    if a\n        && b\n    {\n    }\n}";
        let id_before = to_records("src/x.rs", &scan_source(before).unwrap())[0]
            .id
            .clone();
        let shifted = to_records("src/x.rs", &scan_source(after).unwrap());
        assert_eq!(shifted[0].id, id_before);
        assert_ne!(shifted[0].line, 1, "the decision really did move");
        // Editing the decision itself does retag.
        let edited = to_records(
            "src/x.rs",
            &scan_source("fn f(a: bool, b: bool) { if a || b { } }").unwrap(),
        );
        assert_ne!(edited[0].id, id_before);
    }
}
