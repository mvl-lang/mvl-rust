# 010 — Runtime Manifest (`mvl-manifest`)

**Domain:** Build provenance / startup evidence
**Version:** 0.1.0
**Status:** Implemented (Phase 1)
**Date:** 2026-11-14
**Decided by:** ADR-0013; issue #130 (Phase 1)

## Overview

`mvl-manifest` lets a binary say what it is. At build time, `mvl_manifest::build::generate()` (called from the app's `build.rs`) embeds a `const` manifest: app identity, toolchain, build configuration, source provenance and every package that is linked into the binary or runs during its build. At startup, `mvl_manifest::log_manifest()` logs that manifest plus the runtime environment through the `log` facade.

The manifest is **evidence, not proof** (ADR-0013 threat model): it is the binary's own claim about itself. It carries the identifiers that external verification (`mvl-attest`, #131) joins on: `vcs_commit`, `lockfile_digest`, `source_digest` and per-crate checksums.

Phase 2 fields (`licenses`, `native_deps`, `ffi_bridges`, `assurance`) exist in the types but stay empty or zero.

---

## RFC 2119 Keywords

The key words "MUST", "MUST NOT", "SHOULD", "SHOULD NOT", and "MAY" in this document are to be interpreted as described in [RFC 2119](https://datatracker.ietf.org/doc/html/rfc2119).

---

## Requirements

### Requirement 1: Packages are split into runtime-linked and build-time sets [MUST]

The generator MUST list normal dependencies linked for the target in `packages`, and build-dependencies, proc-macros and their dependencies in `build_packages`. Dev-dependencies MUST NOT appear. The runtime set MUST follow Cargo's per-context feature resolution, so a crate's build-only optional dependencies do not leak into `packages`.

**Implementation:** `crates/mvl-manifest/src/build/graph.rs`

#### Scenario: Build-deps and proc-macros land in build_packages

- GIVEN an app with a path build-dependency and a path proc-macro
- WHEN the manifest is generated
- THEN both appear in `build_packages` with kind `build` / `proc-macro`, and neither appears in `packages`

**Tests:** `crates/mvl-manifest/tests/fixture.rs::fixture_packages_sources_and_substitutions`, `crates/mvl-manifest/src/build/graph.rs::splits_runtime_and_build_and_drops_dev`

#### Scenario: Feature-union leaks are pruned

- GIVEN a crate used both at runtime and as a build-dependency with different features
- WHEN the manifest is generated
- THEN its build-only optional dependencies appear only in `build_packages`

**Tests:** `crates/mvl-manifest/src/build/graph.rs::linked_set_prunes_feature_union_leaks`, `::parses_cargo_tree_lines`

### Requirement 2: Every package records its source, checksum and purl [MUST]

Registry packages MUST carry the registry URL and the `Cargo.lock` sha256. Git packages MUST carry the resolved commit and a purl with `?vcs_url=`. Path packages MUST carry no checksum; `source_digest` covers them.

**Implementation:** `crates/mvl-manifest/src/build/graph.rs`

#### Scenario: Registry, path and git dependencies

- GIVEN an app depending on `log` from crates.io, a path crate and a crate from a local git repository
- WHEN the manifest is generated
- THEN `log` has a 64-hex checksum, the path crate has none, and the git crate records the repository's HEAD commit

**Tests:** `crates/mvl-manifest/tests/fixture.rs::fixture_packages_sources_and_substitutions`, `crates/mvl-manifest/src/build/graph.rs::registry_package_carries_lock_checksum`, `::git_package_records_commit_and_vcs_url`, `::sparse_registry_keeps_protocol`

### Requirement 3: Dependency substitutions are recorded [MUST]

`[patch]` and `[replace]` in the workspace manifest, and `[patch]`, `paths` and `source` replacement in `.cargo/config.toml` files, MUST be listed in `build.substitutions`.

**Implementation:** `crates/mvl-manifest/src/build/config.rs`

#### Scenario: A patch and a paths override

- GIVEN a workspace with `[patch.crates-io]` and a `.cargo/config.toml` `paths` entry
- WHEN the manifest is generated
- THEN both entries appear in `substitutions`

**Tests:** `crates/mvl-manifest/tests/fixture.rs::fixture_packages_sources_and_substitutions`, `crates/mvl-manifest/src/build/config.rs::manifest_patch_and_replace_are_substitutions`, `::config_paths_patch_and_source_replacement_are_substitutions`

### Requirement 4: The build configuration is recorded [MUST]

The generator MUST record the profile (including custom profile names), `overflow_checks` resolved through the profile's `inherits` chain, per-package profile overrides, the toolchain (`rustc -vV`, `cargo -V`), the C compiler and linker with their versions, and the rustc wrappers by basename.

**Implementation:** `crates/mvl-manifest/src/build/config.rs`, `crates/mvl-manifest/src/build/toolchain.rs`

#### Scenario: Custom profile inherits from release

- GIVEN `[profile.release-lto] inherits = "release"` and no explicit `overflow-checks`
- WHEN the profile is resolved
- THEN `overflow_checks` is `false` and per-package overrides of `release` are listed

**Tests:** `crates/mvl-manifest/src/build/config.rs::overflow_checks_follow_inherits_and_defaults`, `::config_profile_beats_manifest`, `crates/mvl-manifest/src/build/mod.rs::profile_name_comes_from_out_dir`

#### Scenario: Toolchain identity

- GIVEN `rustc -vV` output and `-C linker=` flags
- WHEN they are parsed
- THEN release, LLVM version, commit, channel and linker are extracted

**Tests:** `crates/mvl-manifest/src/build/toolchain.rs::parses_rustc_vv`, `::linker_flag_both_spellings`, `::wrapper_paths_are_reduced_to_basenames`

### Requirement 5: Rustflags are redacted before embedding [MUST]

Values that look like secrets, URL credentials and the `FROM` side of `--remap-path-prefix` MUST NOT be embedded.

**Implementation:** `crates/mvl-manifest/src/build/rustflags.rs`

#### Scenario: Secrets and build paths are redacted

- GIVEN rustflags with `--api-token=…`, `https://user:pw@…` and `--remap-path-prefix=/home/me/src=/build`
- WHEN they are redacted
- THEN each secret value and the remap source path become `<redacted>`

**Tests:** `crates/mvl-manifest/src/build/rustflags.rs::secret_like_values_are_redacted`, `::url_credentials_are_redacted`, `::remap_source_path_is_redacted_both_spellings`, `::ordinary_flags_pass_through`

### Requirement 6: Only reproducible values are embedded [MUST]

Embedded values MUST be determined by the build inputs. `build.date` MUST come from `SOURCE_DATE_EPOCH`, otherwise the git commit time, never the wall clock. `source_digest` MUST key files by location-independent names. Two clean release builds of the same commit with `SOURCE_DATE_EPOCH` and `--remap-path-prefix`, from different directories, MUST produce a byte-identical manifest and binary.

**Implementation:** `crates/mvl-manifest/src/build/digest.rs`, `crates/mvl-manifest/src/build/mod.rs`

#### Scenario: Byte-identical rebuilds from different directories

- GIVEN two clones of the same commit in different directories
- WHEN each is built with `--release`, `SOURCE_DATE_EPOCH` and `--remap-path-prefix=<clone>=/build`
- THEN the manifests are equal, `reproducible` is true and the binaries are byte-identical

**Tests:** `crates/mvl-manifest/tests/fixture.rs::reproducible_builds_from_different_directories_are_byte_identical`, `crates/mvl-manifest/src/build/digest.rs::digest_depends_on_keys_and_contents_not_order`, `crates/mvl-manifest/src/build/config.rs::config_keys_do_not_depend_on_checkout_location`, `crates/mvl-manifest/src/time.rs::formats_known_instants`

### Requirement 7: The manifest is never stale [MUST]

The generator MUST emit `cargo:rerun-if-changed` for every hashed file, the source directories, `.git/HEAD`, `.git/index` and the current branch ref, and `cargo:rerun-if-env-changed` for every environment variable it reads.

**Implementation:** `crates/mvl-manifest/src/build/mod.rs`, `crates/mvl-manifest/src/build/vcs.rs`, `crates/mvl-manifest/src/build/digest.rs`

#### Scenario: Edits, commits, lockfile and config changes are picked up

- GIVEN a built fixture
- WHEN a source file is edited, the edit committed, `Cargo.lock` changed and `.cargo/config.toml` changed, rebuilding after each step
- THEN `source_digest`, `vcs_commit`/`vcs_dirty` and `lockfile_digest` change accordingly, and an unchanged rebuild is stable

**Tests:** `crates/mvl-manifest/tests/fixture.rs::staleness_rebuild_picks_up_source_commit_lockfile_and_config_changes`, `crates/mvl-manifest/src/build/digest.rs::top_dir_only_for_nested_files`, `::generated_package_entries_are_skipped`

#### Scenario: The generator emits rerun directives for what it read

- GIVEN the generator run in-process on this crate with Cargo's build-script environment
- WHEN it writes the manifest
- THEN the directives cover `Cargo.lock`, `src/` and each source file, and every environment variable it read; a missing `CARGO` variable is a clear error

**Tests:** `crates/mvl-manifest/src/build/mod.rs::generates_manifest_for_this_crate`, `::missing_cargo_env_is_a_clear_error`

### Requirement 8: Formatters keep mvl-lang's key names [MUST]

`manifest_to_logfmt`, `manifest_to_json` and `manifest_to_block` MUST use mvl-lang's keys for shared fields (`app`, `app_version`, `mvl`, `runtime`, `backend`, `built`, `rustc`, `llvm`, `packages`, `ffi`, `source`/`source_digest`), followed by Rust-specific fields.

**Implementation:** `crates/mvl-manifest/src/format.rs`

#### Scenario: Golden output

- GIVEN a fixed manifest
- WHEN it is formatted as logfmt, JSON and block
- THEN the output matches the golden text

**Tests:** `crates/mvl-manifest/src/format.rs::golden_logfmt`, `::golden_json`, `::golden_block`, `::logfmt_quotes_only_when_needed`, `::json_escapes_control_and_quote`, `crates/mvl-manifest/src/build/codegen.rs::string_literals_are_escaped`, `::renders_package_with_git_source`

### Requirement 9: log_manifest emits the ADR-0013 records [MUST]

`log_manifest()` MUST emit, on target `mvl_manifest` and starting with the app version: `MVL Runtime Manifest`, `MVL Build Provenance`, `MVL Runtime Packages`, `MVL Build Packages`, `MVL Runtime Environment`, then one WARN per assurance-weakening condition. When the logger would drop INFO for that target, or no logger is installed, the records MUST go to stderr.

**Implementation:** `crates/mvl-manifest/src/logging.rs`

#### Scenario: Records in order

- GIVEN a manifest and a runtime environment
- WHEN the records are built
- THEN the five records come first, in order, followed by the warnings

**Tests:** `crates/mvl-manifest/src/logging.rs::emits_five_records_in_order_then_warnings`, `::manifest_record_starts_with_app_version`, `::environment_record_lists_loaded_libraries`, `crates/mvl-manifest/src/format.rs::package_records_show_source_and_checksum`

#### Scenario: No logger installed

- GIVEN a process with no logger installed
- WHEN `log_manifest` runs
- THEN the records are written to stderr

**Tests:** `crates/mvl-manifest/src/logging.rs::no_logger_installed_means_stderr_fallback`, `crates/mvl-manifest/tests/fixture.rs::fixture_binary_falls_back_to_stderr_without_a_logger`

### Requirement 10: Assurance-weakening conditions are warned about [MUST]

A WARN MUST be emitted for: a dirty tree; a non-stable toolchain; and, in release builds, path/git dependencies, substitutions, a rustc wrapper other than `cargo-auditable`, a build without `cargo auditable`, a build that is not reproducible, and debug assertions.

**Implementation:** `crates/mvl-manifest/src/warn.rs`

#### Scenario: Each condition warns on its own

- GIVEN a clean release manifest with exactly one condition introduced
- WHEN the warnings are computed
- THEN exactly that warning is reported

**Tests:** `crates/mvl-manifest/src/warn.rs::clean_release_has_no_warnings`, `::dirty_tree_warns`, `::nightly_toolchain_warns`, `::path_and_git_deps_in_release_warn`, `::substitution_in_release_warns`, `::rustc_wrapper_in_release_warns`, `::auditable_wrapper_is_not_a_wrapper_warning`, `::not_auditable_warns`, `::not_reproducible_warns`, `::debug_assertions_in_release_warn`

#### Scenario: Debug builds only warn about tree and toolchain

- GIVEN a debug build with path deps and no auditable or reproducible settings
- WHEN the warnings are computed
- THEN none of the release-only warnings are reported

**Tests:** `crates/mvl-manifest/src/warn.rs::path_deps_in_debug_do_not_warn`

### Requirement 11: The runtime environment lists loaded libraries [MUST]

`RuntimeEnv::collect()` MUST report OS, arch, kernel version, pid, start time and executable path, and the shared libraries actually loaded (Linux `dl_iterate_phdr`, macOS dyld, Windows module list), plus the glibc version where applicable. With feature `exe-digest` it MUST report the executable's sha256.

**Implementation:** `crates/mvl-manifest/src/env.rs`

#### Scenario: libc is among the loaded libraries

- GIVEN a test process on Linux or macOS
- WHEN the environment is collected
- THEN the library list is non-empty and includes libc / libSystem

**Tests:** `crates/mvl-manifest/src/env.rs::loaded_libraries_include_libc`, `::collects_process_facts`, `::glibc_version_reported`, `::exe_digest_is_sha256_hex`

#### Scenario: App features map back from Cargo's environment

- GIVEN `CARGO_FEATURE_DEFAULT` and `CARGO_FEATURE_FANCY_IO` set
- WHEN the enabled features are computed
- THEN they are `default` and `fancy-io`

**Tests:** `crates/mvl-manifest/src/build/graph.rs::features_map_back_from_env`

### Requirement 12: `runtime` reports the Rust standard library [MUST]

`runtime_version` MUST be `std-<rustc release>`, taken from `rustc -vV`, for every app, whether or not it links the `mvl` crate. A linked `mvl` crate appears in `packages` like any other dependency.

**Implementation:** `crates/mvl-manifest/src/build/mod.rs`

#### Scenario: A plain Rust app reports its std version

- GIVEN an app that links no `mvl` crate, built with rustc 1.99.1
- WHEN the manifest is generated
- THEN `runtime` is `std-1.99.1`, not empty

**Tests:** `crates/mvl-manifest/src/build/mod.rs::runtime_is_the_rust_std_version`, `::generates_manifest_for_this_crate`, `crates/mvl-manifest/tests/fixture.rs::fixture_packages_sources_and_substitutions`

---

## Known Limitations

- `cargo metadata` approximates the real build graph for `build_packages` (one node per package, features unioned; build-deps filtered for the target platform). `cargo auditable` is the exact record (ADR-0013 decision 3).
- `.cargo/config.toml` files are discovered upward from the crate directory, not from the directory Cargo was invoked in; `--config` CLI overrides are not visible to build scripts.
- Adding a new file at a package's top level (not inside `src/`, `tests/`, …) does not trigger a rerun until another watched input changes.
- The Windows module-list path compiles but has no CI runner.
