//! Coherence checks and entailment proofs must be distinguishable in the
//! report (#56).
//!
//! A declaration-site obligation asks *is this predicate satisfiable*; a
//! call- or return-site obligation asks *does Γ entail it*. Before #56 both
//! landed in `prove.obligations[]` with the same shape and the same `layer`,
//! so a consumer reading the list as evidence could not tell
//! `"x > 0 is satisfiable"` -- close to vacuous -- from `"Γ entails
//! need_pos's precondition here"`, which is the actual claim.
//!
//! ADR-0005 §2 keeps the two as deliberately different checks; the defect was
//! only ever in presenting them identically.

use mvl_rust_core::solver::{Layer, ObligationClass};
use rust_refine::checks::find_obligations;

/// Every obligation in `source` as `(id, class)`.
fn classes(source: &str) -> Vec<(String, ObligationClass)> {
    find_obligations(source)
        .expect("fixture parses")
        .iter()
        .map(|found| (found.id(), found.class()))
        .collect()
}

const MIXED: &str = "\
#[mvl::requires(x > 0)]
#[mvl::ensures(result > 0)]
fn caller(x: i32) -> i32 {
    need_pos(x);
    x + 1
}

#[mvl::requires(n > 0)]
fn need_pos(n: i32) -> i32 { n }
";

#[test]
fn each_program_point_reports_its_own_class() {
    assert_eq!(
        classes(MIXED),
        vec![
            (
                "caller::requires#0".to_string(),
                ObligationClass::Declaration
            ),
            (
                "caller::ensures#0".to_string(),
                ObligationClass::Declaration
            ),
            (
                "need_pos::requires#0".to_string(),
                ObligationClass::Declaration
            ),
            (
                "caller::calls::need_pos::requires#0".to_string(),
                ObligationClass::CallSite
            ),
            (
                "caller::returns::ensures#0".to_string(),
                ObligationClass::ReturnSite
            ),
        ]
    );
}

#[test]
fn only_call_and_return_sites_count_as_entailment() {
    let (entailment, coherence): (Vec<_>, Vec<_>) = classes(MIXED)
        .into_iter()
        .partition(|(_, class)| class.is_entailment());

    // 3 of the 5 obligations on this small fixture are coherence checks. A
    // bare `obligations.len()` would report 5 proofs where there are 2.
    assert_eq!(coherence.len(), 3);
    assert_eq!(entailment.len(), 2);
}

#[test]
fn requires_and_ensures_on_a_declaration_share_one_class() {
    // They differ in *which* predicate is checked, not in the question asked
    // of it, and which one it was is already in the id -- so the wire class
    // deliberately does not split them.
    let classes =
        classes("#[mvl::requires(x > 0)]\n#[mvl::ensures(result > 0)]\nfn f(x: i32) -> i32 { 1 }");
    assert!(classes
        .iter()
        .take(2)
        .all(|(_, c)| *c == ObligationClass::Declaration));
}

#[test]
fn a_residual_is_not_a_proof_even_at_a_call_site() {
    use mvl_rust_core::assurance::schema::ObligationRecord;
    use mvl_rust_core::solver::{DischargeResult, Obligation, Warrant};

    // The half of #56 that a `kind` field alone does not fix: an obligation
    // can ask the right question and still not answer it. ADR-0006 §5 --
    // injection "buys soundness, not the right to keep calling it a proof".
    let obligation = Obligation {
        id: "f::calls::g::requires#0".into(),
        predicate: "x > 0".into(),
        provenance: "src/lib.rs:1:1".into(),
        kind: ObligationClass::CallSite,
    };
    let residual = ObligationRecord::new(&obligation, &DischargeResult::Runtime, &Warrant::None);
    assert_eq!(residual.layer, Some(Layer::Runtime));
    assert!(
        !residual.is_proof(),
        "a call-site obligation left to a runtime check is not a proof"
    );

    let proven = ObligationRecord::new(
        &obligation,
        &DischargeResult::Proven { layer: Layer::L2 },
        &Warrant::Proof,
    );
    assert!(proven.is_proof());
}

#[test]
fn a_discharged_coherence_check_is_not_a_proof() {
    use mvl_rust_core::assurance::schema::ObligationRecord;
    use mvl_rust_core::solver::{DischargeResult, Obligation, Warrant};

    // The other half: statically discharged, at a real layer, and still not
    // evidence that anything holds -- only that it *could*.
    let obligation = Obligation {
        id: "f::requires#0".into(),
        predicate: "x > 0".into(),
        provenance: "src/lib.rs:1:1".into(),
        kind: ObligationClass::Declaration,
    };
    let record = ObligationRecord::new(
        &obligation,
        &DischargeResult::Proven { layer: Layer::L2 },
        &Warrant::Proof,
    );
    assert_eq!(record.layer, Some(Layer::L2));
    assert!(!record.is_proof());
}

#[test]
fn the_class_is_on_the_wire_under_a_stable_name() {
    use mvl_rust_core::assurance::schema::ObligationRecord;
    use mvl_rust_core::solver::{DischargeResult, Obligation, Warrant};

    let obligation = Obligation {
        id: "f::returns::ensures#0".into(),
        predicate: "1 > 0".into(),
        provenance: "src/lib.rs:1:1".into(),
        kind: ObligationClass::ReturnSite,
    };
    let record = ObligationRecord::new(
        &obligation,
        &DischargeResult::Proven { layer: Layer::L1 },
        &Warrant::Proof,
    );
    let json = serde_json::to_value(&record).unwrap();
    assert_eq!(json["kind"], "return-site");
    assert_eq!(json["layer"], "L1");
}
