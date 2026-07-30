//! Qualified-subset lint pass: rejects Rust constructs outside the subset
//! `mvl-rust` can verify (spec Requirement 1) — no `unsafe`, no `dyn Trait`,
//! no explicit lifetimes beyond `'static`/`'_`, no `transmute`, and only
//! allowlisted macros. Shipped as `cargo mvl limit` (binary `cargo-mvl-limit`).
//!
//! Every other tool in the workspace assumes this subset; run it first.
//!
//! # Quick example
//!
//! ```
//! use rust_limit::lints::check_source;
//!
//! let compliant = r#"
//!     fn checked_div(n: i32, d: &i32) -> Option<i32> {
//!         if *d == 0 { return None; }
//!         Some(n / *d)
//!     }
//! "#;
//! assert!(check_source(compliant).unwrap().is_empty());
//!
//! let violating = "unsafe fn danger() {}";
//! assert!(!check_source(violating).unwrap().is_empty());
//! ```
//!
//! See [`docs/overview.md`](https://github.com/mvl-lang/mvl-rust/blob/main/docs/overview.md)
//! for how this tool relates to the other four.

pub mod lints;
