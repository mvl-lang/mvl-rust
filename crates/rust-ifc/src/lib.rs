//! IFC label/relabel enforcement for mvl-rust (spec Requirement 5, v1
//! scope per issue #10): a `Labeled` value (see the `mvl` crate's
//! `Tainted`/`Secret`) can only be classified or declassified inside a
//! function whose own `#[mvl::relabel(from = ..., to = ...)]` attribute
//! declares exactly that transition. See [`checks`] for the scanning/
//! checking logic and its documented v1 recognition limits.
//!
//! # Quick example
//!
//! ```
//! use rust_ifc::checks::check_source;
//!
//! let compliant = r#"
//!     #[mvl::relabel(from = "Tainted", to = "_", audit)]
//!     fn trust(value: mvl::Tainted<String>) -> String {
//!         value.into_inner()
//!     }
//! "#;
//! assert!(check_source(compliant).unwrap().is_empty());
//!
//! let violating = r#"
//!     fn leaks(value: mvl::Tainted<String>) -> String {
//!         value.into_inner()
//!     }
//! "#;
//! assert!(!check_source(violating).unwrap().is_empty());
//! ```
//!
//! See [`docs/overview.md`](https://github.com/mvl-lang/mvl-rust/blob/main/docs/overview.md)
//! for how this tool relates to the other four.

pub mod checks;
