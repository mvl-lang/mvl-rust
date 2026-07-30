//! `cargo-mvl`: single entry point aggregating all installed `mvl-rust`
//! tools. Shipped as the `cargo mvl` subcommand (binary `cargo-mvl`):
//!
//! ```text
//! cargo mvl check <FILE>...        # run every Gate tool
//! cargo mvl limit|total|refine|effect|ifc <FILE>...   # run one
//! cargo mvl prove|test|assurance <FILE>...   # emit assurance-JSON
//! ```
//!
//! [`check`] runs the five Gate tools; [`prove`] emits `rust-refine`'s
//! obligation trace as assurance-JSON; [`mod@test`] wraps `cargo test` and
//! parses pass/fail/ignored. See the [root README](https://github.com/mvl-lang/mvl-rust)
//! for the full subcommand table.

pub mod check;
pub mod prove;
pub mod test;
