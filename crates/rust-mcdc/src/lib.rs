//! Simulated MC/DC for stable Rust: obligation scanning (layer "a") plus
//! tagged-test discharge, shipped as `cargo mvl mcdc` (binary
//! `cargo-mvl-mcdc`). See issue #85 for the full design.
//!
//! - [`scanner`] extracts every decision (an `if`/`while` condition or a
//!   `match` guard) as a statically enumerable obligation ([`obligation`]'s
//!   `ObligationRecord`, the shared JSON artifact).
//! - [`harvest`] is the CLI-exposed discharge path (scan → generate → run →
//!   harvest, issue #85): join `obligations.json` against tests explicitly
//!   tagged `mcdc__<id>__v<N>` -- trusts a human/LLM wrote the vector, just
//!   counts which ones pass. This is a real reporting tool against an
//!   *already-existing* test suite once its relevant tests carry that tag,
//!   not merely a fallback for people who forgot to tag.
//!
//! [`mutate`]/[`discharge`] (condition-mutation testing -- force each leaf
//! `→true`/`→false`, flip each `&&`↔`||`, see if `cargo test` fails) are
//! kept as a **library-only** capability, not wired into this crate's CLI
//! or `cargo-mvl`: re-running the entire test suite once per mutant, with
//! no per-mutant timeout, was too heavy and disruptive for everyday use
//! against a real codebase. The obligation/mutant machinery stays here in
//! case a future, more targeted use (e.g. mutating just one file's tests
//! in isolation) needs it, but nothing in this workspace calls it today.
//!
//! Nightly `-Z coverage-options=mcdc` is deliberately unused here -- it's
//! a periodic calibration audit against the simulation, not a gating
//! mechanism (see issue #85's amendment).

pub mod discharge;
pub mod harvest;
pub mod mutate;
pub mod obligation;
pub mod scanner;
