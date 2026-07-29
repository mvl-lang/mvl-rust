# 002 — The Qualified Subset

**Domain:** Language contraction / gate
**Version:** 0.1.0
**Status:** Implemented
**Date:** 2026-07-29
**Decided by:** ADR-0002

## Overview

`rust-limit` rejects Rust source using constructs the other four tools cannot verify. It runs **first** in the `cargo mvl check` pipeline, so the later tools never have to reason about what they cannot model.

The subset is not a style opinion. It is the **precondition under which every other tool's output means anything**: an `#[mvl::ensures]` on a function whose body contains `transmute` is not a weaker guarantee than one without — it is *no* guarantee, delivered in the same font.

`rust-limit` is the one tool with **no annotation surface**. There is nothing to opt into; it is a whole-file syntactic lint.

### Philosophy

- **Each rule names the tool it protects.** A rule justified only by "this is bad style" does not belong here — that is `clippy`'s job.
- **Over-match, because this is a gate.** A false rejection means "change your code"; a false acceptance means a downstream guarantee is void. Precision is traded for soundness, never the reverse (ADR-0001 §5).
- **No reviewed exceptions.** An exception attribute would make the subset's guarantee "someone looked at it", which the four downstream tools cannot compose over.

---

## RFC 2119 Keywords

The key words "MUST", "MUST NOT", "SHOULD", "SHOULD NOT", and "MAY" in this document are to be interpreted as described in [RFC 2119](https://datatracker.ietf.org/doc/html/rfc2119).

---

## Requirements

### Requirement 1: Memory-unsafe constructs are rejected [MUST]

The tool MUST reject `unsafe` blocks, `unsafe fn`, `unsafe impl`, and `unsafe trait`; `std::mem::transmute` calls; and raw address-of expressions (`&raw const`, `&raw mut`).

Rationale: `unsafe` is precisely the escape hatch from what the type system can check, and every tool's reasoning is downstream of the type system holding. `transmute` reinterprets bits with no compiler-checked relationship between source and target type. Once a raw address exists, every subsequent dereference is unsafe by construction.

**Implementation:** `crates/rust-limit/src/lints/unsafe_construct.rs`, `crates/rust-limit/src/lints/transmute.rs`, `crates/rust-limit/src/lints/raw_addr.rs`

#### Scenario: An unsafe block is rejected

- GIVEN a source file containing an `unsafe { }` block
- WHEN `cargo mvl-limit` runs over it
- THEN a `Level::Error` diagnostic MUST be reported at the block's span
- AND the process MUST exit non-zero

**Tests:** `crates/rust-limit/tests/qualified_subset.rs::forbidden_construct_rejected`, `::unsafe_fn_rejected`

#### Scenario: `transmute` is matched on its last path segment

- GIVEN a call whose callee path ends in `transmute`, however it was imported or re-exported
- WHEN the transmute lint runs
- THEN the call MUST be rejected
- AND an unrelated function coincidentally named `transmute` MAY also be rejected — a false positive accepted deliberately, since a false rejection is the safe direction for a gate

**Tests:** `crates/rust-limit/tests/qualified_subset.rs::transmute_call_rejected`

### Requirement 2: Constructs that hide the callee are rejected [MUST]

The tool MUST reject `dyn Trait`, including where nested inside generic arguments (`Box<dyn Any>`).

Rationale: refinement obligations and effect rows attach to *concrete* signatures. A `dyn` call site does not syntactically reveal which implementation runs, so there is no single `requires` to discharge or effect list to check.

**Implementation:** `crates/rust-limit/src/lints/dyn_trait.rs`

#### Scenario: A nested trait object is rejected with a specific message

- GIVEN a signature containing `Box<dyn Any>`
- WHEN the `dyn` lint runs
- THEN the trait object MUST be rejected
- AND the diagnostic SHOULD name the nesting rather than pointing only at the outer type

**Tests:** `crates/rust-limit/tests/qualified_subset.rs::box_dyn_any_gets_a_friendlier_message`

### Requirement 3: Named lifetimes are restricted to elision [MUST]

The tool MUST reject lifetime parameters other than `'static` and `'_`.

Rationale: a named lifetime parameter usually encodes a cross-reference invariant ("the result lives as long as the input") which is itself a refinement obligation that nothing currently models. This is the **weakest-justified** of the rules and the most likely to loosen — it should be revisited when refinements need to describe borrowed data, not before.

**Implementation:** `crates/rust-limit/src/lints/lifetimes.rs`

#### Scenario: `'static` and `'_` are permitted

- GIVEN a signature using only `'static` and `'_`
- WHEN the lifetime lint runs
- THEN no diagnostic MUST be reported

**Tests:** `crates/rust-limit/tests/qualified_subset.rs::static_and_placeholder_lifetimes_are_allowed`

### Requirement 4: Macro invocations are restricted to an allowlist [MUST]

The tool MUST reject macro *invocations* whose name is outside a curated allowlist. `macro_rules!` *definitions* MUST NOT be flagged. Derive and attribute macros are a distinct `syn` syntax form and are **not** covered by this rule.

Rationale: `syn` keeps a macro body as an opaque token stream, so a call, an `unsafe` block, or an effectful operation inside one is *invisible* — not rejected, invisible. A syntactic pass cannot see through expansion.

The allowlist MUST be small and is expected to grow as real use surfaces macros that provably expand to nothing outside Requirements 1–3.

**Implementation:** `crates/rust-limit/src/lints/macros.rs`

#### Scenario: Defining a macro is permitted, invoking an unreviewed one is not

- GIVEN a file that defines a `macro_rules!` macro and invokes a macro outside the allowlist
- WHEN the macro lint runs
- THEN the definition MUST NOT be flagged
- AND the unreviewed invocation MUST be rejected

**Tests:** `crates/rust-limit/tests/qualified_subset.rs::macro_rules_definition_itself_is_not_flagged`, `::macro_outside_allowlist_rejected`

### Requirement 5: The gate is a pure syntactic pass with no annotation surface [MUST]

The tool MUST operate on `syn::parse_file` output with one `syn::visit::Visit` pass per rule, consuming **no** `mvl::` attribute. It MUST NOT require type information or name resolution. Every violation MUST be `Level::Error`; there MUST NOT be a warning tier or a per-construct override.

A malformed source file MUST surface as a parse error rather than a panic or a silent pass.

**Implementation:** `crates/rust-limit/src/lints/mod.rs`, `crates/rust-limit/src/main.rs`

#### Scenario: A compliant file produces no diagnostics

- GIVEN a source file using only subset-compliant constructs
- WHEN `cargo mvl-limit` runs over it
- THEN no diagnostics MUST be reported
- AND the process MUST exit zero

**Tests:** `crates/rust-limit/tests/qualified_subset.rs::whitelisted_construct_accepted`

#### Scenario: Unparseable input is reported, not swallowed

- GIVEN a file that is not valid Rust
- WHEN the tool runs
- THEN a parse error MUST be returned
- AND the tool MUST NOT report the file as compliant

**Tests:** `crates/rust-limit/tests/qualified_subset.rs::malformed_source_returns_parse_error`

---

## Known Limitations

- **The gate's ordering is not enforced outside `cargo mvl check`.** A user invoking `cargo mvl-refine` directly gets refinement output on unrestricted Rust with no warning that the precondition is unmet.
- **Requirement 4 is the one that bites in practice.** Idiomatic Rust uses far more macros than the starter allowlist admits — `thiserror`, `tracing`, `serde_json::json!` and test macros are all outside it.
- **Requirement 3 rejects a large class of correct programs.** Combined with methods in `impl` blocks being unanalysed by the annotation tools (spec 001), the practically verifiable surface today is roughly free functions over owned scalars.
- **`unsafe impl`/`unsafe trait` are the only reason this tool visits `ItemImpl`/`ItemTrait`.** It is therefore the only tool that sees `impl` blocks at all.

---

## Traceability

| Layer | Artefact |
|---|---|
| **Intent** | #4 (`rust-limit`), #18 (implementation), #19 (escape hatch — rejected in its proposed form), #12 (Ferrocene epic) |
| **Specification** | this document |
| **Decision** | ADR-0002 (the subset), ADR-0001 §5 (the greenfield rule it depends on) |
| **Program** | `crates/rust-limit/src/lints/` |
| **Evidence** | `crates/rust-limit/tests/qualified_subset.rs` (15 tests), `crates/rust-limit/tests/assurance_mode.rs` (4 tests), `examples/rust-limit-demo/{compliant,violating}/` |
