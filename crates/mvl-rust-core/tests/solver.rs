//! No concrete `SolverBackend` exists yet (ADR-0005: a native L1+L2
//! backend is tracked as its own follow-up ticket) — these tests cover
//! the data model (`Obligation`, `DischargeResult`, `Layer`) that any
//! future backend and the assurance-JSON schema (spec Requirement 13)
//! will both depend on.

use mvl_rust_core::solver::{DischargeResult, Layer, Obligation};

fn sample_obligation() -> Obligation {
    Obligation {
        id: "ob1".into(),
        predicate: "x >= 0".into(),
        provenance: "src/lib.rs:1:1".into(),
    }
}

#[test]
fn obligation_round_trips_through_json() {
    let obligation = sample_obligation();
    let json = serde_json::to_string(&obligation).unwrap();
    let decoded: Obligation = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, obligation);
}

#[test]
fn layer_serializes_to_the_assurance_json_schema_strings() {
    let cases = [
        (Layer::L1, "\"L1\""),
        (Layer::L2, "\"L2\""),
        (Layer::L3, "\"L3\""),
        (Layer::L4, "\"L4\""),
        (Layer::L5, "\"L5\""),
        (Layer::Runtime, "\"runtime\""),
    ];
    for (layer, expected) in cases {
        assert_eq!(serde_json::to_string(&layer).unwrap(), expected);
    }
}

#[test]
fn discharge_result_proven_round_trips_through_json() {
    let result = DischargeResult::Proven { layer: Layer::L2 };
    let json = serde_json::to_string(&result).unwrap();
    let decoded: DischargeResult = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, result);
}

#[test]
fn discharge_result_violated_round_trips_through_json() {
    let result = DischargeResult::Violated {
        counterexample: "x = 5".into(),
    };
    let json = serde_json::to_string(&result).unwrap();
    let decoded: DischargeResult = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, result);
}

#[test]
fn discharge_result_runtime_round_trips_through_json() {
    let result = DischargeResult::Runtime;
    let json = serde_json::to_string(&result).unwrap();
    let decoded: DischargeResult = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, result);
}
