//! The assurance-JSON schema's own version, independent of any crate's
//! version. Bump this whenever [`super::schema::AssuranceReport`]'s shape
//! changes in a way that could break an existing consumer — the
//! `tests/schema_stability.rs` snapshot test fails on any shape drift
//! precisely so this bump doesn't get missed.
//!
//! **Not every failure of that test is a shape change.** `schemars` embeds
//! Rust doc comments as `description`, so editing a comment on any type
//! reachable from `AssuranceReport` fails it too. Regenerate the committed
//! file in that case but leave this constant alone: bumping it would signal
//! a break to consumers when nothing they parse has moved (#64).
//!
//! Each version keeps its own committed file under `schemas/`. Superseded
//! ones are never rewritten, so a consumer pinned to an older version can
//! still validate against the shape it was promised.
//!
//! - `1.0` — initial shape (#13).
//! - `1.1` — `ObligationRecord.kind` added, and the type renamed from
//!   `ProvenObligationRecord` (#56). A new required field, so a strict
//!   `1.0` validator rejects `1.1` documents.
pub const ASSURANCE_SCHEMA_VERSION: &str = "1.1";
