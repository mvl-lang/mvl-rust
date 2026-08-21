//! Simulated MC/DC for stable Rust: obligation scanning (layer "a") plus
//! condition-mutation discharge (layer "c"), shipped as `cargo mvl mcdc`
//! (binary `cargo-mvl-mcdc`). See issue #85 for the full design.
//!
//! - [`scanner`] extracts every decision (an `if`/`while` condition or a
//!   `match` guard) as a statically enumerable obligation.
//! - [`mutate`] turns one decision into its condition-mutation set.
//! - [`discharge`] applies those mutants to a real file on disk, one at a
//!   time, running `cargo test` to see if each is killed.
//!
//! Discharge policy: `discharged ⇔ compiler-void ∨
//! all-condition-mutants-killed`. Nightly `-Z coverage-options=mcdc` is
//! deliberately out of this crate's regular path -- it's a periodic
//! calibration audit against the simulation, not a gating mechanism (see
//! issue #85's amendment).

pub mod discharge;
pub mod mutate;
pub mod scanner;
