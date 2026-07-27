//! Shared JSON schema for assurance-report output (spec Requirement 13) —
//! the contract every tool crate emits against (per-tool emission is #14,
//! not yet built) and every consumer (`cargo-mvl`'s `assurance` subcommand,
//! the MVL playground's assurance pane, CI dashboards, audit tooling)
//! validates against.

pub mod schema;
pub mod version;
