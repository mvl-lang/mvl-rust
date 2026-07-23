//! Qualified-subset lint pass: rejects Rust constructs outside the subset
//! `mvl-rust` can verify (spec Requirement 1). Shipped as the `cargo mvl-limit`
//! subcommand (binary `cargo-mvl-limit`).

pub mod lints;
