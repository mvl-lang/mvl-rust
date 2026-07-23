//! Solver-integration trait for discharging refinement obligations.
//!
//! `rust-refine` (Phase 3) needs the same L1-L5 obligation dispatcher the
//! MVL compiler uses. [`SolverBackend`] is the abstraction point: the
//! default [`ShellOutSolver`] shells out to `mvl solve --json` per the
//! spec's own recommendation (start with shell-out, migrate to a linked
//! solver crate once the compiler exposes one — see ADR-0001). The exact
//! wire format below is a first cut; ADR-0001 owns the final contract.
//! Real solver integration for `rust-refine` itself is out of scope here —
//! this ticket only owns the trait and its default implementation.

use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::process::{Command, Stdio};
use thiserror::Error;

/// The layer that discharged an obligation, mirroring the MVL compiler's
/// dispatch order: trivial syntactic check, interval arithmetic, bounded
/// path enumeration, Cooper's quantifier elimination, full SMT, or a
/// runtime assertion when no static layer could close it. Serializes to the
/// string values used by the assurance-JSON schema (spec Requirement 13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Error)]
pub enum SolverError {
    #[error("failed to invoke solver process: {0}")]
    Spawn(#[source] io::Error),
    #[error("failed to write obligation to solver stdin: {0}")]
    Write(#[source] io::Error),
    #[error("solver response was not valid JSON: {0}")]
    InvalidResponse(#[source] serde_json::Error),
}

/// Abstract interface for the L1-L5 obligation dispatcher.
pub trait SolverBackend {
    fn discharge(&self, obligation: &Obligation) -> Result<DischargeResult, SolverError>;
}

/// Default backend: shells out to `mvl solve --json` per obligation,
/// writing the obligation as JSON on stdin and reading a `DischargeResult`
/// as JSON from stdout.
#[derive(Debug, Clone, Default)]
pub struct ShellOutSolver {
    /// Path to the `mvl` binary; defaults to `"mvl"`, resolved via `PATH`.
    pub mvl_path: Option<String>,
}

impl ShellOutSolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_binary(mvl_path: impl Into<String>) -> Self {
        ShellOutSolver {
            mvl_path: Some(mvl_path.into()),
        }
    }

    fn binary(&self) -> &str {
        self.mvl_path.as_deref().unwrap_or("mvl")
    }
}

impl SolverBackend for ShellOutSolver {
    fn discharge(&self, obligation: &Obligation) -> Result<DischargeResult, SolverError> {
        let mut child = Command::new(self.binary())
            .args(["solve", "--json"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(SolverError::Spawn)?;

        let input = serde_json::to_vec(obligation).expect("Obligation always serializes");
        child
            .stdin
            .as_mut()
            .expect("stdin was piped")
            .write_all(&input)
            .map_err(SolverError::Write)?;

        let output = child.wait_with_output().map_err(SolverError::Spawn)?;
        serde_json::from_slice(&output.stdout).map_err(SolverError::InvalidResponse)
    }
}
