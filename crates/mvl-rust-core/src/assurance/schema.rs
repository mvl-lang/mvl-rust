//! The assurance-JSON schema types (spec Requirement 13).
//!
//! Each top-level section is optional — a given invocation only fills in
//! the sections for the subcommands it actually ran (e.g. `cargo mvl check`
//! only ever fills `check`; `mcdc`/`coverage` stay `None` until #15 wires
//! `cargo llvm-cov` in, and `test` until #14's per-tool emission lands).
//! The `assurance` section (claim/argument_tree/leaves) is explicitly
//! **provisional** — see [`AssuranceLeaf`]'s doc comment.

use crate::solver::{Layer, ObligationClass};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::version::ASSURANCE_SCHEMA_VERSION;

/// Top-level assurance report: one JSON document aggregating every
/// subcommand that ran against `target`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AssuranceReport {
    pub version: String,
    pub target: Target,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check: Option<CheckSection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prove: Option<ProveSection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test: Option<TestSection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcdc: Option<McdcSection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage: Option<CoverageSection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assurance: Option<AssuranceSection>,
}

impl AssuranceReport {
    /// Starts an empty report for `crate_name` at `timestamp` (every
    /// section absent) — callers fill in whichever sections their
    /// subcommand actually ran.
    pub fn new(crate_name: impl Into<String>, timestamp: impl Into<String>) -> Self {
        AssuranceReport {
            version: ASSURANCE_SCHEMA_VERSION.to_string(),
            target: Target {
                crate_name: crate_name.into(),
                commit: None,
                timestamp: timestamp.into(),
            },
            check: None,
            prove: None,
            test: None,
            mcdc: None,
            coverage: None,
            assurance: None,
        }
    }
}

/// Identifies what was checked: which crate, at which commit (if known),
/// and when.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Target {
    #[serde(rename = "crate")]
    pub crate_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    pub timestamp: String,
}

/// A single diagnostic in wire form — a plain-data projection of
/// [`crate::diagnostics::Diagnostic`], which carries a `proc_macro2::Span`
/// that isn't itself serializable. `provenance` is a rendered
/// `file:line:col` string, mirroring [`crate::solver::Obligation`]'s
/// `provenance` field convention.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DiagnosticRecord {
    pub level: String,
    pub message: String,
    pub provenance: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

/// `cargo mvl check`'s output: Gate-mode diagnostics (`rust-limit`,
/// `rust-total`, ...) plus any obligations checked along the way. No tool
/// currently populates `obligations` here (that's `prove`'s job) — kept
/// per spec Requirement 13's schema shape for future Gate-mode tools that
/// might model checks as obligations too.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct CheckSection {
    #[serde(default)]
    pub obligations: Vec<ObligationRecord>,
    #[serde(default)]
    pub diagnostics: Vec<DiagnosticRecord>,
}

/// One refinement obligation's outcome, flattened into a single wire
/// record — combines [`crate::solver::Obligation`]'s fields with its
/// [`crate::solver::DischargeResult`] rather than nesting them, matching
/// spec Requirement 13's `{ id, predicate, kind, layer, provenance,
/// counterexample? }` shape.
///
/// **Was `ProvenObligationRecord` until #56**, which was wrong twice over:
/// the list holds coherence checks and undischarged residuals alongside real
/// proofs, so "proven" described only a subset of what lands here. Reading
/// `kind` and `layer` together is what separates the three:
///
/// | `kind` | `layer` | what the record actually claims |
/// |---|---|---|
/// | `declaration` | `L1`–`L5` | the predicate is *satisfiable* — coherence, close to vacuous as evidence |
/// | `call-site`/`return-site` | `L1`–`L5` | Γ entails it — a real entailment proof |
/// | any | `runtime` | **not discharged**; a runtime check is owed (ADR-0006 §5) |
///
/// So a bare `obligations.len()` is not an evidence count. Anything
/// summarising this list should split on
/// [`ObligationClass::is_entailment`] and exclude `Layer::Runtime` rather
/// than totalling it.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ObligationRecord {
    pub id: String,
    pub predicate: String,
    pub provenance: String,
    /// Which question was asked — see [`ObligationClass`]. The field whose
    /// absence let coherence pass for proof.
    pub kind: ObligationClass,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer: Option<Layer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counterexample: Option<String>,
}

impl ObligationRecord {
    /// Builds a wire record from an obligation and its discharge outcome.
    pub fn new(
        obligation: &crate::solver::Obligation,
        result: &crate::solver::DischargeResult,
    ) -> Self {
        use crate::solver::DischargeResult;
        let (layer, counterexample) = match result {
            DischargeResult::Proven { layer } => (Some(*layer), None),
            DischargeResult::Runtime => (Some(Layer::Runtime), None),
            DischargeResult::Violated { counterexample } => (None, Some(counterexample.clone())),
        };
        ObligationRecord {
            id: obligation.id.clone(),
            predicate: obligation.predicate.clone(),
            provenance: obligation.provenance.clone(),
            kind: obligation.kind,
            layer,
            counterexample,
        }
    }

    /// Whether this record is a real entailment proof: the right question
    /// asked *and* actually discharged statically.
    ///
    /// Both halves matter. A `call-site` record with `layer: runtime` asked
    /// the right question and did not answer it, and ADR-0006 §5 is explicit
    /// that injecting the check "buys soundness, not the right to keep
    /// calling it a proof".
    pub fn is_proof(&self) -> bool {
        self.kind.is_entailment() && matches!(self.layer, Some(layer) if layer != Layer::Runtime)
    }
}

/// `cargo mvl prove`'s output: `rust-refine`'s obligation trace.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ProveSection {
    #[serde(default)]
    pub obligations: Vec<ObligationRecord>,
}

/// One test's outcome (`cargo mvl test`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TestRecord {
    pub name: String,
    pub outcome: String,
    pub duration_ms: u64,
}

/// Aggregate pass/fail/ignored counts for a test run.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct TestSummary {
    pub passed: u64,
    pub failed: u64,
    pub ignored: u64,
}

/// `cargo mvl test`'s output. Nothing produces this yet — #6's `test`
/// subcommand is still a "not yet implemented" stub pending this schema
/// and #14's per-tool emission wiring.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct TestSection {
    #[serde(default)]
    pub tests: Vec<TestRecord>,
    pub summary: TestSummary,
}

/// One MC/DC condition's coverage status (`cargo mvl mcdc`, #15, via
/// `cargo llvm-cov --mcdc`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct McdcCondition {
    pub id: String,
    pub covered: bool,
}

/// `cargo mvl mcdc`'s output. Nothing produces this yet — tracked by #15.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct McdcSection {
    #[serde(default)]
    pub conditions: Vec<McdcCondition>,
    pub coverage_pct: f64,
}

/// Covered/total counts for one coverage dimension (lines or branches).
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct CoverageStat {
    pub covered: u64,
    pub total: u64,
}

/// `cargo mvl coverage`'s output (`cargo llvm-cov`, #15). Nothing produces
/// this yet.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct CoverageSection {
    pub lines: CoverageStat,
    pub branches: CoverageStat,
}

/// One leaf of the assurance argument tree: a discharged obligation plus
/// the reasoning (`warrant`) connecting it to the top-level `claim`.
///
/// **Provisional.** #22 flagged that `warrant`'s semantics (a free-text
/// justification? a typed enum keyed to the obligation's discharge layer?
/// something else?) and `argument_tree`'s structure (a strict tree vs a
/// DAG; how `cargo mvl assurance` actually builds it from the independent
/// `prove`/`test`/`mcdc`/`coverage` sections) aren't resolved yet. This
/// type is a structural placeholder, not a settled design — expect it to
/// change once #22 lands.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AssuranceLeaf {
    pub warrant: String,
    pub obligation_id: String,
    pub provenance: String,
}

/// `cargo mvl assurance`'s aggregated output — see [`AssuranceLeaf`]'s doc
/// comment for what's still unresolved here.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AssuranceSection {
    pub claim: String,
    /// Untyped for now — see #22; the tree/DAG structure isn't decided.
    pub argument_tree: serde_json::Value,
    #[serde(default)]
    pub leaves: Vec<AssuranceLeaf>,
}
