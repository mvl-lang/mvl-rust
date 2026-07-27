//! Shared infrastructure for `mvl-rust`'s tool crates: the attribute
//! grammar, `syn`-based AST walker, diagnostic emission layer, and
//! solver-integration trait. Tool crates (`rust-limit`, `rust-total`,
//! `rust-refine`, `rust-effect`, `rust-ifc`, `cargo-mvl`) depend on the
//! types exported here; none of it is re-exported by those crates as part
//! of their own public API (spec Requirement 8).

pub mod assurance;
pub mod attrs;
pub mod diagnostics;
pub mod solver;
pub mod walker;
