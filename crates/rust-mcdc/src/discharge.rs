//! Layer-(c) discharge: for every decision, apply each of its
//! [`crate::mutate`] mutants to the real file on disk, run `cargo test`,
//! and record whether the mutant was killed (test suite failed) or
//! survived (test suite still passed).
//!
//! **Safety.** This is the only tool in the workspace that ever writes to
//! a file it's analyzing. Every mutant is applied, tested, and reverted
//! one at a time -- [`FileGuard`] holds the original bytes for the whole
//! run and restores them (a) after each mutant, and (b) unconditionally on
//! drop, so a panic or an early `?` return can never leave a mutated file
//! behind. Run this only against a working tree with no uncommitted
//! changes you care about losing if the process is killed mid-mutant.
//!
//! **Known scope limit:** no per-mutant timeout yet -- an infinite-looping
//! mutant (e.g. an operator flip that turns a loop guard into `true`)
//! blocks `cargo test` indefinitely. Same category of known simplification
//! as `cargo-mvl::test`'s untimed `cargo test` shellout.

use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

use thiserror::Error;

use crate::mutate::{self, Mutant};
use crate::scanner::{scan_source, Decision, ScanError};

#[derive(Debug, Error)]
pub enum DischargeError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error(transparent)]
    Scan(#[from] ScanError),
    #[error("failed to spawn `cargo test`: {0}")]
    Spawn(#[source] io::Error),
}

/// One decision's discharge result: how many of its mutants were killed,
/// out of how many total.
#[derive(Debug, Clone)]
pub struct DecisionOutcome {
    pub line: usize,
    pub decision: String,
    pub vectors_required: usize,
    pub compiler_void: bool,
    pub mutants_total: usize,
    pub mutants_killed: usize,
    pub survived_descriptions: Vec<String>,
}

impl DecisionOutcome {
    /// `discharged ⇔ compiler-void ∨ all-condition-mutants-killed`
    /// (issue #85's amended policy).
    pub fn discharged(&self) -> bool {
        self.compiler_void || (self.mutants_total > 0 && self.mutants_killed == self.mutants_total)
    }
}

/// Holds a file's original bytes for the duration of a discharge run and
/// restores them unconditionally when dropped -- see the module doc.
struct FileGuard<'a> {
    path: &'a Path,
    original: String,
}

impl<'a> FileGuard<'a> {
    fn new(path: &'a Path) -> Result<Self, DischargeError> {
        let original = fs::read_to_string(path).map_err(|source| DischargeError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Ok(FileGuard { path, original })
    }

    fn write(&self, text: &str) -> Result<(), DischargeError> {
        fs::write(self.path, text).map_err(|source| DischargeError::Io {
            path: self.path.display().to_string(),
            source,
        })
    }

    fn restore(&self) -> Result<(), DischargeError> {
        self.write(&self.original)
    }
}

impl Drop for FileGuard<'_> {
    fn drop(&mut self) {
        let _ = fs::write(self.path, &self.original);
    }
}

/// Runs `cargo test` in `run_dir`, returning whether the whole suite
/// passed (compile failure counts as a fail, same as a normal `cargo
/// test` invocation would report it).
fn cargo_test_passes(run_dir: &Path) -> Result<bool, DischargeError> {
    let status = Command::new("cargo")
        .arg("test")
        .current_dir(run_dir)
        .status()
        .map_err(DischargeError::Spawn)?;
    Ok(status.success())
}

fn decision_line(decision: &Decision) -> usize {
    decision.site.start().line
}

/// Discharges every decision in `path` by mutation testing, running
/// `cargo test` from `run_dir` (typically the crate root) once per mutant.
///
/// A mutant is *killed* when `cargo test` fails against the mutated file,
/// *survived* when the suite still passes -- MC/DC's independence
/// criterion, demonstrated empirically rather than proven statically.
pub fn discharge_file(path: &Path, run_dir: &Path) -> Result<Vec<DecisionOutcome>, DischargeError> {
    let guard = FileGuard::new(path)?;
    let decisions = scan_source(&guard.original)?;

    let mut outcomes = Vec::with_capacity(decisions.len());
    for decision in &decisions {
        let mutants = mutate::mutants_for(decision);
        let mut killed = 0usize;
        let mut survived_descriptions = Vec::new();

        for mutant in &mutants {
            if run_and_record(&guard, run_dir, mutant)? {
                killed += 1;
            } else {
                survived_descriptions.push(mutant.description.clone());
            }
        }

        outcomes.push(DecisionOutcome {
            line: decision_line(decision),
            decision: decision.text.clone(),
            vectors_required: decision.vectors_required(),
            compiler_void: decision.compiler_void,
            mutants_total: mutants.len(),
            mutants_killed: killed,
            survived_descriptions,
        });
    }

    guard.restore()?;
    Ok(outcomes)
}

/// Applies `mutant`, runs `cargo test` in `run_dir`, restores the original
/// file, and returns whether the mutant was killed (suite failed).
fn run_and_record(
    guard: &FileGuard<'_>,
    run_dir: &Path,
    mutant: &Mutant,
) -> Result<bool, DischargeError> {
    guard.write(&mutate::apply(&guard.original, mutant))?;
    let passed = cargo_test_passes(run_dir)?;
    guard.restore()?;
    Ok(!passed)
}
