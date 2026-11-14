//! Runtime manifest for mvl-rust binaries (ADR-0013).
//!
//! At build time, [`build::generate`] (feature `build`) records app identity,
//! the toolchain, the build configuration, source provenance and every
//! package linked into the binary or used to build it. At startup,
//! [`log_manifest`] (feature `log`) logs it all, starting with the app version.
//!
//! The manifest is **evidence, not proof**: the binary's own claim about
//! itself. It provides the identifiers external verification joins on
//! (`vcs_commit`, `lockfile_digest`, `source_digest`, crate checksums);
//! signing and verification live in `mvl-attest`.
//!
//! ```toml
//! [dependencies]
//! mvl-manifest = { version = "0.12", features = ["log"] }
//! [build-dependencies]
//! mvl-manifest = { version = "0.12", features = ["build"] }
//! ```
//!
//! ```ignore
//! // build.rs
//! fn main() { mvl_manifest::build::generate().expect("mvl manifest"); }
//!
//! // main.rs
//! fn main() {
//!     env_logger::init();
//!     mvl_manifest::log_manifest(&mvl_manifest::manifest!());
//! }
//! ```

pub mod format;
#[cfg(any(feature = "build", feature = "log"))]
mod time;
mod types;
pub mod warn;

#[cfg(feature = "build")]
pub mod build;
#[cfg(feature = "log")]
mod env;
#[cfg(feature = "log")]
pub mod logging;

#[cfg(test)]
mod testdata;

pub use format::{manifest_to_block, manifest_to_json, manifest_to_logfmt};
pub use types::*;
pub use warn::{assurance_warnings, Warning};

#[cfg(feature = "log")]
pub use logging::{log_manifest, log_manifest_with_env};

/// The manifest written by [`build::generate`], as a `const`-evaluable
/// [`Manifest`] expression. Requires `generate()` in the crate's `build.rs`.
#[macro_export]
macro_rules! manifest {
    () => {
        include!(concat!(env!("OUT_DIR"), "/mvl_manifest.rs"))
    };
}
