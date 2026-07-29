# 004 — Information Flow via Types

**Domain:** Information-flow control
**Version:** 0.1.0
**Status:** Implemented
**Date:** 2026-07-29
**Decided by:** ADR-0004

## Overview

Information-flow control asks a question about **values**, not functions. "This string came from an untrusted source" is a property that travels with the string through every assignment, argument pass and return — not a property of one function's body. An attribute on a function cannot express it.

Rust already has a mechanism for properties that travel with values: the type system. So the label lives in the type — `Tainted<T>`, `Secret<T>`, `Labeled<L, T>` — and `rustc` propagates it for free. What is left for a tool to check is not the flow but the **exceptions to it**: the points where a label is deliberately added or stripped.

That inverts the tool's job relative to spec 003. `rust-total` and `rust-effect` check what a body *does*. `rust-ifc` checks that a body's *declassifications are declared*.

### Philosophy

- **Most of the guarantee is `rustc`'s, not ours.** A program that never calls `::new()` or `.into_inner()` needs no checking at all — the types cannot be mixed. This tool exists exclusively to police the escape hatches.
- **Under-match, because this reports a policy violation.** The inverse of spec 002's gate: a false accusation of illegal declassification is an error on correct code, so recognition is a closed name list. *Over-match where the failure mode is "you must change your code", under-match where it is "your code is wrong."*
- **No dataflow, deliberately.** Both boundary crossings are recognised from syntactically-explicit local facts, because the type system has already done the propagation. Adding dataflow would re-derive what `rustc` guarantees.

---

## RFC 2119 Keywords

The key words "MUST", "MUST NOT", "SHOULD", "SHOULD NOT", and "MAY" in this document are to be interpreted as described in [RFC 2119](https://datatracker.ietf.org/doc/html/rfc2119).

---

## Requirements

### Requirement 1: Declassification must be declared by the enclosing function [MUST]

The tool MUST reject a `.into_inner()` call on a recognised labeled type unless the enclosing function carries `#[mvl::relabel(from = "<label>", …)]` naming that label.

The receiver MUST be a bare identifier that is one of the enclosing function's own parameters, whose declared type is `Tainted<T>`, `Secret<T>`, or the two-argument `Labeled<L, T>` form.

**Implementation:** `crates/rust-ifc/src/checks.rs`

**Tests:** `crates/rust-ifc/src/checks.rs::tests::declassify_without_relabel_is_an_error`, `::declassify_with_mismatched_relabel_is_an_error`, `::matching_relabel_declassify_is_fine`, `::direct_labeled_form_is_recognized`

#### Scenario: Stripping a label without permission is rejected

- GIVEN `fn leak<T>(value: Tainted<T>) -> T { value.into_inner() }` with no `#[mvl::relabel]`
- WHEN `cargo mvl-ifc` runs
- THEN a `Level::Error` diagnostic MUST be reported naming the stripped label

#### Scenario: A mismatched declaration does not authorise the crossing

- GIVEN a function carrying `#[mvl::relabel(from = "Secret", …)]` that declassifies a `Tainted<T>` parameter
- WHEN the check runs
- THEN the declassification MUST be rejected — the declaration MUST match the transition exactly

### Requirement 2: Classification must be declared by the enclosing function [MUST]

The tool MUST reject a `::new()` call that classifies into a recognised labeled type unless the enclosing function's `#[mvl::relabel(… to = "<label>")]` declares that transition. The call's own path MUST directly name the label — `Tainted::new(..)`, `Secret::new(..)`, or `Labeled::<L, _>::new(..)` with an explicit turbofish.

**Implementation:** `crates/rust-ifc/src/checks.rs`

**Tests:** `crates/rust-ifc/src/checks.rs::tests::classify_without_relabel_is_an_error`, `::matching_relabel_classify_is_fine`

#### Scenario: Adding a label without permission is rejected

- GIVEN a function calling `Tainted::new(v)` with no matching `#[mvl::relabel]`
- WHEN the check runs
- THEN a `Level::Error` diagnostic MUST be reported

### Requirement 3: Recognition is a closed name list [MUST]

The tool MUST recognise only the literal type names `Tainted`, `Secret` and `Labeled`. It MUST NOT generalise to "any single-generic-argument type with an `.into_inner()` method".

Rationale: that generalisation would immediately flag `RefCell`, `Mutex`, `BufWriter` and every other stdlib type sharing the method name. Holding false positives at zero is the priority for a tool that accuses code of violating a declared policy.

**Implementation:** `crates/rust-ifc/src/checks.rs`

**Tests:** `crates/rust-ifc/src/checks.rs::tests::unrelated_into_inner_on_refcell_is_not_flagged`

#### Scenario: An unrelated `into_inner` is not flagged

- GIVEN a function calling `.into_inner()` on a `RefCell`
- WHEN the check runs
- THEN no diagnostic MUST be reported

### Requirement 4: Label names match the spelling at the recognition site [MUST]

`relabel`'s `from`/`to` strings MUST match the label name exactly as spelled where it is recognised — for the built-in aliases the alias itself (`"Tainted"`, not the underlying `TaintedLabel` marker struct); for `Labeled<L, T>` the name of `L` verbatim.

A string-matched name rather than a resolved type is a consequence of having no type information (spec 001), not a preference.

**Implementation:** `crates/rust-ifc/src/checks.rs`

**Tests:** `crates/mvl/tests/passthrough.rs::custom_phi_label_round_trips_through_ingest_and_release`, `crates/mvl-rust-core/tests/attrs.rs::parses_relabel_attr_with_from_to_and_audit`, `::parses_relabel_attr_without_audit`

#### Scenario: A custom marker label round-trips

- GIVEN a user-defined label type used as `Labeled<PhiLabel, T>` with `#[mvl::relabel]` naming `"PhiLabel"`
- WHEN the value is classified in one function and declassified in another
- THEN both crossings MUST be accepted
- AND the annotated program MUST compile and run unchanged

### Requirement 5: Multi-hop chains are independent hops [MUST]

A value crossing more than one label boundary MUST be checked as a sequence of independent, locally-declared hops. The tool MUST NOT attempt to track a chain across functions.

Malformed source MUST surface as a parse error.

**Implementation:** `crates/rust-ifc/src/checks.rs`

**Tests:** `crates/rust-ifc/src/checks.rs::tests::multi_hop_chain_is_fine_as_two_independent_hops`, `::malformed_source_returns_parse_error`, `crates/rust-ifc/tests/assurance_mode.rs`

#### Scenario: Two declared hops compose without a call graph

- GIVEN one function declaring `from = "Tainted", to = "Reviewed"` and another declaring `from = "Reviewed", to = "_"`
- WHEN the check runs over both
- THEN each hop MUST be accepted on its own local declaration
- AND no cross-function reasoning MUST be required

---

## Known Limitations

- **Three recognition gaps, all silent.** A value that becomes labeled via an intermediate `let`, a generic helper, or a field access is not recognised as a declassification source; a bare `Labeled::new(..)` without turbofish does not reveal `L` syntactically. Pinned by `bare_labeled_new_without_turbofish_is_a_known_gap_not_flagged`.
- **The direction of those gaps differs from every other tool's.** Elsewhere silence means a missed *proof*; here it means a real declassification goes **unpoliced** — a missing check on a security property. The guarantee is therefore "declassifications *of parameters* are declared", not "declassifications are declared".
- **No lattice.** Flat, string-compared label names with no partial order means no "Secret must not flow to Public" reasoning. A lattice needs either resolved types or a declared ordering, and would supersede Requirement 4.
- **`#[mvl::label]` is parsed and unclaimed** (#54). Since Requirement 1's carrier is the *type*, `label` may be redundant by design rather than merely unimplemented.
- **This is the only annotation tool spec 007's runtime enforcement does not touch** — there is no predicate to lower to a runtime check.

---

## Traceability

| Layer | Artefact |
|---|---|
| **Intent** | #10 (`rust-ifc`, v1 scope and the design history for why no call graph is needed), #54 (`label` unclaimed) |
| **Specification** | this document |
| **Decision** | ADR-0004; ADR-0002 (Requirements 2 and 4 of the subset protect this tool's mechanism directly) |
| **Program** | `crates/rust-ifc/src/checks.rs` |
| **Evidence** | `crates/rust-ifc/src/checks.rs::tests` (10 tests), `crates/rust-ifc/tests/assurance_mode.rs` (3 tests), `crates/mvl/tests/passthrough.rs` (label round-trip), `examples/rust-ifc-demo/{compliant,violating}/` |
