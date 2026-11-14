---
status: Accepted
date: 2026-11-14
---

# ADR-0013: `mvl-manifest` — Log Bound Packages and Build Provenance at Startup

## Context

mvl-lang ships a runtime manifest (`std.runtime`, mvl-lang#803). At compile time it records the app's identity, its toolchain and the packages linked into it. At startup `log_manifest()` writes all of that to the log, starting with the app version. The reference implementation in mvl-lang is:

- `std/runtime.mvl`: types, formatters, `log_manifest()`
- `src/mvl/manifest_embed.rs`: compile-time synthesis
- `build.rs`: `MVL_*` toolchain env vars

mvl-rust has no equivalent. A Rust binary built with the mvl-rust pipeline can't say at runtime which crates it contains, which toolchain built it, from which commit, or under which build configuration. Operators need that to answer "what exactly is running here?" #129 asks to port the manifest to mvl-rust and extend it for **maximum assurance about what is running**. It should record, at build time:

- app identity, the toolchain and the build configuration
- source provenance
- every package linked into the binary *or* used to build it

### Threat model: evidence, not proof

The manifest is **evidence**: the binary's own claim about itself. Anyone who can tamper with the binary can tamper with its manifest, including a self-computed executable digest. A malicious dependency's `build.rs` or proc-macro also runs before ours and can alter the inputs we measure. **The manifest records what was built. It doesn't prove it.**

It becomes **proof** only when something outside the binary checks it:

- **Signed CI provenance** (#131, `mvl-attest`), signed by the CI system's identity and not by our build script.
- **Verification at deploy time** against a trust anchor outside the binary, and an executable digest measured by the platform rather than by the process.
- **Reproducible rebuilds** that produce a byte-identical binary.

The manifest's job is to provide the identifiers that this external verification joins on: `vcs_commit`, `lockfile_digest`, `source_digest`, and the per-crate checksums.

**Out of scope:** libraries the app `dlopen`s itself (plugins), and the app's own config and data files.

## Decision

### 1. A separate crate, not part of `mvl`

A new workspace crate, `mvl-manifest`, holds:

- the **runtime types and formatters**, with no dependencies
- a **`build` feature** with the `build.rs` generator
- a **`log` feature** with `log_manifest()`

Signing and verification live in a separate crate, **`mvl-attest`** (#131). `mvl-attest` depends on `mvl-manifest`, never the other way round, so signing and verification code never ends up in an app binary.

### 2. How it hooks into the build

The user's `build.rs` calls `mvl_manifest::build::generate()`, which writes `$OUT_DIR/mvl_manifest.rs`. The app reads it through `mvl_manifest::manifest!()`. The data sources are:

- `cargo metadata --offline --frozen --filter-platform $TARGET`, run with the build's actual feature set. The implementation must confirm there is no lock contention with the outer `cargo` invocation.
- `Cargo.lock`; `rustc -vV` and `$CARGO -V`.
- Cargo build-script env vars: `TARGET`, `HOST`, `OPT_LEVEL`, `CARGO_CFG_*`, `CARGO_FEATURE_*`, `CARGO_ENCODED_RUSTFLAGS`, `RUSTC_WRAPPER`, `RUSTC_WORKSPACE_WRAPPER`, `CARGO_PKG_*`.
- git.

### 3. Logging only; `cargo-auditable` is the exact dependency record

`cargo metadata` only approximates the real build graph. For example, it can't see feature unification across workspace members. **Release builds MUST use `cargo auditable build --locked`.** That embeds the dependency graph exactly as Cargo built it, readable from outside the binary by `cargo audit bin`, syft and `mvl-attest`. `mvl-manifest`'s log reports the same graph for people to read. It doesn't replace the auditable record.

### 4. Facade: `log` only

`log_manifest()` targets the `log` facade. `tracing` is out of scope for now (see Phase 2).

### 5. mvl-lang is a starting point, not a contract

Keep mvl-lang's field names where the meaning is the same. Drop fields that mean nothing for Rust, and add Rust-specific fields wherever they give more assurance.

### 6. Only reproducible values are embedded in the binary

An embedded field MUST be fully determined by the build inputs (source, lockfile, toolchain, config). Values that depend on the build environment go in the signed CI provenance (`mvl-attest`) instead:

- the CI run id/URL
- the builder identity
- the build host's OS version
- the wall-clock build time

Release builds MUST use `--remap-path-prefix` so absolute paths aren't embedded. Together these rules let release builds be checked by rebuilding them byte for byte.

## Embedded fields

### Shared fields (mvl-lang → mvl-rust)

| Field | mvl-rust value |
|---|---|
| `app_name` / `app_version` | `CARGO_PKG_NAME` / `CARGO_PKG_VERSION` of the binary crate |
| `mvl_version` | version of `mvl-manifest` |
| `runtime_version` | resolved version of the `mvl` crate, or `""` |
| `stdlib_version`, `stdlib_profile` | **dropped**: rustc pins `std`; Rust has no std profiles |
| `packages` | runtime-linked crates (normal deps reachable for the target) |
| `ffi_bridges`, `licenses`, `native_deps`, `assurance` | empty/zero in Phase 1 (see Phase 2) |
| `build.backend` | `"mvl-rust"` |
| `build.rustc_version` / `llvm_version` | `rustc -vV` |
| `build.target` / `profile` | `TARGET` / profile name (`PROFILE` only reports `debug` or `release`; custom profile names are taken from the `OUT_DIR` path) |
| `build.date` | `SOURCE_DATE_EPOCH`, otherwise the git commit timestamp; **never the wall clock** |
| `source_digest` | SHA-256 tree digest over **every build input**: all files Cargo would package for the crate *and* every workspace/path dependency, plus the workspace root `Cargo.toml`, `Cargo.lock`, every `.cargo/config.toml` in effect, and `rust-toolchain.toml` |

### Rust-specific additions (all embedded, all reproducible)

**`PackageInfo`** (used by both `packages` and `build_packages`) gains:

| Field | Meaning |
|---|---|
| `kind` | `runtime` \| `build` \| `proc-macro` |
| `source` | `registry` (with the registry URL) \| `git` (with the resolved commit) \| `path` |
| `checksum` | the `Cargo.lock` sha256 for registry crates. Path deps have none; `source_digest` covers them |
| `purl` | `pkg:cargo/<name>@<ver>`; git deps add `?vcs_url=` |

**`Manifest.build_packages`** lists build-deps and proc-macros. They run code during the build, so they shape the binary even though they aren't linked into it.

**`BuildInfo`** gains:

| Field | Source |
|---|---|
| `host` | `HOST` triple |
| `rustc_commit`, `rustc_channel`, `cargo_version` | `rustc -vV`, `$CARGO -V` |
| `cc_version`, `linker` | the C compiler (`CC` / `cc` crate default) and the linker in effect, each with its version |
| `opt_level`, `debug_assertions`, `panic`, `target_features` | `OPT_LEVEL`, `CARGO_CFG_*` |
| `overflow_checks`, `profile_overrides` | the resolved profile config (there is no stable cfg for overflow checks); per-package overrides (`[profile.*.package.*]`) are listed |
| `features` | the app crate's enabled Cargo features |
| `rustflags` | `CARGO_ENCODED_RUSTFLAGS`, with secrets redacted |
| `rustc_wrapper` | `RUSTC_WRAPPER` / `RUSTC_WORKSPACE_WRAPPER` (e.g. sccache can return tampered cached build output) |
| `substitutions` | `[patch]` / `[replace]` entries, `paths` overrides, `source.replacement` (vendoring/mirrors): dependency swaps that `Cargo.lock` doesn't show |
| `vcs_commit`, `vcs_dirty`, `vcs_describe` | git (empty if there is no git) |
| `lockfile_digest` | sha256 of `Cargo.lock` |
| `reproducible` | whether `SOURCE_DATE_EPOCH` and path remapping were both in effect |
| `auditable` | whether the build ran under `cargo auditable` (detected via its env/wrapper) |

### Runtime environment (collected at startup, not embedded)

`log_manifest()` collects these when the process starts and logs them as their own record:

- `os`, `arch`, kernel/OS version, `pid`, `started_at`, executable path
- the **shared libraries actually loaded**: `dl_iterate_phdr` on Linux, the dyld image list on macOS, the module list on Windows
- the glibc version, where applicable

This covers dependencies Cargo never sees, such as glibc, OpenSSL found via pkg-config, and libSystem.

Optional, behind feature `exe-digest`: the sha256 of the running executable. This is evidence only. The external check is done by `mvl-attest` or the platform.

## Log records emitted by `log_manifest()`

All records go to the dedicated log target **`mvl_manifest`**. If logging is disabled or no logger is initialized, `log_manifest()` falls back to stderr, so the manifest never disappears silently.

1. **`MVL Runtime Manifest`**: summary, plus `vcs_commit`, `lockfile_digest` and `source_digest`
2. **`MVL Build Provenance`**: `BuildInfo`
3. **`MVL Runtime Packages`**
4. **`MVL Build Packages`**
5. **`MVL Runtime Environment`**: host, OS, process info and loaded libraries
6. **WARN** for each condition that weakens assurance:
   - a dirty tree
   - a nightly toolchain
   - path/git deps, or any `substitutions`, in a release build
   - a `rustc_wrapper` in a release build
   - a release build without `cargo auditable`, or without `reproducible`
   - debug assertions on in a release build

## Phase 2 extension (recorded here; implemented in later tickets)

| Field | Source in mvl-rust | mvl-lang equivalent |
|---|---|---|
| `licenses`, `PackageInfo.license` | `cargo metadata` license, deduplicated | Phase 6 |
| `native_deps` | `links = "..."` crates (`-sys`), including the **version of the bundled C library** (e.g. `openssl-src`), not just the crate version; `introduced_by` path | Phase 7 (mvl-lang#1661) |
| `ffi_bridges` | `extern "C"` / `unsafe extern` blocks found by the `mvl-rust-core` AST walker | Phase 3 |
| `assurance` | from a cached `cargo mvl assurance` report (the checkers don't run inside `build.rs`) | Phase 5 |
| `unsafe_usage` | per-crate `forbid(unsafe_code)` / unsafe counts from the `mvl-rust-core` scanner | n/a |
| per-dependency features | `cargo metadata` resolve | n/a |
| `tracing` facade | `tracing` feature | n/a |

## Consequences

- **New crate, opt-in per app.** An app opts in by adding `mvl-manifest` as a dependency and a `build-dependency` (feature `build`), calling `generate()` from its `build.rs`, and calling `log_manifest()` at startup. Nothing changes for apps that don't.
- **Release discipline becomes part of the contract.** `cargo auditable build --locked`, `SOURCE_DATE_EPOCH` and `--remap-path-prefix` are required for a release build to log without WARNs. The manifest makes skipping them visible rather than silent.
- **Build-time cost.** `generate()` runs `cargo metadata`, `rustc -vV`, `cargo -V` and git, and hashes the source tree on every build-script rerun. The implementation must declare precise `cargo:rerun-if-changed` / `rerun-if-env-changed` lines so it doesn't rerun on every build or miss a changed input.
- **Divergence from mvl-lang is deliberate.** `stdlib_version` and `stdlib_profile` are dropped, and many fields are added (decision 5). Consumers that parse both formats must key on `build.backend`.
- **No security claim on its own.** The ADR's threat model limits what the manifest can promise: tamper resistance comes only from #131 (`mvl-attest`), deploy-time verification, and reproducible rebuilds.

## Links

- #129: this ADR's ticket
- #131: `mvl-attest`, for signed CI provenance, signing and verification (depends on `mvl-manifest`, never the reverse)
- mvl-lang#803: the original runtime manifest (`std.runtime`); reference implementation in mvl-lang `std/runtime.mvl`, `src/mvl/manifest_embed.rs`, `build.rs`
- mvl-lang#1661: native dependency tracking (Phase 2 `native_deps`)
- ADR-0007: assurance vocabulary (Phase 2 `assurance` field)
