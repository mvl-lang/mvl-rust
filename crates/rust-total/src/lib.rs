//! `#[mvl::total]` verifier: a syntactic panic-risk scan, plus a
//! `decreases`-measure provability check on direct recursion (spec 003
//! Requirements 1 and 3, ADR-0009). Only functions carrying `#[mvl::total]`
//! are scanned — everything else is untouched. Shipped as `cargo mvl total`
//! (binary `cargo-mvl-total`).
//!
//! Panic-freedom is not a proof: only the absence of syntactically obvious
//! panic constructs is established. Termination is closer to one, but
//! still bounded: `decreases` must name a parameter directly, and every
//! direct recursive call must pass a recognized strictly-decreasing
//! argument for it, or the tool rejects it (ADR-0009) — a small recognized
//! shape set, not a general proof. See spec 003's Known Limitations for
//! what this does and doesn't guarantee, and how it differs from MVL's own
//! `total fn`.
//!
//! # Quick example
//!
//! ```
//! use rust_total::checks::check_source;
//!
//! let compliant = r#"
//!     #[mvl::total]
//!     #[mvl::decreases(n)]
//!     fn factorial(n: u64) -> u64 {
//!         if n == 0 { 1 } else { n * factorial(n - 1) }
//!     }
//! "#;
//! assert!(check_source(compliant).unwrap().is_empty());
//!
//! let violating = r#"
//!     #[mvl::total]
//!     fn boom() { panic!("not panic-free") }
//! "#;
//! assert!(!check_source(violating).unwrap().is_empty());
//! ```
//!
//! See [`docs/overview.md`](https://github.com/mvl-lang/mvl-rust/blob/main/docs/overview.md)
//! for how this tool relates to the other four.

pub mod checks;
