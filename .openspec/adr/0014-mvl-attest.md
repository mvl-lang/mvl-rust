---
status: Accepted
date: 2026-11-14
---

# ADR-0014: `mvl-attest` — Signed Build Provenance and Verification

## Context

ADR-0013 gave mvl-rust binaries a runtime manifest, and is explicit about its limit: the manifest is **evidence**, the binary's own claim about itself. Anyone who can change the binary can change its manifest. It becomes **proof** only when something outside the binary checks it against a trust anchor the binary cannot forge.

#131 asks for that check: sign the build's provenance in CI, and verify a binary against it before it runs.

## Decision

### 1. A separate crate, never linked into an app

`mvl-attest` holds the verification logic and the predicate generator. It depends on `mvl-manifest` (for the record format); `mvl-manifest` never depends on it. The CLI is `cargo mvl attest` / `cargo mvl verify`, through `cargo-mvl`. No signing or verification code ends up in an app binary.

### 2. Trust anchor: keyless signing by default

CI signs with GitHub artifact attestations: Sigstore keyless signing with a short-lived certificate bound to the workflow's OIDC identity (repository, workflow file, ref), recorded in a transparency log. There is no long-lived key to steal.

**Alternative (not in Phase 1): a pinned key in a KMS or HSM**, verified with `cosign`, for air-gapped or regulated environments. A self-signed key kept in a file or a CI secret is discouraged: whoever reads the secret can sign anything.

### 3. What is signed

Two attestations over the **final artifact's digest** (after any strip or codesign step), both signed by the same workflow identity:

1. **SLSA build provenance** (`https://slsa.dev/provenance/v1`, `actions/attest-build-provenance`): source repository and commit, workflow, ref, run.
2. **The build environment** (`https://mvl-lang.org/attest/build-env/v1`, `actions/attest` with a predicate from `cargo mvl attest`): the values ADR-0013 decision 6 keeps out of the binary because they depend on the environment, not the inputs.
   - `lockfile_path`, `lockfile_digest`
   - `auditable_digest`: sha256 of the embedded cargo-auditable data
   - `rustc`, `host_os`, `build_time` (wall clock), `run_url`

GitHub's SLSA predicate cannot carry custom fields, hence the second attestation.

### 4. The verification record in the binary

To cross-check the manifest without running the program, `mvl-manifest` embeds one delimited string, the **verification record**:

```
mvl-manifest-record/v1;app=…;app_version=…;vcs_commit=…;vcs_dirty=…;lockfile_digest=…;source_digest=…;auditable=…;reproducible=…;end
```

`verify` finds it with a byte scan, which works for ELF, Mach-O and PE alike. `log_manifest` keeps it alive under LTO with `std::hint::black_box`.

### 5. What `cargo mvl verify` checks

`gh attestation verify` checks the signature, the transparency log, the certificate chain and the signer (repository and workflow). `verify` selects the result whose **subject digest equals the artifact's sha256** (one attestation can cover several subjects), then cross-checks what the signature tools don't. Every check must pass:

| Check | Passes when |
|---|---|
| `signature` | SLSA provenance verifies for this exact artifact |
| `build-env` | the build-environment attestation verifies for it too |
| `repository`, `workflow`, `ref` | the certificate's repository, signer workflow and ref match the policy (`--ref` takes `*` wildcards) |
| `manifest-record` | the binary carries one verification record |
| `clean-build` | the record says `vcs_dirty=false` |
| `auditable` | cargo-auditable data is present, and the record says `auditable=true` |
| `commit` | manifest `vcs_commit` = certificate commit = SLSA `resolvedDependencies` commit |
| `lockfile` | sha256 of the lockfile **at the signed commit** = manifest `lockfile_digest` = signed `lockfile_digest` |
| `dependencies` | every cargo-auditable package (name, version, source) is in that lockfile, and the embedded data is the data that was signed |

Phase 1 calls `gh`; it implements no cryptography.

### 6. Enforcement happens outside the binary

A binary that checks its own signature can be patched to skip the check. "Run only if it verifies" belongs to the platform: a Kubernetes admission controller, IMA/fs-verity, OS code signing, or a minimal verifying launcher. `mvl-attest` provides the verification logic those can call.

**Amendment (v0.14.0, #137): key/KMS mode.** For verifiers that cannot reach GitHub or the public Sigstore instance, or policies that require a key under the organisation's control, `verify --key <public key or KMS URI>` checks both attestations with `cosign verify-blob-attestation` against cosign bundles (`<binary>.slsa.bundle`, `<binary>.build-env.bundle`). Three things differ from keyless mode:

- **Who generates the SLSA provenance.** `actions/attest-build-provenance` cannot sign with a KMS key, so `cargo mvl attest --slsa` builds the predicate itself, in the same shape as GitHub's, from the GitHub Actions environment or explicit `--repo/--workflow/--ref/--commit`.
- **Who vouches for the identity.** There is no certificate. The `repository`, `workflow` and `ref` checks read the signed SLSA predicate, so they hold only as far as the key holder is trusted: anyone with access to the key can sign any claim. Keyless mode remains the default because the CI platform, not a key holder, vouches for the workflow identity.
- **The transparency log.** Required by default, as in keyless mode; `--no-tlog` turns it off for air-gapped signers that have no log to write to.

Every other check (`clean-build`, `auditable`, `commit`, `lockfile`, `dependencies`) is the same code. `tools/ci/attest-kms.yml` is the CI template; `attest.yml` exercises key mode on every run with an ephemeral key.

## Consequences

- **Release builds need a signing workflow.** `.github/workflows/attest.yml` is the template: a clean `cargo auditable build --locked` with `SOURCE_DATE_EPOCH` and `--remap-path-prefix`, both attestations, then `cargo mvl verify` (must pass) and three runs that must fail (modified binary, wrong repository, wrong ref).
- **Apps must log their manifest** (or otherwise reference `MANIFEST.record`), or `verify` finds no record. This needs `mvl-manifest` 0.13.0 or later.
- **`gh` is a runtime dependency of `verify`**, with network access to GitHub. Offline verification (`--bundle`, `--custom-trusted-root`) and native Sigstore verification are later work.
- **The build-environment predicate is signed by the workflow, not measured by the platform.** Its `host_os` and `build_time` are what the workflow reports; the workflow identity vouches for them, nothing more.
- **Later tickets:** native Sigstore verification without `gh`; example admission policies (Kyverno `verifyImages`, Sigstore policy-controller); a minimal verifying launcher; an automated independent rebuild-and-compare.

## Links

- #131 (this ADR's ticket), #129 / ADR-0013 (the runtime manifest), #130 (its implementation)
- `docs/attestation.md` (threat model and how to verify)
- spec 011
