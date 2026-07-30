//! `#[mvl::total]` verifier: panic-freedom and terminating recursion
//! (spec Requirement 2). Only functions carrying `#[mvl::total]` are
//! scanned — everything else is untouched. Shipped as `cargo mvl total`
//! (binary `cargo-mvl-total`).
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
