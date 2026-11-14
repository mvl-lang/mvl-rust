# Build attestation and verification

This page explains what `mvl-manifest` and `mvl-attest` can and cannot tell
you about a running binary, and how to verify one. The design is in
[ADR-0013](../.openspec/adr/0013-runtime-manifest.md) (the manifest) and
[ADR-0014](../.openspec/adr/0014-mvl-attest.md) (signing and verification).

## Evidence versus proof

**Evidence** is what the binary says about itself. `mvl-manifest` embeds it
at build time and logs it at startup: the commit, the lockfile digest, the
source digest, every linked package. It is useful, and it is forgeable:
anyone who can change the binary can change what it says. A malicious
dependency's `build.rs` or proc-macro also runs before the manifest is
generated and can alter what it measures.

**Proof** is a claim checked against something the binary cannot forge. Here
that is a **signature by the CI workflow's identity**: GitHub artifact
attestations, signed keylessly through Sigstore with a short-lived certificate
bound to the repository, the workflow file and the ref, and recorded in a
public transparency log. To forge it, an attacker has to control that
workflow run.

`cargo mvl verify` joins the two: it checks the signatures, then checks that
what the binary claims matches what was signed.

| Threat | Caught by |
|---|---|
| Binary modified after the build | `signature`: no attestation matches its digest |
| Built by someone else, from a fork or another workflow | `repository`, `workflow` |
| Built from an unreviewed branch | `ref` (e.g. require `refs/tags/v*`) |
| Built from uncommitted changes | `clean-build` |
| Manifest claims a different commit or lockfile than was built | `commit`, `lockfile` |
| A dependency that is not in the reviewed lockfile | `dependencies` |
| Built without `cargo auditable` (no exact dependency record) | `auditable` |

Not covered: a compromised workflow or runner (it holds the signing
identity), a malicious dependency that is in the reviewed lockfile, and
libraries the app loads itself at runtime.

## Producing a signed binary

Copy the steps of [`.github/workflows/attest.yml`](../.github/workflows/attest.yml):

1. Build clean and reproducibly:
   `SOURCE_DATE_EPOCH=$(git log -1 --format=%ct) RUSTFLAGS="--remap-path-prefix=$PWD=/build" cargo auditable build --release --locked`
2. `cargo mvl attest <binary> --lockfile <path/to/Cargo.lock> > build-env.json`
3. `actions/attest-build-provenance` with `subject-path: <binary>`
4. `actions/attest` with `subject-path: <binary>`,
   `predicate-type: https://mvl-lang.org/attest/build-env/v1`,
   `predicate-path: build-env.json`

The workflow needs `permissions: id-token: write, attestations: write`.
The app must call `mvl_manifest::log_manifest` (mvl-manifest 0.13.0 or later)
so the binary carries its verification record.

## Verifying a binary

```bash
cargo mvl verify ./my-app \
  --repo my-org/my-app \
  --workflow .github/workflows/release.yml \
  --ref 'refs/tags/v*'
```

It needs `gh` (logged in, or `GH_TOKEN` set). The lockfile at the signed
commit is fetched through the GitHub API; pass `--source <checkout>` to read
it from a local clone instead. Exit code 0 means every check passed:

```
ok   signature        SLSA provenance for sha256 3f2a… (https://github.com/my-org/my-app/actions/runs/…)
ok   build-env        build-environment attestation present
ok   repository       https://github.com/my-org/my-app (want https://github.com/my-org/my-app)
…
ok   dependencies     112 packages match the lockfile
verified: ./my-app
```

Enforce it **outside** the binary: in a deploy step, a Kubernetes admission
controller, or a launcher that verifies before `exec`. A binary that checks
its own signature can be patched to skip the check.

## Key/KMS mode

Keyless signing needs the verifier to reach GitHub and the public Sigstore
instance, and trusts GitHub's OIDC identity. When neither fits (air-gapped or
regulated environments, or a policy that requires a key under your control),
sign with a key held in a KMS instead. Start from
[`tools/ci/attest-kms.yml`](../tools/ci/attest-kms.yml):

1. `cargo mvl attest --slsa > slsa.json` and
   `cargo mvl attest <binary> --lockfile Cargo.lock > build-env.json`
2. `cosign attest-blob --key <KMS URI> --type https://slsa.dev/provenance/v1 --predicate slsa.json --bundle <binary>.slsa.bundle <binary>`,
   and the same for `build-env.json` with
   `--type https://mvl-lang.org/attest/build-env/v1 --bundle <binary>.build-env.bundle`
3. Ship the two bundles next to the binary.

Verify with the same command plus the key:

```bash
cargo mvl verify ./my-app --key awskms:///alias/release-signing \
  --repo my-org/my-app --workflow .github/workflows/release.yml --ref 'refs/tags/v*'
```

What changes: there is no certificate, so `repository`, `workflow` and `ref`
are read from the signed SLSA predicate. They hold as far as you trust
everyone who can use the key. Every other check is identical. The
transparency log is still required; `--no-tlog` turns that off for a signer
with no log.

**Keep the key in a KMS or HSM**, and give CI access to it through OIDC, not
a stored credential. A private key in a file or a CI secret can be copied by
anyone who can read the secret or run a workflow that prints it, and they can
then sign anything without leaving a trace.

## Checking a reproducible rebuild by hand

The signature proves *who* built the binary. A rebuild proves *what* it is
built from, without trusting that builder:

```bash
git clone https://github.com/my-org/my-app && cd my-app
git checkout <vcs_commit from the manifest or the provenance>
export SOURCE_DATE_EPOCH="$(git log -1 --format=%ct)"
export RUSTFLAGS="--remap-path-prefix=$PWD=/build"
cargo auditable build --release --locked     # same toolchain as `rustc` in the build-env attestation
sha256sum target/release/my-app ./my-app     # the two digests must be equal
```

Use the toolchain recorded in the build-environment attestation (`rustc`) and
the same target. If the digests differ, compare the two manifests
(`mvl_manifest::manifest_to_json`) to find which input changed.
