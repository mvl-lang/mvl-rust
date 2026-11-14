# mvl-manifest

Runtime manifest for mvl-rust binaries ([ADR-0013](../../.openspec/adr/0013-runtime-manifest.md),
[spec 010](../../.openspec/specs/010-runtime-manifest/spec.md)).

At build time it records app identity, the toolchain, the build configuration,
source provenance and every package linked into the binary or run during its
build. At startup it logs all of that, plus the runtime environment (including
the shared libraries actually loaded), starting with the app version.

The manifest is **evidence, not proof**: it is the binary's own claim about
itself. It carries the identifiers external verification joins on
(`vcs_commit`, `lockfile_digest`, `source_digest`, crate checksums). Signing
and verification belong to `mvl-attest`.

## Usage

```toml
[dependencies]
mvl-manifest = { version = "0.12", features = ["log"] }

[build-dependencies]
mvl-manifest = { version = "0.12", features = ["build"] }
```

```rust
// build.rs
fn main() {
    mvl_manifest::build::generate().expect("mvl manifest");
}
```

```rust
// main.rs
const MANIFEST: mvl_manifest::Manifest = mvl_manifest::manifest!();

fn main() {
    env_logger::init();
    mvl_manifest::log_manifest(&MANIFEST);
}
```

Records go to log target `mvl_manifest`. If the logger would drop INFO for
that target, or no logger is installed, they go to stderr instead. See
[`examples/mvl-manifest-demo`](../../examples/mvl-manifest-demo).

## Features

| Feature | Adds |
|---|---|
| *(none)* | Types and the logfmt / JSON / block formatters; no dependencies |
| `build` | `build::generate()` for `build.rs` |
| `log` | `log_manifest()` and runtime-environment collection |
| `exe-digest` | sha256 of the running executable in the environment record |

## Release builds

For a release build to log without assurance warnings, build it with
`cargo auditable build --locked`, set `SOURCE_DATE_EPOCH`, and pass
`--remap-path-prefix=<checkout>=/build` in `RUSTFLAGS`. Two such builds of the
same commit, from different directories, give byte-identical binaries.
