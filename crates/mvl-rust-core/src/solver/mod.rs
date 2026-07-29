//! Obligation-discharge types and the solver-integration trait.
//!
//! `rust-refine` (Phase 3) needs an obligation dispatcher, layered like the
//! MVL compiler's own: `L1` trivial syntactic checks, `L2` interval
//! arithmetic, `L3` bounded path enumeration, `L4` Cooper's quantifier
//! elimination, `L5` full SMT, with an uncloseable obligation falling
//! through to a runtime check.
//!
//! Per ADR-0005 (`.openspec/adr/0005-refinement-obligations.md`), this
//! dispatcher is implemented **natively** in `mvl-rust-core` — not by
//! shelling out to or linking `mvl-lang/mvl`'s own solver. Doing either
//! would mean `rust-refine` isn't independent verification at all (the
//! same solver with a Rust UI on top can never disagree with itself), and
//! would pressure `mvl-lang/mvl`'s codebase to grow Rust-specific
//! accommodations it doesn't need. There is deliberately no shell-out or
//! linked backend here, not even as a documented fallback option.
//!
//! [`SolverBackend`] is the abstraction point tool crates depend on.
//! [`native::NativeBackend`] is the concrete `L1`+`L2` implementation
//! (v0.1 scope per ADR-0005 — `L3`–`L5` deferred, falling through to
//! `DischargeResult::Runtime`) that `rust-refine` (#8) uses.

pub mod native;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The layer that discharged an obligation: trivial syntactic check,
/// interval arithmetic, bounded path enumeration, Cooper's quantifier
/// elimination, full SMT, or a runtime assertion when no static layer
/// could close it. Serializes to the string values used by the
/// assurance-JSON schema (spec Requirement 13) -- `JsonSchema` derived
/// here since [`crate::assurance::schema::ProvenObligationRecord`]
/// embeds this type directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Layer {
    #[serde(rename = "L1")]
    L1,
    #[serde(rename = "L2")]
    L2,
    #[serde(rename = "L3")]
    L3,
    #[serde(rename = "L4")]
    L4,
    #[serde(rename = "L5")]
    L5,
    #[serde(rename = "runtime")]
    Runtime,
}

/// A single refinement obligation to discharge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Obligation {
    pub id: String,
    pub predicate: String,
    pub provenance: String,
}

/// Outcome of attempting to discharge an [`Obligation`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "lowercase")]
pub enum DischargeResult {
    /// Proven, attributed to the layer that closed it.
    Proven { layer: Layer },
    /// Could not be proven statically; a runtime assertion should be
    /// inserted at the obligation's site.
    Runtime,
    /// Disproven, with a counterexample from the solver.
    Violated { counterexample: String },
}

/// Abstract interface for the obligation dispatcher. Implemented natively
/// in `mvl-rust-core` (ADR-0005) — there is no shell-out or linked
/// backend. Native reasoning always produces *some* outcome (`Proven`,
/// `Runtime`, or `Violated`), never an I/O-style failure, so this doesn't
/// return a `Result`.
pub trait SolverBackend {
    fn discharge(&self, obligation: &Obligation) -> DischargeResult;
}
