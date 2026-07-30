//! Obligation-discharge types and the solver-integration trait.
//!
//! `rust-refine` (Phase 3) needs an obligation dispatcher, layered like the
//! MVL compiler's own: `L1` trivial syntactic checks, `L2` interval
//! arithmetic, `L3` bounded-quantifier expansion, `L4` Fourier–Motzkin
//! elimination, `L5` full SMT, with an uncloseable obligation falling
//! through to a runtime check.
//!
//! Two of those names differ from the reference's, deliberately (#55):
//!
//! - **`L3` is bounded-quantifier expansion, not path enumeration.** The
//!   reference's L3 enumerates execution paths through pure function bodies;
//!   this one substitutes each value of a literal range and re-dispatches.
//!   Different mechanisms sharing a label. Path enumeration is deferred on the
//!   reference's own hit rate — L3 discharges 1 of 174 obligations there
//!   (ADR-0006 §3).
//! - **`L4` is Fourier–Motzkin, not Cooper's quantifier elimination.** Cooper
//!   is implemented nowhere, in either codebase: [`native`]'s `Constraint` has
//!   no divisibility atom, so Cooper's central atom is unrepresentable. The
//!   `L4` label and the `L4:cooper` stats key are retained for wire
//!   compatibility with upstream tooling; only the prose is corrected
//!   (ADR-0006 §1, upstream `mvl-lang/mvl`#2022).
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
mod smt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The layer that discharged an obligation: trivial syntactic check,
/// interval arithmetic, bounded-quantifier expansion, Fourier–Motzkin
/// elimination, full SMT, or a runtime outcome when no static layer could
/// close it. The variant names are wire-compatible with upstream's stats keys
/// and so keep the `L4`/`cooper` spelling; see the module doc for why the
/// technique names differ (#55). Serializes to the string values used by the
/// assurance-JSON schema (spec Requirement 13) -- `JsonSchema` derived
/// here since [`crate::assurance::schema::ObligationRecord`]
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

/// Which program point an obligation came from, and so **which question was
/// asked of the solver** (#56).
///
/// This is the discriminator the assurance report was missing. A declaration
/// site asks *is this predicate satisfiable* — coherence — while a call or
/// return site asks *does Γ entail it* — a real entailment proof. Both
/// previously landed in the report with the same shape and the same `layer`,
/// so a consumer reading `prove.obligations[]` as evidence could not tell
/// `"x > 0 is satisfiable"` from `"Γ entails h's precondition here"`. On the
/// shipped compliant demo the first kind was 7 of 16 records.
///
/// Per ADR-0005 §2 the two are deliberately different checks rather than one
/// being a weaker approximation of the other — a self-contradictory
/// `requires` is a real defect worth reporting. The defect was only ever in
/// presenting them identically.
///
/// Deliberately coarser than `rust_refine::checks::ObligationKind`, which
/// also carries the callee name: this is the wire-facing classification, and
/// the callee is already in the obligation's id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ObligationClass {
    /// A `#[mvl::requires]`/`#[mvl::ensures]` on a function, checked for
    /// internal coherence. Nothing is known about arguments here, so the
    /// question is satisfiability, **not** whether the predicate holds.
    #[serde(rename = "declaration")]
    Declaration,
    /// A call whose callee's `requires` must be entailed by the caller's Γ.
    #[serde(rename = "call-site")]
    CallSite,
    /// A return point whose returned expression must establish the
    /// function's `ensures` (#42).
    #[serde(rename = "return-site")]
    ReturnSite,
}

impl ObligationClass {
    /// Whether discharging this obligation constitutes an **entailment
    /// proof** — the claim a certification audience is reading the report
    /// for — as opposed to a coherence check.
    ///
    /// The distinction a bare count of `prove.obligations[]` erases, so
    /// anything summarising the report should split on this rather than
    /// totalling the list.
    pub fn is_entailment(&self) -> bool {
        matches!(
            self,
            ObligationClass::CallSite | ObligationClass::ReturnSite
        )
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ObligationClass::Declaration => "declaration",
            ObligationClass::CallSite => "call-site",
            ObligationClass::ReturnSite => "return-site",
        }
    }
}

/// A single refinement obligation to discharge.
///
/// `kind` and `provenance` are carried for the report rather than for the
/// solver, which ignores both — this type is the obligation's identity and
/// origin record, not just solver input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Obligation {
    pub id: String,
    pub predicate: String,
    pub provenance: String,
    pub kind: ObligationClass,
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

/// What actually backs a reported outcome — the third axis alongside
/// [`ObligationClass`] ("which question was asked") and [`Layer`]/
/// [`DischargeResult`] ("did static reasoning close it") (#69, spec 007
/// Requirement 6).
///
/// Upstream `mvl-lang/mvl` never needed this distinction: it enforces
/// *every* runtime-checkable `requires`/`ensures` unconditionally, with no
/// opt-out, so every postcondition is always backed by a check nobody can
/// disable and "proven or enforced" is a distinction without a difference
/// for propagation purposes. This port introduced `#[mvl::unchecked]`
/// (#53) specifically to resolve the `#[mvl::total]`/panic-freedom
/// collision — a problem upstream doesn't have, since its `total` is
/// termination-only (ADR-0003 §Consequences amendment). Once a function can
/// nominally carry `#[mvl::ensures]` while opting out of the assert,
/// "was this actually enforced" stops being universally true, and Γ
/// propagation (ADR-0006 §5 condition 5) needs a way to say so.
///
/// | `Warrant` | What it claims |
/// |---|---|
/// | `Proof` | A real static entailment/satisfiability proof, untainted by any enforced-not-proven premise |
/// | `Enforcement` | Rests on at least one runtime-enforced (not statically proven) premise, named exactly — not a proof, but not silently unverified either |
/// | `None` | Neither proven nor backed by enforcement — genuinely unverified |
///
/// Computed only for entailment obligations (`ObligationClass::CallSite`/
/// `ReturnSite`); a `Declaration`-kind coherence check has no Γ and no
/// enforcement concept to rest on, so it is always `Proof` or `None`,
/// never `Enforcement`. `DischargeResult::Violated` is always `None`
/// regardless of enforcement — a demonstrated counterexample is a real
/// defect to fix, and the safety net an assert provides doesn't excuse it.
///
/// `premises` names the exact functions this outcome depends on, computed
/// by `rust-refine` via leave-one-out re-discharge (`checks.rs`): each
/// candidate enforced-not-proven Γ hypothesis is removed and the goal
/// re-discharged; if the outcome still holds without it, it wasn't
/// load-bearing. This is exact, not a conservative over-approximation —
/// a hypothesis that happened to be in scope but wasn't actually needed for
/// this particular proof is not listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "warrant", rename_all = "lowercase")]
pub enum Warrant {
    Proof,
    Enforcement { premises: Vec<String> },
    None,
}
