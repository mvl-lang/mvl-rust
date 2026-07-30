//! `#[mvl::requires(pred)]`/`#[mvl::ensures(pred)]` refinement obligations
//! for mvl-rust. Proves as much as it can at compile time (layers `L1`
//! through `L4` natively, `L5` via an optional Z3 feature) and falls
//! through to the `mvl`-crate-injected runtime `assert!` for the rest —
//! never silently accepting what it can't decide. See [`checks`] for the
//! scanning/discharge logic, and `crates/mvl-rust-core/src/solver` for the
//! layered solver itself.
//!
//! # Quick example
//!
//! ```
//! use mvl_rust_core::diagnostics::Level;
//! use rust_refine::checks::check_source;
//!
//! // A satisfiable obligation is reported at `Level::Note` ("proven at
//! // L1"), not silently dropped -- `check_source` always reports every
//! // obligation it finds, so "compliant" means "no `Level::Error`", not
//! // "no diagnostics at all".
//! let compliant = r#"
//!     #[mvl::requires(n > 0)]
//!     fn positive_only(n: i32) -> i32 { n }
//!
//!     fn caller() -> i32 { positive_only(5) }
//! "#;
//! assert!(check_source(compliant).unwrap().iter().all(|d| d.level != Level::Error));
//!
//! let violating = r#"
//!     #[mvl::requires(n > 0)]
//!     fn positive_only(n: i32) -> i32 { n }
//!
//!     fn caller() -> i32 { positive_only(-1) }
//! "#;
//! assert!(check_source(violating).unwrap().iter().any(|d| d.level == Level::Error));
//! ```
//!
//! See [`docs/overview.md`](https://github.com/mvl-lang/mvl-rust/blob/main/docs/overview.md)
//! for how this tool relates to the other four.

pub mod checks;
