# mvl-attest

Signed build provenance and verification for mvl-rust binaries
([ADR-0014](../../.openspec/adr/0014-mvl-attest.md),
[spec 011](../../.openspec/specs/011-attestation/spec.md)).

`mvl-manifest` makes a binary carry evidence about itself. `mvl-attest` checks
that evidence against GitHub artifact attestations signed by the CI workflow's
identity, so the claim becomes proof.

```bash
# in CI, next to actions/attest-build-provenance:
cargo mvl attest ./my-app --lockfile Cargo.lock > build-env.json

# before deploying:
cargo mvl verify ./my-app --repo my-org/my-app \
  --workflow .github/workflows/release.yml --ref 'refs/tags/v*'
```

`verify` calls `gh attestation verify` for the signatures, then cross-checks
the embedded `mvl-manifest` record, the `cargo auditable` dependency data and
the lockfile at the signed commit. See [docs/attestation.md](../../docs/attestation.md)
for the threat model and the full workflow. This crate is never linked into an
app binary.
