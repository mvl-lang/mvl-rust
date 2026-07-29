# 008 — Diagnostics and Assurance Reporting

**Domain:** Output / evidence emission
**Version:** 0.1.0
**Status:** Implemented, with known mislabelling
**Date:** 2026-07-29
**Decided by:** ADR-0006 §5 (provenance); spec 001 (shared schema)

## Overview

Every tool in the workspace produces two outputs: human-facing diagnostics on stderr, and a machine-readable assurance record on stdout. This spec covers both, and the one property they share that matters most:

**a report may not claim more than the analysis established.**

That sounds obvious and is currently violated in three places — a satisfiability check reported identically to an entailment proof, undischarged obligations serialised into a record named for proven ones, and diagnostic text asserting an action the code does not take. Each is filed; each is a requirement below.

This spec is the fourth part of the refinement fan-out (005 obligations, 006 solver, 007 enforcement, 008 reporting) but applies to all five tools.

### Philosophy

- **The evidence layer is the ISPE link most easily faked.** A count of "proven obligations" that silently mixes proof kinds inflates assurance without lying about any single record. Distinguishing kinds is therefore a requirement, not a nicety.
- **Diagnostics describe, they do not promise.** Text asserting a side effect the tool does not perform is worse than silence, because it is load-bearing for a reader's trust.
- **The schema is a contract.** It is consumed by tooling outside this workspace, so shape changes need snapshot evidence.

---

## RFC 2119 Keywords

The key words "MUST", "MUST NOT", "SHOULD", "SHOULD NOT", and "MAY" in this document are to be interpreted as described in [RFC 2119](https://datatracker.ietf.org/doc/html/rfc2119).

---

## Requirements

### Requirement 1: Diagnostics carry a level, a span, and a rendered source snippet [MUST]

Every finding MUST be reported as a diagnostic carrying a severity level, the span it applies to, and a label. Rendering MUST show the offending source line with the span underlined.

Only `Level::Error` MUST fail the build. Informational outcomes MUST be reported at a level that does not.

**Implementation:** `crates/mvl-rust-core/src/diagnostics.rs`

#### Scenario: An informational outcome does not fail the build

- GIVEN a file whose only findings are informational notes
- WHEN a tool runs over it in gate mode
- THEN the process MUST exit zero

**Tests:** `crates/rust-refine/tests/call_sites.rs::an_informational_outcome_does_not_fail_the_build`

#### Scenario: A violation fails the build

- GIVEN a file containing at least one `Level::Error` finding
- WHEN a tool runs over it in gate mode
- THEN the process MUST exit non-zero

**Tests:** `crates/rust-refine/tests/call_sites.rs::a_violated_return_site_is_an_error_and_fails_the_build`

### Requirement 2: Every tool emits the shared assurance schema [MUST]

Each tool MUST support an assurance mode emitting a machine-readable record conforming to the shared schema, independently of its gate-mode behaviour. A read or parse failure MUST be captured as a diagnostic within the report rather than aborting the run.

The schema MUST be stable across releases, with changes caught by snapshot evidence.

**Implementation:** `crates/mvl-rust-core/src/assurance/schema.rs`, `crates/mvl-rust-core/src/assurance/report.rs`

#### Scenario: A compliant file yields a valid record

- GIVEN a source file with no findings
- WHEN a tool runs in assurance mode
- THEN valid JSON conforming to the shared schema MUST be emitted on stdout

**Tests:** `crates/rust-limit/tests/assurance_mode.rs::emits_valid_assurance_json_for_compliant_source`

#### Scenario: An unreadable file is reported, not fatal

- GIVEN a path that cannot be read
- WHEN a tool runs in assurance mode
- THEN the failure MUST appear as a diagnostic inside the report
- AND the tool MUST NOT abort before emitting it

**Tests:** `crates/rust-limit/tests/assurance_mode.rs::assurance_mode_captures_a_read_error_as_a_diagnostic_instead_of_aborting`

### Requirement 3: Obligation kinds must be distinguishable in the report [MUST]

An obligation record MUST carry enough information to distinguish a declaration-site coherence check from a call-site or return-site entailment proof.

Rationale: coherence asks "is this predicate satisfiable" (spec 005 Requirement 1), which is a materially weaker claim than "Γ entails this goal". Reporting them with the same shape and the same layer field inflates the apparent evidence count — on the shipped compliant demo, seven of sixteen reported obligations are coherence checks.

**Implementation:** `crates/mvl-rust-core/src/assurance/schema.rs` — not yet implemented (#56)

#### Scenario: A coherence check is not counted as a proof

- GIVEN a function whose `#[mvl::requires]` is satisfiable but whose call sites are unproven
- WHEN the assurance record is emitted
- THEN a consumer MUST be able to tell that no entailment was proven

### Requirement 4: Undischarged obligations must not be recorded as proven [MUST]

An obligation whose outcome is a runtime check MUST NOT be serialised into a collection named or typed for proven obligations.

**Implementation:** `crates/mvl-rust-core/src/assurance/schema.rs` — not yet implemented (#56)

#### Scenario: A residual is visibly residual

- GIVEN an obligation that could not be discharged by any layer
- WHEN the assurance record is emitted
- THEN a consumer reading the proven collection MUST NOT find it there

### Requirement 5: Obligations must be individually addressable [MUST]

Every obligation MUST carry an identifier unique within its enclosing function, so that a report leaf can be traced back to exactly one obligation.

Rationale: two calls to the same callee, two clauses on one function, and two return points currently collide. That makes report leaves non-addressable, which is the one property an evidence trail needs, and it blocks any keyed discharge cache.

**Implementation:** `crates/rust-refine/src/checks.rs` — not yet implemented (#51)

#### Scenario: Two calls to one callee are separately addressable

- GIVEN a function calling the same callee twice
- WHEN the assurance record is emitted
- THEN the two obligations MUST carry distinct identifiers

### Requirement 6: `cargo mvl` aggregates the five tools in a fixed order [MUST]

The meta-command MUST run the five tools in the order `limit → total → refine → effect → ifc`, as in-process library calls over explicit file paths. It MUST aggregate their diagnostics, and MUST fail the build if any tool reports an error.

**Implementation:** `crates/cargo-mvl/src/check.rs`, `crates/cargo-mvl/src/main.rs`

#### Scenario: The subset gate runs before the annotation tools

- GIVEN a file violating the qualified subset and also carrying refinement annotations
- WHEN `cargo mvl check` runs
- THEN the subset violation MUST be reported
- AND the build MUST fail regardless of the refinement outcomes

**Tests:** `crates/cargo-mvl/tests/check.rs::tool_order_is_limit_total_refine_effect_ifc`, `::check_source_runs_all_five_tools_in_order`

---

## Known Limitations

- **Requirements 3, 4 and 5 are violated today** — #56 and #51. The proven collection currently contains three different things: entailment proofs, satisfiability checks, and undischarged obligations.
- **Diagnostic text claims an action the tool does not take.** Three sites report `"inserting a runtime check"` while inserting nothing; spec 007 Requirement 1 covers the fix.
- **Line coverage is not an ISPE link and is not part of the assurance record.** It measures the program against itself. It is reported in the dashboard because its *interaction* with scenario coverage determines what work to do next — low/low means the tests do not exist and must be written; low scenario with high line means they exist but are not linked, which is traceability work rather than engineering work. A single ratio cannot distinguish those two, and they call for different people on different days.
- **Nothing enforces that the subset gate ran.** Requirement 6 fixes the order inside `cargo mvl check`, but a tool invoked directly analyses unrestricted Rust with no warning that spec 002's precondition is unmet.
- **`cargo mvl` takes explicit file paths, not a crate graph.** It reads no manifest and resolves no dependencies, so a whole-crate assurance claim has to be assembled by the caller.

---

## Traceability

| Layer | Artefact |
|---|---|
| **Intent** | #51 (obligation identity), #56 (proof-kind labelling), #47 (diagnostic honesty), #15 (coverage subcommands) |
| **Specification** | this document; spec 005–007 (what is being reported) |
| **Decision** | ADR-0006 §5 (provenance must distinguish enforced from proven); ADR-0001 §3 (the five-tool dispatcher) |
| **Program** | `crates/mvl-rust-core/src/assurance/`, `crates/mvl-rust-core/src/diagnostics.rs`, `crates/cargo-mvl/src/` |
| **Evidence** | `crates/mvl-rust-core/tests/schema_stability.rs` (3 tests), `crates/mvl-rust-core/tests/diagnostics_ui.rs` (2 tests), five per-tool `tests/assurance_mode.rs` (19 tests), `crates/cargo-mvl/tests/check.rs` (11 tests) |
| **Meta-evidence** | `tools/assurance.py` — the ISPE dashboard measuring completeness (S→P), coverage (E→P) and assurance (E→S) over this spec set |
