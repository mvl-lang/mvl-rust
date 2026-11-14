//! Signed build provenance and verification for mvl-rust binaries (ADR-0014).
//!
//! `mvl-manifest` makes a binary carry **evidence**: its own claim about what
//! it is. `mvl-attest` turns that into **proof** by checking the claim against
//! a trust anchor outside the binary: a GitHub artifact attestation, signed
//! keylessly by the CI workflow's identity (Sigstore) and recorded in a
//! transparency log.
//!
//! - [`predicate`]: `cargo mvl attest` builds the build-environment predicate
//!   CI signs next to the SLSA provenance.
//! - [`verify`]: `cargo mvl verify` checks the signatures through
//!   `gh attestation verify`, then cross-checks what the signature tools do
//!   not: the embedded manifest record, the cargo-auditable dependency list and
//!   the lockfile at the signed commit.
//!
//! No cryptography is implemented here; signatures are verified by `gh`.
//! This crate is never linked into an app binary.

pub mod auditable;
pub mod cli;
pub mod lockfile;
pub mod predicate;
pub mod provenance;
pub mod record;
pub mod verify;

use sha2::{Digest, Sha256};

/// sha256 of a byte slice, lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// sha256 of a file, lowercase hex.
pub fn sha256_file(path: &std::path::Path) -> Result<String, String> {
    std::fs::read(path)
        .map(|b| sha256_hex(&b))
        .map_err(|e| format!("cannot read {}: {e}", path.display()))
}
