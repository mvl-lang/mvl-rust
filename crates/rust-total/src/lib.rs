//! `#[mvl::total]` verifier: a syntactic panic-risk scan, plus a
//! `decreases`-measure provability check on direct recursion (spec 003
//! Requirements 1 and 3, ADR-0009). Only functions carrying `#[mvl::total]`
//! are scanned — everything else is untouched. Shipped as `cargo mvl total`
//! (binary `cargo-mvl-total`).
//!
//! Panic-freedom is not a proof: only the absence of syntactically obvious
//! panic constructs is established. Termination is a real proof, but a
//! bounded one: `decreases` must name a parameter directly, and every
//! direct recursive call's argument for it is discharged as an entailment
//! obligation through the same native linear-arithmetic solver
//! `rust-refine` uses for `requires`/`ensures` (ADR-0009) — subtraction of a
//! literal or a `requires`-bounded amount is provable; division/modulo is
//! outside that solver's linear-arithmetic system entirely and never is.
//! See spec 003's Known Limitations for what this does and doesn't
//! guarantee, and how it differs from MVL's own `total fn`.
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
