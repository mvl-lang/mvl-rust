---
domain: workspace
version: 0.2.0
status: active
date: 2026-07-29
---

# 001 — System Overview

The `mvl-rust` workspace is a second implementation of MVL's language guarantees, expressed as Rust attribute macros and lint passes. It exists so that Rust codebases can adopt MVL's guarantees incrementally, so that MVL's semantics are validated against a completely independent parser and typechecker, and so that certified-domain adoption can ride Ferrocene's existing DO-178C qualification rather than requiring a new qualification for MVL itself.

## Vision

**MVL's ideas belong in Rust too.** Refinement types, effect tracking, totality, and information flow control are not MVL-specific — they are properties any statically-typed language can express. Proving they work in Rust is what turns MVL from "a project's opinion" into "a family of language guarantees that happens to have two implementations."

Concretely: an engineering team on Rust adds `#[mvl::requires(x > 0)]` to a new function and the same discharge machinery MVL uses closes the obligation. If the team eventually migrates to MVL, the semantics carry over — only the syntax differs. If the team stays on Rust, they still have every guarantee MVL was pitched to deliver.

**The LLM angle is load-bearing.** LLMs already generate excellent Rust and know the attribute idiom. Getting an LLM to produce `#[mvl::requires(x > 0)] fn f(x: i32) -> i32 { x + 1 }` is a much smaller ask than getting it to produce syntactically-correct MVL. `mvl-rust` is where the LLM-native verified-code story lands soonest against the largest audience.

**The Ferrocene angle is opportunistic, not central.** Ferrocene qualifies Rust for DO-178C already. `mvl-rust` on top of Ferrocene delivers verified-Rust-for-certified-domains without qualifying MVL itself. That is the pitch for the segment where reviewer cost is regulated to be highest. It is a segment, not the primary market — see `iheitlager/my-brain` `work/projects/mvl/claude-aviation-software.md` Wave 2b/2e.

### Two facets — Gate AND Assurance Platform

Like MVL itself, `mvl-rust` has two sides:

- **Gate** — the five tool crates as verifiers/lints. Does this code pass? Emit rustc-style diagnostics; block the build if it doesn't. This is the compile-time enforcement side.
- **Assurance Platform** — the same tools running in *reporting* mode, emitting structured JSON evidence: which obligations discharged at which layer, which tests ran, MC/DC coverage, aggregated assurance case. This is what Kanz calls "evidence review" ([`kanz-2026-evidence-review.md`](https://github.com/iheitlager/my-brain/blob/main/work/projects/mvl/references/kanz-2026-evidence-review.md)) and what DO-178C SOI audits consume.

The `cargo mvl` meta-command surfaces both facets:

| Subcommand | Facet | What it does |
|---|---|---|
| `cargo mvl check` | Gate | Runs all installed tools; blocks build on failure |
| `cargo mvl prove` | Assurance | Runs `rust-refine`, emits obligation-trace JSON (layer per obligation) |
| `cargo mvl test` | Assurance | Runs `cargo test`; emits structured test-result JSON |
| `cargo mvl mcdc` | Assurance | Emits MC/DC coverage report via `cargo llvm-cov` MC/DC mode |
| `cargo mvl coverage` | Assurance | Emits line + branch coverage via `cargo llvm-cov` |
| `cargo mvl assurance` | Assurance | Aggregates the above into a claim → argument → evidence tree |

Losing the assurance surface would make `mvl-rust` just another Rust lint suite. Keeping it makes `mvl-rust` the tooling side of the evidence-review argument, mirroring what the MVL playground exposes visually.

## Main structure

### Workspace layout

```
mvl-rust/
├── crates/
│   ├── mvl-rust-core/       shared: attribute grammar, AST walker, solver bindings, diagnostics
│   ├── rust-limit/          qualified-subset linter (Clippy-flavored, no attribute)
│   ├── rust-total/          #[total]  — totality attribute + verifier
│   ├── rust-refine/         #[requires]/#[ensures] — refinement obligations + L1–L5 dispatch
│   ├── rust-effect/         #[effect(list)] — effect algebra attribute
│   ├── rust-ifc/            #[label(l)] — information flow labels + declassification
│   └── cargo-mvl/           cargo subcommand — one entry point for all five tools
├── examples/                real Rust code showing each attribute in isolation and combined
├── docs/                    hand-written prose (concept guides, integration recipes); per-crate API docs on docs.rs
├── Cargo.toml               workspace root
└── README.md
```

Each `crates/rust-*` and `crates/cargo-mvl` publishes independently to `crates.io`. All depend on `mvl-rust-core`.

### Phased delivery

| Phase | Tools shipped | External dependency | Ships when |
|---|---|---|---|
| **1** | `mvl-rust-core` scaffold, `rust-limit` | none | linter enforces the qualified subset on a real Rust crate |
| **2** | `rust-total` + `cargo-mvl` meta-command | none | totality-verified real code compiles, cargo-mvl runs both tools |
| **3** | `rust-refine` | MVL solver integration (see below) | refinement obligations discharged through L1–L5 on a real Rust crate |
| **4** | `rust-effect` | none (needs effect algebra design landed in `mvl-spec`) | effect propagation checks pass end-to-end |
| **5** | `rust-ifc` | none (needs IFC design landed in `mvl-spec`) | Denning-lattice violations rejected end-to-end |
| **6** *(optional)* | Ferrocene qualified-subset compatibility layer | Ferrocene toolchain in CI | `mvl-rust` runs against Ferrocene on `rustup component` install |

### Solver integration with `mvl-lang/mvl`

`rust-refine` (Phase 3) needs the L1–L5 obligation dispatcher — the same one the MVL compiler uses. Three integration options:

1. **Shell out** to `mvl solve --json` — simplest, IPC cost per obligation, requires the compiler binary on PATH. Best for a first cut.
2. **Link as a Rust library** — the MVL compiler exposes its solver as a `libmvl_solver` crate. Fastest, but requires refactoring the compiler.
3. **Reimplement** in `mvl-rust-core` — self-contained, but duplicates code and risks drift.

**Decided: option (3), reimplement.** Options (1) and (2) were rejected because sharing an implementation is not independent verification — see ADR-0001 §4 for the reasoning and ADR-0005 for what replaced them.

### Attribute shape reference

The attributes MVL-Rust introduces:

```rust
// Refinement type on parameter and return
#[mvl::requires(x >= 0 && x < 100)]
#[mvl::ensures(result >= 0)]
fn abs(x: i32) -> i32 { ... }

// Effect declaration
#[effect(Console, Time)]
fn log_now(msg: &str) { ... }

// Totality guarantee
#[mvl::total]
fn signum(n: i32) -> i32 {
    if n > 0 { 1 } else if n < 0 { -1 } else { 0 }
}

// Information flow: the label lives in the type (spec 004)
fn ingest(raw: String) -> Tainted<String> { Tainted::new(raw) }

// Declassification is a declared exception, not a separate attribute
#[mvl::relabel(from = "Tainted", to = "_", audit)]
fn trust<T>(value: Tainted<T>, tag: &'static str) -> T { value.into_inner() }
```

Exact grammar of predicate DSLs lands per-crate; `mvl-rust-core` provides the shared parser.

### Dependency on `mvl-lang/mvl-spec`

`mvl-rust` tracks a specific version of the MVL spec (`mvl-spec/VERSION`). Every attribute macro's semantics MUST match the corresponding MVL construct at that spec version. Drift is caught by `mvl-spec/tools/check-versions.py` (invoke with a flag pointing at a local `mvl-rust` checkout).

## Detailed specifications

This document covers the workspace's architecture and cross-cutting concerns. Per-tool behaviour lives in its own spec, in dependency order:

| Spec | Covers | ADR |
|---|---|---|
| [002](../002-qualified-subset/spec.md) | The qualified subset — `rust-limit` | ADR-0002 |
| [003](../003-function-contracts/spec.md) | Function contracts — `total`, `effect` | ADR-0003 |
| [004](../004-information-flow/spec.md) | Information flow via types — `ifc` | ADR-0004 |
| [005](../005-refinement-obligations/spec.md) | Refinement obligations and Γ | ADR-0005 |
| [006](../006-layered-solver/spec.md) | The layered solver, L1–L5 | ADR-0006 §1–3 |
| [007](../007-runtime-enforcement/spec.md) | Runtime enforcement of residuals | ADR-0006 §4–5 |
| [008](../008-reporting/spec.md) | Diagnostics and assurance reporting | ADR-0006 §5 |

## Requirements

### Requirement 1: Verification attaches to unmodified Rust via attributes [MUST]

A file under these tools MUST be a Rust file that `rustc` compiles unchanged. There MUST be no dialect, fork, preprocessor, or new syntax.

Verification information MUST be carried by attribute macros in the `mvl::` namespace, parsed centrally and recognised by **last path segment**, so `#[mvl::requires]`, `#[requires]` and `#[alias::requires]` all resolve. An attribute the workspace does not own MUST be skipped rather than rejected.

**Implementation:** `crates/mvl-rust-core/src/attrs.rs`

#### Scenario: A third-party attribute is skipped, not rejected

- GIVEN a function carrying `#[derive(Debug)]` alongside `#[mvl::requires(x > 0)]`
- WHEN attributes are parsed
- THEN the unowned attribute MUST be skipped
- AND the `mvl::` attribute MUST still be recognised

**Tests:** `crates/mvl-rust-core/tests/attrs.rs::unrecognized_attribute_returns_none`

#### Scenario: A malformed predicate is a parse error, not silence

- GIVEN an `mvl::` attribute whose argument tokens do not match its grammar
- WHEN parsing runs
- THEN an error MUST be returned rather than the attribute being ignored

**Tests:** `crates/mvl-rust-core/tests/attrs.rs::malformed_predicate_returns_parse_error`

### Requirement 2: Verification is out-of-band from compilation [MUST]

Annotated code MUST compile and run identically whether or not any verification tool has been run. The tools MUST read source with `syn` and report diagnostics; nothing they do MUST reach `rustc`.

The facade crate MUST be a convenience rather than a requirement for compilation.

> **Amended by spec 007 Requirement 3.** Once contract attributes enforce their predicates at runtime, this requirement no longer holds for `requires`/`ensures` — enforcement is the deliberate exception, and the tools remain out-of-band.

**Implementation:** `crates/mvl-macros/src/lib.rs`

#### Scenario: Annotations do not alter behaviour

- GIVEN a function annotated with `#[mvl::requires]` and `#[mvl::ensures]`
- WHEN the program runs
- THEN its observable behaviour MUST be identical to the unannotated function

**Tests:** `crates/mvl/tests/passthrough.rs::attributes_are_pass_through_and_dont_alter_behavior`

### Requirement 3: One dispatcher, five tools, no shared analysis state [MUST]

`cargo mvl check` MUST run the five tools in the order `limit → total → refine → effect → ifc` as in-process library calls over explicit file paths. Each tool MUST also be independently invocable as its own `cargo` subcommand.

Shared infrastructure MUST be limited to the attribute grammar, the diagnostic type, and the solver. There MUST be no shared program representation and tools MUST NOT exchange results.

**Implementation:** `crates/cargo-mvl/src/check.rs`, `crates/cargo-mvl/src/main.rs`

#### Scenario: Each tool is independently invocable

- GIVEN an installed workspace
- WHEN `cargo mvl-limit <FILE>` is invoked directly
- THEN only that tool MUST run, and its exit code MUST reflect only its own findings

**Tests:** `crates/cargo-mvl/tests/check.rs::check_single_runs_only_the_named_tool`, `::check_single_returns_none_for_an_unknown_tool`

### Requirement 4: No dependency on `mvl-lang/mvl` [MUST]

There MUST be no dependency — build-time, runtime, or logical — on `mvl-lang/mvl`. Its source MAY be read as a design reference and its test fixtures MAY be ported as cross-validation corpora, but neither constitutes a dependency.

Rationale: cross-validation is the mechanism by which a divergence is caught, and it only works if the two implementations are actually separate. Where they disagree, the divergence MUST be asserted by a test rather than smoothed over.

**Implementation:** `crates/mvl-rust-core/src/solver/native.rs`

#### Scenario: A ported upstream fixture closes without the upstream solver

- GIVEN a fixture ported from the reference implementation's SMT-layer corpus
- WHEN the obligation is discharged by this workspace's native solver
- THEN it MUST close without invoking any external solver
- AND the divergence in discharge layer MUST be asserted rather than hidden

**Tests:** `crates/rust-refine/tests/call_sites.rs::chained_hypotheses_close_at_l4_without_an_smt_solver`

### Requirement 5: Greenfield only — no grandfathering, no exceptions [MUST]

The tools target code written to be verified. There MUST NOT be a compatibility mode, a warn-instead-of-error tier for unverifiable constructs, a per-crate opt-out, or an `#[allow]`-shaped escape hatch for any verification attribute.

Where a construct cannot be verified, the resolution MUST be a change to the code, not an exception in the tool.

Precision MAY be traded for soundness; the reverse MUST NOT occur. A construct the tools cannot model MUST yield *no claim* rather than a weakened one.

**Implementation:** `crates/rust-limit/src/lints/mod.rs`

#### Scenario: An unmodellable construct yields no claim

- GIVEN a call the tools cannot resolve
- WHEN analysis runs
- THEN no obligation MUST be produced and no diagnostic MUST be emitted in either direction

**Tests:** `crates/rust-refine/tests/call_sites.rs::a_call_to_an_unresolvable_function_produces_no_obligation`

### Requirement 6: Each crate publishes independently [MUST]

Every crate MUST be publishable to crates.io on its own, with the workspace version inherited from the root manifest. CI MUST build and test across stable and the declared MSRV.

**Implementation:** `Cargo.toml`, `.github/workflows/ci.yml`

#### Scenario: CI covers stable and MSRV

- GIVEN a pull request
- WHEN CI runs
- THEN the workspace MUST build and test green on both stable and the declared MSRV

### Requirement 7: Attribute semantics track a pinned `mvl-spec` version [MUST]

Each attribute's semantics MUST match the corresponding MVL construct at the tracked spec version. Drift MUST be detectable by an automated check rather than by review.

**Implementation:** `Cargo.toml` — not yet implemented

#### Scenario: Divergence from the tracked spec version is detected

- GIVEN a local checkout of `mvl-spec` at a different version
- WHEN the version-alignment check runs
- THEN the mismatch MUST be reported

### Requirement 8: Every tool ships a compliant and a violating example [MUST]

Each tool MUST ship a paired example: one crate that passes cleanly and one that is rejected. Both MUST be exercised in CI, and the violating example MUST be asserted to fail rather than merely run.

**Implementation:** `examples/`, `Makefile`

#### Scenario: The violating example is asserted to fail

- GIVEN the violating example for any tool
- WHEN `make examples` runs
- THEN the tool MUST exit non-zero
- AND the target MUST treat a zero exit as a failure of the example itself

### Requirement 9: Assurance subcommands emit structured evidence [MUST]

`cargo mvl` MUST provide evidence-emitting subcommands emitting machine-readable evidence: obligation traces, test results, and an aggregated assurance tree. Coverage and MC/DC reporting MUST be delegated to `cargo llvm-cov` rather than reimplemented.

**Implementation:** `crates/cargo-mvl/src/prove.rs`, `crates/cargo-mvl/src/test.rs`

#### Scenario: `cargo mvl prove` emits per-obligation layers

- GIVEN a crate using `#[mvl::requires]` and `#[mvl::ensures]`
- WHEN `cargo mvl prove` runs
- THEN the output MUST record, per obligation, the layer that discharged it or that it remains undischarged

**Tests:** `crates/cargo-mvl/tests/subcommands.rs::prove_emits_a_prove_section_with_no_check_or_test`

### Requirement 10: Ferrocene compatibility [SHOULD]

The workspace SHOULD build and test green under the Ferrocene toolchain, so that certified-domain adoption can ride Ferrocene's existing qualification rather than requiring a new one.

**Implementation:** `.github/workflows/ci.yml` — not yet implemented

#### Scenario: The suite runs green under Ferrocene

- GIVEN the Ferrocene toolchain is available to CI
- WHEN the full suite runs under it
- THEN it MUST pass without source changes

## Design decisions locked

| Decision | Value | Alternative considered |
|---|---|---|
| Repo layout | Cargo workspace, one crate per tool + `mvl-rust-core` + `cargo-mvl` | Five separate repos |
| Attribute style | Rustc proc-macros with `syn` | Function-like macros; `rustc_ast` internals |
| Solver integration | Reimplemented natively in `mvl-rust-core`, no dependency on `mvl-lang/mvl` (ADR-0001 §4) | Shell out to `mvl solve --json`; link its solver as a library — both rejected as not independent verification |
| Diagnostic emission | Rustc `Diagnostic` API via `proc_macro2::Span` | Custom formatter |
| CI toolchains | Stable + MSRV; Ferrocene added when accessible | Nightly-only |
| Publish target | crates.io + docs.rs | Internal registry |
| Publish sequence | rust-limit → rust-total → rust-refine → rust-effect → rust-ifc | Big-bang release |
| Assurance emission | Structured JSON per the shared schema (spec 008) | Free-form text output |
| MC/DC coverage tooling | `cargo llvm-cov --mcdc` | Roll our own instrumentation |
| Coverage tooling | `cargo llvm-cov` | `tarpaulin` |

Recorded in `adr/0001-annotation-driven-verification.md` (attribute model and the greenfield rule) and `adr/0002-qualified-subset.md` (the `rust-limit` subset). A Ferrocene-toolchain ADR remains unwritten — tracked as #12.

## Ideas (not yet requirements)

The following are noted for future consideration and are NOT part of the current spec:

- **Cross-crate solver caching.** If a workspace uses `rust-refine` on hundreds of functions, memoise discharge results between builds. Solver invocations are the slowest part.
- **`#[refine]` on struct fields.** Currently only function signatures. Struct-field refinements would need MIR-level tracking to catch violations at construction sites.
- **`#[axiom]` for user-declared lemmas.** Explicit assumed facts the solver can use, with runtime checks in debug builds. Design overlap with `#[refine]`.
- **rust-analyzer plugin.** Attributes as inline diagnostics without requiring `cargo build`. Faster feedback loop; needs LSP-side integration.
- **Interop with Kani / Creusot / Prusti / Verus.** Adjacent tools with overlapping goals. Some ADR-worthy questions about whether to call out to them, feed into them, or treat them as competitors. Recommendation: friendly coexistence, integration recipe in `docs/`.
- **`no_std` support.** `rust-limit` and `rust-total` could work in `no_std`; `rust-refine` needs runtime for uncloseable obligations. Design decision deferred.
- **Cross-crate build performance.** Five proc-macros in one workspace can slow builds. Investigate compile-time incrementality, lazy attribute expansion, or a shared query engine. Not urgent until users complain.
- **VSCode / Zed / RustRover extension.** UI polish on top of rust-analyzer plugin.
- **A verified stdlib subset.** Wrap `std::collections::HashMap` etc. with refinement types on their APIs (`insert` guarantees `contains_key` post). Would ship as a separate crate `mvl-rust-std`.

## Cross-refs

- `mvl-lang/mvl-spec` Wave 2b (in `iheitlager/my-brain` `work/projects/mvl/claude-aviation-software.md`): the "Rust bolt-on adoption path" argument.
- `iheitlager/my-brain` `work/projects/mvl/paper6-verified-rust.md`: the Paper 6 sketch this workspace realises.
- `iheitlager/my-brain` `work/projects/mvl/rust-limit-linter.md`: the `rust-limit` design.
- `mvl-lang/mvl`: the reference compiler. Solver integration lands here (Phase 3).
- `mvl-lang/mvl-spec/tools/check-versions.py`: extended to recognise `mvl-rust` as a versioned artefact.
- Ferrocene (Ferrous Systems): the DO-178C-qualified Rust toolchain that Requirement 11 targets.
