# 011 — Signed Provenance and Verification (`mvl-attest`)

**Domain:** Build provenance / deploy-time verification
**Version:** 0.1.0
**Status:** Implemented (Phase 1)
**Date:** 2026-11-14
**Decided by:** ADR-0014; issue #131

## Overview

`mvl-attest` turns the runtime manifest's evidence (spec 010) into proof. CI signs two attestations over the artifact's digest with its workflow identity: GitHub's SLSA provenance and mvl's build-environment predicate. `cargo mvl verify` checks both signatures through `gh attestation verify`, then cross-checks what the signature tools don't: the binary's embedded manifest record, its cargo-auditable data and the lockfile at the signed commit.

---

## RFC 2119 Keywords

The key words "MUST", "MUST NOT", "SHOULD", "SHOULD NOT", and "MAY" in this document are to be interpreted as described in [RFC 2119](https://datatracker.ietf.org/doc/html/rfc2119).

---

## Requirements

### Requirement 1: A genuine artifact verifies [MUST]

`verify` MUST pass when both attestations verify for the artifact's sha256, the signer matches the policy, the build was clean and auditable, and the manifest, the provenance and the lockfile at the signed commit agree.

**Implementation:** `crates/mvl-attest/src/verify.rs`

#### Scenario: Every check passes

- GIVEN an artifact whose attestations, record, auditable data and lockfile all agree
- WHEN it is verified against its own repository, workflow and ref
- THEN all eleven checks pass

**Tests:** `crates/mvl-attest/src/verify.rs::genuine_artifact_passes_every_check`, `crates/mvl-attest/tests/end_to_end.rs::real_auditable_binary_verifies_and_a_modified_copy_does_not`

### Requirement 2: A modified or unsigned artifact fails [MUST]

`verify` MUST fail when no attestation subject matches the artifact's sha256, selecting by digest rather than by position since one attestation can cover several subjects.

**Implementation:** `crates/mvl-attest/src/provenance.rs`

#### Scenario: One flipped byte

- GIVEN a signed binary and a copy with one byte changed
- WHEN the copy is verified with the same attestations
- THEN the `signature` check fails

**Tests:** `crates/mvl-attest/src/verify.rs::modified_binary_fails`, `::unsigned_artifact_fails`, `crates/mvl-attest/src/provenance.rs::modified_artifact_matches_no_subject`, `::selects_the_subject_matching_the_artifact_not_the_first`, `crates/mvl-attest/tests/end_to_end.rs::real_auditable_binary_verifies_and_a_modified_copy_does_not`

### Requirement 3: The signer must match the policy [MUST]

`verify` MUST fail when the certificate's repository, signer workflow or source ref differ from the policy. `--ref` MUST accept `*` wildcards.

**Implementation:** `crates/mvl-attest/src/verify.rs`

#### Scenario: Wrong repository, workflow or ref

- GIVEN attestations from a fork, from another workflow, or from a pull-request ref
- WHEN verified against the release policy
- THEN `repository`, `workflow` or `ref` fails respectively

**Tests:** `crates/mvl-attest/src/verify.rs::wrong_repo_fails`, `::wrong_workflow_fails`, `::wrong_ref_fails`, `::glob_matches_refs`

### Requirement 4: Manifest, provenance and lockfile must agree [MUST]

The manifest's `vcs_commit` MUST equal the certificate's and the SLSA provenance's commit. The lockfile at the signed commit MUST hash to the manifest's and the signed `lockfile_digest`. Every cargo-auditable package MUST be in that lockfile, and the embedded audit data MUST be the data that was signed.

**Implementation:** `crates/mvl-attest/src/verify.rs`, `crates/mvl-attest/src/lockfile.rs`

#### Scenario: Commit or lockfile mismatch

- GIVEN a manifest claiming another commit, or another lockfile digest, than was signed
- WHEN verified
- THEN `commit` or `lockfile` fails

**Tests:** `crates/mvl-attest/src/verify.rs::commit_mismatch_between_manifest_and_provenance_fails`, `::lockfile_mismatch_between_manifest_and_signed_commit_fails`, `::dependency_not_in_signed_lockfile_fails`, `crates/mvl-attest/src/lockfile.rs::maps_lockfile_sources_to_auditable_names`, `::parses_lockfile_packages`

### Requirement 5: Builds must be clean and auditable [MUST]

`verify` MUST fail for a binary without cargo-auditable data, for a record that says `auditable=false`, and for a record that says `vcs_dirty=true`.

**Implementation:** `crates/mvl-attest/src/auditable.rs`, `crates/mvl-attest/src/verify.rs`

#### Scenario: Built without cargo auditable

- GIVEN a real binary built with plain `cargo build`
- WHEN its audit data is read
- THEN reading fails, and verification fails the `auditable` check

**Tests:** `crates/mvl-attest/src/auditable.rs::binary_without_auditable_data_is_rejected`, `::parses_packages_sorted`, `crates/mvl-attest/src/verify.rs::binary_built_without_cargo_auditable_fails`, `::dirty_build_fails`

### Requirement 6: The binary's record is found without running it [MUST]

The verifier MUST find the `mvl-manifest` verification record by scanning the binary's bytes, ignoring stray markers, and MUST fail when there is none or when records conflict.

**Implementation:** `crates/mvl-attest/src/record.rs`

#### Scenario: Stray markers and duplicates

- GIVEN bytes holding a stray marker and two copies of the same record
- WHEN scanned
- THEN the one record is returned; a second, different record is an error

**Tests:** `crates/mvl-attest/src/record.rs::finds_the_record_and_ignores_stray_markers`, `::no_record_is_an_error`, `::conflicting_records_are_an_error`

### Requirement 7: CI can produce the build-environment predicate [MUST]

`cargo mvl attest <binary> --lockfile <path>` MUST print the build-environment predicate: lockfile path and digest, the auditable data's digest, rustc, host OS, build time and run URL. It MUST fail for a binary without cargo-auditable data.

**Implementation:** `crates/mvl-attest/src/predicate.rs`, `crates/mvl-attest/src/cli.rs`

#### Scenario: Predicate for a real binary

- GIVEN the demo built with `cargo auditable`
- WHEN the predicate is generated
- THEN its digests equal the binary's audit digest and the manifest's lockfile digest

**Tests:** `crates/mvl-attest/tests/end_to_end.rs::real_auditable_binary_verifies_and_a_modified_copy_does_not`, `crates/mvl-attest/src/predicate.rs::binary_without_auditable_data_has_no_predicate`, `crates/mvl-attest/src/cli.rs::parses_both_option_spellings`, `::rejects_unknown_and_valueless_options`, `::missing_arguments_are_a_usage_error`

---

## Known Limitations

- `verify` needs `gh` and network access to GitHub; offline verification and native Sigstore verification are later work (ADR-0014).
- The build-environment predicate is reported by the workflow, not measured by the platform.
- cosign/KMS mode is not implemented in Phase 1.
