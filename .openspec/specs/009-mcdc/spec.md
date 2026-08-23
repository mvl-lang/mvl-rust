# 009 — MC/DC Obligations (`rust-mcdc`)

**Domain:** Coverage obligation scanning and discharge
**Version:** 0.1.0
**Status:** Implemented
**Date:** 2026-08-23
**Decided by:** issue #85 (no ADR exists for this tool — design decisions were captured directly in the issue's body and comments, not formalized separately; this spec is the first formalization)

## Overview

`rust-mcdc` is the sixth tool: simulated MC/DC (Modified Condition/Decision Coverage) for stable Rust. Real MC/DC — `-Z coverage-options=mcdc` — is nightly-only, so this tool unbundles MC/DC into two independently-useful halves and builds each with tools this workspace already has:

- **Obligation scanning** (deterministic, `syn`-based): every `if`/`while` condition and `match` guard in a file is a *decision*; each is flattened into its leaf conditions and the minimum vector count MC/DC would require to exercise it independently.
- **Discharge**: joining those obligations against evidence that they were actually exercised.

**This spec's discharge mechanism is tagged tests, not mutation.** An earlier design (issue #85's original body) treated condition-mutation testing as the primary, fully-automatic discharge path. In practice, re-running the whole test suite once per mutant — with no per-mutant timeout — was too disruptive for everyday use against a real codebase, and the tagged-test path already does the real reporting job against an *already-existing* test suite once its relevant tests carry the tagging convention (confirmed against a live corpus, not just in theory). The mutation engine (`mutate`/`discharge` modules) still exists as a library capability but is deliberately not exposed by any CLI or Makefile target — see Requirement 6.

### Pipeline

| Step | Verb | Artifact |
|---|---|---|
| 1. Scan | extract | `obligations.json` |
| 2. Generate | list | tagging convention + ids/vector counts, for a human/LLM to write tests against |
| 3. Run | execute | `cargo test` (ordinary tests, already run by `make test`) |
| 4. Harvest | join | discharge report |

### Philosophy

- **An obligation inventory and its discharge mechanism are the same artifact.** `obligations.json` (step 1) is exactly what step 4 joins against — no separate, drifting representation.
- **Trust the tag, not an interpreter.** `harvest` never re-derives whether a test actually exercises a vector; it counts a passing, correctly-tagged test as evidence. The alternative (empirically deriving independence via mutation) exists as a library capability but isn't the default, for the disruption reasons above.
- **An exhaustive `match` is stronger than per-arm MC/DC.** Rust's own exhaustiveness check already covers every arm; recording it as an obligation would ask for evidence the compiler already provides.

---

## RFC 2119 Keywords

The key words "MUST", "MUST NOT", "SHOULD", "SHOULD NOT", and "MAY" in this document are to be interpreted as described in [RFC 2119](https://datatracker.ietf.org/doc/html/rfc2119).

---

## Requirements

### Requirement 1: Every decision site is extracted with a computable minimum vector count [MUST]

Every `if`/`while` condition and every `match` arm's guard MUST be extracted as a decision. A decision MUST be flattened into its leaf conditions and the `&&`/`||` operators joining them, left to right. A decision's minimum required vector count MUST be `n + 1`, where `n` is its leaf count — a single-leaf decision reduces to plain branch coverage (`1 + 1 = 2`), covered by the same formula without a special case.

A `match` expression itself MUST be recorded as **compiler-void**: exhaustiveness is established by the compiler, not by a test, so it carries no vector requirement. A compiler-void `match` whose exhaustiveness comes from a `_`/catch-all arm rather than every variant being named MUST be flagged `wildcard_risk` — compiler-void still holds (no test can add more coverage than the compiler already guarantees), but a `_` arm can silently absorb a variant added later with no compiler signal. `wildcard_risk` detection is deliberately scoped to `Pat::Wild` only — a bound-name catch-all (`other => ...`) is indistinguishable from a legitimately named unit-variant arm without type information, so it is not flagged, trading a missed rare case for no false positives on the common one.

A `let` pattern (`if let`/`while let`, or one leaf of a `&&`-joined let-chain) MUST NOT be decomposed into its own sub-conditions — it counts as one opaque leaf, same as any other leaf, toward `vectors_required`.

**Implementation:** `crates/rust-mcdc/src/scanner.rs`

#### Scenario: A two-leaf `||` decision requires three vectors

- GIVEN `if !remaining.is_empty() || ancestors.is_empty() { }`
- WHEN the file is scanned
- THEN the decision has 2 leaves and `vectors_required() == 3`

**Tests:** `crates/rust-mcdc/src/scanner.rs::tests::worked_example_delete_rs_decision`, `::two_condition_or_requires_three_vectors`

#### Scenario: An exhaustive `match` on a `_` arm is compiler-void but flagged as a wildcard risk

- GIVEN `match x { 0 => 1, _ => 0 }`
- WHEN the file is scanned
- THEN the `match` decision is `compiler_void: true` with `vectors_required() == 0`, and `wildcard_risk: true`

**Tests:** `crates/rust-mcdc/src/scanner.rs::tests::wildcard_arm_flags_totality_risk`, `::fully_named_exhaustive_match_has_no_wildcard_risk`, `::unguarded_catch_all_binding_is_not_detected_as_wildcard_risk`

#### Scenario: A `match` guard is its own, independent decision

- GIVEN `match x { n if a => n, _ => 0 }`
- WHEN the file is scanned
- THEN two decisions are found: the compiler-void `match` itself, and a one-leaf decision for the guard `a`

**Tests:** `crates/rust-mcdc/src/scanner.rs::tests::match_is_compiler_void_and_guard_is_its_own_decision`

#### Scenario: A `let` pattern is an opaque but counted leaf

- GIVEN `if let Some(n) = x { }`, and separately `if a && let Some(n) = x { }`
- WHEN the file is scanned
- THEN the first decision has exactly 1 leaf (`vectors_required() == 2`); the second has exactly 2 leaves (`vectors_required() == 3`), with the `let` leaf not further decomposed in either case

**Tests:** `crates/rust-mcdc/src/scanner.rs::tests::if_let_is_an_opaque_single_leaf_decision`, `::let_chain_leaf_is_opaque_but_counted`

---

### Requirement 2: Every obligation has a stable, serializable identity [MUST]

Every decision MUST be representable as a serializable `ObligationRecord` — `id`, `file`, `line`, `decision` (source text), `conditions` (leaf count), `vectors_required`, `compiler_void`, `wildcard_risk` — independent of the in-process `Decision` type a scan produces, so `obligations.json` can be written by one process invocation and read by another (scan and harvest are separate invocations by design).

An obligation's `id` MUST be derived from the file's stem and the decision's line number (e.g. `delete_60` for `btree/delete.rs:60`), slugifying any non-alphanumeric stem character to `_`. This id format is NOT guaranteed unique across two files sharing a stem (e.g. two `mod.rs`s) — a caller scanning a whole crate that cares about that collision MUST qualify further itself.

**Implementation:** `crates/rust-mcdc/src/obligation.rs`, `crates/rust-mcdc/src/scanner.rs::Decision::to_record`

#### Scenario: An obligation id is the file stem plus line number, slugified

- GIVEN a decision at `src/btree/delete.rs:60`
- WHEN its id is computed
- THEN the id is `delete_60`

**Tests:** `crates/rust-mcdc/src/obligation.rs::tests::obligation_id_uses_the_file_stem_and_line`, `::obligation_id_slugifies_non_alphanumeric_stem_characters`

---

### Requirement 3: Discharge is by explicit test tagging, joined against real test outcomes [MUST]

A test MUST declare which obligation and which of its required vectors it exercises by including `mcdc__<obligation-id>__v<N>` anywhere in its (possibly module-qualified) name. `harvest` MUST run `cargo test` in a given directory, parse stable libtest's plain-text `test <name> ... ok`/`FAILED` output (not `--format json`, which is nightly-only), and extract every tagged test's `(obligation-id, vector-number)` pair and pass/fail outcome.

An obligation MUST be considered discharged once it is `compiler_void`, OR at least `vectors_required` **distinct** vector numbers each have at least one **passing** tagged test. `harvest` MUST NOT verify that a tagged test actually exercises the vector it claims — the tag is trusted, not derived; this is what distinguishes tagged-test discharge from mutation discharge (Requirement 6).

**Implementation:** `crates/rust-mcdc/src/harvest.rs`

#### Scenario: All required vectors tagged and passing discharges the obligation

- GIVEN an obligation requiring 3 vectors, and 3 tests tagged `mcdc__<id>__v1`, `__v2`, `__v3`, all passing
- WHEN `harvest` runs
- THEN the obligation is `discharged: true` with `vectors_discharged: 3`

**Tests:** `crates/rust-mcdc/tests/harvest.rs::all_three_vectors_tagged_and_passing_discharges_the_obligation`

#### Scenario: A missing vector leaves the obligation undischarged

- GIVEN an obligation requiring 3 vectors, and only 1 tagged, passing test
- WHEN `harvest` runs
- THEN the obligation is `discharged: false` with `vectors_discharged: 1`

**Tests:** `crates/rust-mcdc/tests/harvest.rs::a_missing_vector_leaves_the_obligation_undischarged`

#### Scenario: An untagged passing test does not count toward discharge

- GIVEN an obligation requiring 3 vectors, and a passing test with no `mcdc__` tag in its name
- WHEN `harvest` runs
- THEN the obligation remains `discharged: false` with `vectors_discharged: 0`

**Tests:** `crates/rust-mcdc/tests/harvest.rs::untagged_tests_do_not_count_toward_discharge`

---

### Requirement 4: The CLI exposes `scan`/`generate`/`harvest`; the mutation engine is library-only [MUST]

`cargo-mvl-mcdc` (the standalone binary) MUST expose exactly three subcommands: `scan` (obligation extraction to a file or stdout), `generate` (lists an obligations file's ids, vector counts, and the tagging convention, for a human or LLM session to write tests against), and `harvest` (Requirement 3). It MUST NOT expose a `discharge` subcommand.

`cargo mvl mcdc <FILE>...` (the in-process, source-text-only dispatcher) MUST run the obligation scan and emit it as assurance-JSON's `McdcSection`, where `covered` means **compiler-void**, not discharged — a real boolean decision always reports `covered: false` from this scan alone, regardless of how well-tested it is. It MUST detect `scan`/`harvest`/`generate` typed as its first argument and redirect to the standalone binary with a clear message rather than misreading the keyword as a file path. It MUST detect `discharge` specifically and report that the mutation engine is a library-only capability, not available via any CLI.

**Implementation:** `crates/rust-mcdc/src/main.rs`, `crates/cargo-mvl/src/main.rs::run_mcdc`

#### Scenario: `cargo mvl mcdc discharge` reports the engine is library-only, not a misread file path

- GIVEN the command `cargo mvl mcdc discharge src/lib.rs`
- WHEN it runs
- THEN it exits non-zero with a message naming the engine as library-only, and does NOT report a "failed to read discharge" file error

**Tests:** `crates/cargo-mvl/tests/subcommands.rs::mcdc_redirects_standalone_subcommands_instead_of_misreading_them_as_files`

#### Scenario: `cargo mvl mcdc`'s scan-only `covered` field means compiler-void, not discharged

- GIVEN a file with one compiler-void `match` and one real (undischarged) guard decision
- WHEN `cargo mvl mcdc` runs against it
- THEN the `McdcSection` reports exactly one condition with `covered: true` (the compiler-void one) and one with `covered: false`

**Tests:** `crates/cargo-mvl/tests/subcommands.rs::mcdc_scan_reports_compiler_void_not_discharge`

---

### Requirement 5: The condition-mutation engine exists as a library capability, not a gating mechanism [SHOULD]

A condition-mutation engine SHOULD remain available in `rust_mcdc::mutate`/`rust_mcdc::discharge` for a future, more targeted use (e.g. mutating a single file's tests in isolation) without needing to be rebuilt from scratch. For each leaf condition, it SHOULD generate a `→true` and a `→false` mutant; for each `&&`/`||` operator, it SHOULD generate an operator-flip mutant. Applying a mutant, running `cargo test`, and restoring the original file SHOULD happen one mutant at a time, with the original file's bytes restored unconditionally on drop — a panic or early return during a mutation run MUST NOT leave a mutated file on disk.

An obligation discharged this way is one where `compiler_void` holds, or all of its mutants are killed (the test suite fails against the mutated file). This mechanism MUST NOT be exposed by any CLI subcommand or Makefile target (Requirement 4) — re-running the entire test suite once per mutant, with no per-mutant timeout, was found too disruptive for everyday use.

**Implementation:** `crates/rust-mcdc/src/mutate.rs`, `crates/rust-mcdc/src/discharge.rs`

#### Scenario: A fully-tested decision has every mutant killed

- GIVEN a two-leaf `||` decision and a test suite exercising all 3 MC/DC vectors
- WHEN `discharge_file` runs against it
- THEN every generated mutant is killed and the decision is `discharged() == true`

**Tests:** `crates/rust-mcdc/tests/discharge.rs::fully_exercised_decision_is_discharged`

#### Scenario: An under-tested decision leaves at least one mutant surviving

- GIVEN the same decision, but a test suite exercising only one vector
- WHEN `discharge_file` runs against it
- THEN at least one mutant survives and the decision is `discharged() == false`

**Tests:** `crates/rust-mcdc/tests/discharge.rs::undertested_decision_survives_a_mutant`

#### Scenario: A killed process never leaves a mutated file behind

- GIVEN a `FileGuard` holding a file's original bytes across a mutation run
- WHEN the guard is dropped, for any reason including a panic mid-run
- THEN the file's original bytes are restored, unconditionally

**Implementation:** `crates/rust-mcdc/src/discharge.rs::FileGuard` (`Drop` impl)

---

## Known Scope Limits

- **Single file at a time; no crate-wide obligation index.** `.openspec/mcdc/index.yaml`, proposed in issue #85's worked example, has not been built — a caller scanning multiple files gets multiple independent obligation lists, not one aggregated view.
- **`let` patterns are opaque leaves** (Requirement 1) — no decomposition of the pattern itself.
- **No per-mutant timeout** (Requirement 5) — a mutant that turns a loop guard into `true` can block `cargo test` indefinitely; this is part of why the mechanism stays library-only.
- **`wildcard_risk` cannot distinguish a bound-name catch-all from a legitimately named exhaustive arm** (Requirement 1) — a `syn`-only, no-type-info limitation shared with every other tool in this workspace.
- **The dashboard/report feature** (grouping decisions by single-leaf/multi-leaf, `ADD TEST` suggestions) discussed as a follow-up is explicitly **not** specified here — it does not exist in the implementation yet.
