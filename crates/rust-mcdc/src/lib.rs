//! Simulated MC/DC for stable Rust: obligation scanning (layer "a") plus
//! condition-mutation discharge (layer "c"), shipped as `cargo mvl mcdc`
//! (binary `cargo-mvl-mcdc`). See issue #85 for the full design.
//!
//! - [`scanner`] extracts every decision (an `if`/`while` condition or a
//!   `match` guard) as a statically enumerable obligation ([`obligation`]'s
//!   `ObligationRecord`, the shared JSON artifact).
//! - Two independent discharge paths over that same obligation:
//!   - [`mutate`] + [`discharge`]: force each leaf `→true`/`→false`, flip
//!     each `&&`↔`||`, and see if `cargo test` fails -- fully automatic,
//!     `discharged ⇔ compiler-void ∨ all-condition-mutants-killed`.
//!   - [`harvest`]: join `obligations.json` against tests explicitly
//!     tagged `mcdc__<id>__v<N>` -- no mutation, trusts a human/LLM wrote
//!     the vector, just counts which ones pass (issue #85's
//!     scan → generate → run → harvest pipeline).
//!
//! Nightly `-Z coverage-options=mcdc` is deliberately in neither path --
//! it's a periodic calibration audit against the simulation, not a
//! gating mechanism (see issue #85's amendment).

pub mod discharge;
pub mod harvest;
pub mod mutate;
pub mod obligation;
pub mod scanner;
