//! Shared infrastructure for `mvl-rust`'s tool crates: the attribute
//! grammar, `syn`-based AST walker, diagnostic emission layer, and
//! solver-integration trait. Tool crates (`rust-limit`, `rust-total`,
//! `rust-refine`, `rust-effect`, `rust-ifc`, `cargo-mvl`) depend on the
//! types exported here; none of it is re-exported by those crates as part
//! of their own public API (spec Requirement 8).
//!
//! Internal crate — if you're writing code annotated with `#[mvl::...]`
//! attributes, depend on [`mvl`](https://docs.rs/mvl) instead; if you're
//! consuming one of the checkers as a library, depend on that tool crate
//! directly (e.g. `rust-refine`). This crate has no stability guarantees
//! of its own beyond what those crates re-expose.

pub mod assurance;
pub mod attrs;
pub mod diagnostics;
pub mod impl_methods;
pub mod solver;
pub mod walker;
