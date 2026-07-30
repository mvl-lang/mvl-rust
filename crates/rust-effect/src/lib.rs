//! `#[mvl::effect(list)]` propagation checking for mvl-rust (spec
//! Requirement 4, v1 scope per issue #9): a caller must declare every
//! effect its same-file, resolvable callees declare, so a function with no
//! `#[mvl::effect(...)]` is a claim of purity — an unverified one where the
//! call graph leaves the file (#67). See [`checks`] for the scanning/
//! checking logic.
//!
//! # Quick example
//!
//! ```
//! use rust_effect::checks::check_source;
//!
//! let compliant = r#"
//!     #[mvl::effect(Console)]
//!     fn log(msg: &str) { println!("{msg}"); }
//!
//!     #[mvl::effect(Console)]
//!     fn report() { log("hi"); }
//! "#;
//! assert!(check_source(compliant).unwrap().is_empty());
//!
//! let violating = r#"
//!     #[mvl::effect(Console)]
//!     fn log(msg: &str) { println!("{msg}"); }
//!
//!     fn report() { log("hi"); }
//! "#;
//! assert!(!check_source(violating).unwrap().is_empty());
//! ```
//!
//! See [`docs/overview.md`](https://github.com/mvl-lang/mvl-rust/blob/main/docs/overview.md)
//! for how this tool relates to the other four.

pub mod checks;
