//! Mutant generation (design layer "c"): for each decision the
//! [`crate::scanner`] found, emits the exact set of source-text edits that
//! `discharged ⇔ compiler-void ∨ all-condition-mutants-killed` (issue #85's
//! amended policy) requires — one `→true`, one `→false` per leaf condition,
//! and one operator flip (`&&`↔`||`) per join. The obligation inventory and
//! the mutant set are the same artifact by construction: every mutant here
//! traces back to exactly one [`crate::scanner::Decision`].

use std::ops::Range;

use crate::scanner::{Decision, DecisionOp};

/// One source-text edit: replace the bytes at `range` (byte offsets into
/// the original source) with `replacement`.
#[derive(Debug, Clone)]
pub struct Mutant {
    pub description: String,
    pub range: Range<usize>,
    pub replacement: String,
}

/// Every mutant for one decision. Empty for a compiler-void decision --
/// there is nothing to discharge by mutation.
pub fn mutants_for(decision: &Decision) -> Vec<Mutant> {
    if decision.compiler_void {
        return Vec::new();
    }

    let mut mutants = Vec::new();

    for (index, leaf) in decision.leaves.iter().enumerate() {
        let range = leaf.byte_range();
        mutants.push(Mutant {
            description: format!("condition {index} forced to `true`"),
            range: range.clone(),
            replacement: "true".to_string(),
        });
        mutants.push(Mutant {
            description: format!("condition {index} forced to `false`"),
            range,
            replacement: "false".to_string(),
        });
    }

    for (index, op) in decision.ops.iter().enumerate() {
        mutants.push(operator_flip(index, op));
    }

    mutants
}

fn operator_flip(index: usize, op: &DecisionOp) -> Mutant {
    let (from, to) = if op.is_and { ("&&", "||") } else { ("||", "&&") };
    Mutant {
        description: format!("operator {index} flipped from `{from}` to `{to}`"),
        range: op.span.byte_range(),
        replacement: to.to_string(),
    }
}

/// Applies `mutant` to `source`, returning the mutated text. `source` must
/// be the exact text `mutant`'s byte range was computed against.
pub fn apply(source: &str, mutant: &Mutant) -> String {
    let mut mutated = String::with_capacity(source.len());
    mutated.push_str(&source[..mutant.range.start]);
    mutated.push_str(&mutant.replacement);
    mutated.push_str(&source[mutant.range.end..]);
    mutated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::scan_source;

    #[test]
    fn compiler_void_decision_has_no_mutants() {
        let source = "fn f(x: Option<i32>) -> i32 { match x { Some(n) => n, None => 0 } }";
        let decisions = scan_source(source).unwrap();
        let match_decision = decisions.iter().find(|d| d.compiler_void).unwrap();
        assert!(mutants_for(match_decision).is_empty());
    }

    #[test]
    fn two_leaf_or_decision_yields_five_mutants() {
        let source = "fn f(a: bool, b: bool) { if a || b { } }";
        let decisions = scan_source(source).unwrap();
        let decision = &decisions[0];
        let mutants = mutants_for(decision);
        // 2 leaves * 2 (true/false) + 1 operator flip = 5
        assert_eq!(mutants.len(), 5);
    }

    #[test]
    fn apply_replaces_only_the_targeted_range() {
        let source = "fn f(a: bool, b: bool) { if a || b { } }";
        let decisions = scan_source(source).unwrap();
        let mutant = &mutants_for(&decisions[0])[0];
        let mutated = apply(source, mutant);
        assert_ne!(mutated, source);
        assert_eq!(mutated.len(), source.len() - "a".len() + "true".len());
    }
}
