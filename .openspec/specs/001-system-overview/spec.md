---
domain: workspace
version: 0.1.2
status: draft
date: 2026-07-22
---

# 001 — System Overview

The `mvl-rust` workspace is a second implementation of MVL's language guarantees, expressed as Rust attribute macros and lint passes. It exists so that Rust codebases can adopt MVL's guarantees incrementally, so that MVL's semantics are validated against a completely independent parser and typechecker, and so that certified-domain adoption can ride Ferrocene's existing DO-178C qualification rather than requiring a new qualification for MVL itself.

## Vision

**MVL's ideas belong in Rust too.** Refinement types, effect tracking, totality, and information flow control are not MVL-specific — they are properties any statically-typed language can express. Proving they work in Rust is what turns MVL from "a project's opinion" into "a family of language guarantees that happens to have two implementations."

Concretely: an engineering team on Rust adds `#[refine(x > 0)]` to a new function and the same discharge machinery MVL uses closes the obligation. If the team eventually migrates to MVL, the semantics carry over — only the syntax differs. If the team stays on Rust, they still have every guarantee MVL was pitched to deliver.

**The LLM angle is load-bearing.** LLMs already generate excellent Rust and know the attribute idiom. Getting an LLM to produce `#[refine(x > 0)] fn f(x: i32) -> i32 { x + 1 }` is a much smaller ask than getting it to produce syntactically-correct MVL. `mvl-rust` is where the LLM-native verified-code story lands soonest against the largest audience.

**The Ferrocene angle is opportunistic, not central.** Ferrocene qualifies Rust for DO-178C already. `mvl-rust` on top of Ferrocene delivers verified-Rust-for-certified-domains without qualifying MVL itself. That is the pitch for the segment where reviewer cost is regulated to be highest. It is a segment, not the primary market — see `iheitlager/my-brain` `work/projects/mvl/claude-aviation-software.md` Wave 2b/2e.

## Main structure

### Workspace layout

```
mvl-rust/
├── crates/
│   ├── mvl-rust-core/       shared: attribute grammar, AST walker, solver bindings, diagnostics
│   ├── rust-limit/          qualified-subset linter (Clippy-flavored, no attribute)
│   ├── rust-total/          #[total]  — totality attribute + verifier
│   ├── rust-refine/         #[refine(pred)] — refinement type attribute + L1–L5 dispatch
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

**Recommendation: start with (1), migrate to (2) when the compiler exposes a solver crate.** ADR-0001 will lock this decision before `rust-refine` implementation starts.

### Attribute shape reference

The attributes MVL-Rust introduces:

```rust
// Refinement type on parameter and return
#[refine(x >= 0 && x < 100)]
#[refine_ret(y => y >= 0)]
fn abs(x: i32) -> i32 { ... }

// Effect declaration
#[effect(Console, Time)]
fn log_now(msg: &str) { ... }

// Totality guarantee
#[total]
fn signum(n: i32) -> i32 {
    if n > 0 { 1 } else if n < 0 { -1 } else { 0 }
}

// Information flow label
#[label(Secret)]
type Password = String;

// Declassification (explicit)
#[declassify]
fn hash_for_logging(pw: &Password) -> String { ... }
```

Exact grammar of predicate DSLs lands per-crate; `mvl-rust-core` provides the shared parser.

### Dependency on `mvl-lang/mvl-spec`

`mvl-rust` tracks a specific version of the MVL spec (`mvl-spec/VERSION`). Every attribute macro's semantics MUST match the corresponding MVL construct at that spec version. Drift is caught by `mvl-spec/tools/check-versions.py` (invoke with a flag pointing at a local `mvl-rust` checkout).

## Requirements

### Requirement 1: The qualified subset [MUST]

`rust-limit` MUST provide a lint pass that identifies uses of Rust language features outside the qualified subset defined in `mvl-spec` Wave 2b (see [rust-limit-linter design](https://github.com/iheitlager/my-brain/blob/main/work/projects/mvl/rust-limit-linter.md)). The lint pass MUST run as a `cargo` subcommand and MUST emit diagnostics using rustc's standard span-annotated error format.

**Implementation:** `crates/rust-limit/src/lints/`

**Tests:** `crates/rust-limit/tests/qualified_subset.rs`

#### Scenario: Forbidden construct rejected

- GIVEN a Rust file containing an `unsafe` block
- WHEN `cargo mvl-limit` runs
- THEN the linter MUST emit a diagnostic naming `unsafe` as outside the qualified subset, with the exact span of the offending block

#### Scenario: Whitelisted construct accepted

- GIVEN a Rust file using only permitted constructs (safe references, `Result`, `Option`, non-generic lifetimes, no macros beyond an allowlist)
- WHEN `cargo mvl-limit` runs
- THEN the linter MUST exit with status 0 and no diagnostics

### Requirement 2: Totality attribute [MUST]

`rust-total` MUST provide `#[total]` on `fn` declarations and MUST verify that: (a) every `match` on an enum is exhaustive, (b) no panicking construct is reachable (`unwrap`, `expect`, indexing without bounds proof, arithmetic that can overflow in `#[deny(overflow)]` context), (c) recursion is bounded by a `#[decreases(measure)]` annotation the verifier can prove strictly decreases.

**Implementation:** `crates/rust-total/src/verifier.rs`

**Tests:** `crates/rust-total/tests/totality.rs`

#### Scenario: Non-exhaustive match rejected

- GIVEN `#[total] fn f(x: Option<i32>) -> i32 { match x { Some(n) => n } }`
- WHEN the crate compiles
- THEN compilation MUST fail with "non-exhaustive match under `#[total]`: variant `None` not handled"

#### Scenario: Terminating recursion accepted

- GIVEN a `#[total]` function with `#[decreases(len - i)]` where the verifier can prove strict decrease
- WHEN the crate compiles
- THEN compilation MUST succeed

#### Scenario: Missing decreases annotation rejected

- GIVEN a recursive `#[total]` function without `#[decreases(...)]`
- WHEN the crate compiles
- THEN compilation MUST fail with a diagnostic pointing to the recursion site and requesting the annotation

### Requirement 3: Refinement attribute [MUST]

`rust-refine` MUST provide `#[refine(pred)]` on function parameters and return types, and MUST discharge the resulting obligations through the same layered dispatch (L1 trivial → L2 intervals → L3 path enumeration → L4 Cooper QE → L5 SMT → runtime) that the MVL compiler uses. Obligations MUST be attributable to a specific layer in the diagnostic output — this is the load-bearing UX for the certified-domain pitch.

**Implementation:** `crates/rust-refine/src/dispatch.rs`, `crates/rust-refine/src/attr_parser.rs`

**Tests:** `crates/rust-refine/tests/refinements.rs`

**Blocked on:** ADR-0001 (solver integration story).

#### Scenario: Simple bound proven at L2

- GIVEN `#[refine(x >= 0 && x < 100)] fn f(x: i32) -> #[refine(y >= 0)] i32 { x }`
- WHEN the crate compiles
- THEN compilation MUST succeed AND the verifier MUST report the discharge layer as L2 (intervals)

#### Scenario: Uncloseable obligation surfaces as runtime check

- GIVEN a refinement over an opaque function output
- WHEN the crate compiles
- THEN compilation MUST succeed AND `rust-refine` MUST emit a runtime assertion at the site with attribution to the unclosed obligation ("could not be discharged by L1–L5; runtime check inserted")

#### Scenario: Genuine violation rejected

- GIVEN `#[refine(x < 0)] fn f(x: i32) { ... }` called with `f(5)`
- WHEN the crate compiles
- THEN compilation MUST fail with a diagnostic including the counterexample from the solver (`x = 5` violates `x < 0`)

### Requirement 4: Effect attribute [SHOULD]

`rust-effect` SHOULD provide `#[effect(list)]` on function declarations declaring the effects the function performs (`Console`, `Time`, `File`, `Network`, `Random`, `Panic`, `Nondet`, `Actor`, and user-declared effects). Effect tracking MUST be structural: a caller of an effectful function inherits its effects unless they are handled.

**Implementation:** `crates/rust-effect/src/`

**Tests:** `crates/rust-effect/tests/effect_propagation.rs`

#### Scenario: Effect propagation

- GIVEN `#[effect(Console)] fn print_line(s: &str)` and a caller `fn wrap(s: &str) { print_line(s) }`
- WHEN the crate compiles
- THEN compilation MUST fail with "caller `wrap` lacks declared effect `Console`; add `#[effect(Console)]` or handle the effect"

#### Scenario: Pure function forbidden from calling effectful function

- GIVEN `#[effect()] fn pure_computation() { print_line("side effect") }`
- WHEN the crate compiles
- THEN compilation MUST fail

### Requirement 5: Information flow attribute [SHOULD]

`rust-ifc` SHOULD provide `#[label(l)]` on type declarations and MUST enforce a Denning-lattice information flow discipline (`Public ≤ Tainted ≤ Secret`; declassification via functions annotated `#[declassify]`).

**Implementation:** `crates/rust-ifc/src/`

**Tests:** `crates/rust-ifc/tests/lattice.rs`

#### Scenario: Cross-label flow rejected

- GIVEN a `#[label(Secret)] String` value flowing into a `#[label(Public)] String` binding without a declassifier
- WHEN the crate compiles
- THEN compilation MUST fail with an IFC violation naming the source and sink

#### Scenario: Explicit declassification accepted

- GIVEN a `#[declassify]`-annotated function producing a `Public` from a `Secret` argument
- WHEN the crate compiles
- THEN compilation MUST succeed

### Requirement 6: `cargo mvl` meta-command [MUST]

The workspace MUST provide a `cargo mvl` subcommand (`crates/cargo-mvl`) that invokes the installed tool crates as a single pipeline. Subcommands: `cargo mvl check` (runs all installed tools), `cargo mvl limit` (linter only), `cargo mvl total`, `cargo mvl refine`, `cargo mvl effect`, `cargo mvl ifc`. Diagnostics from all tools MUST be rendered through a unified formatter so that users see one output stream, not five.

**Implementation:** `crates/cargo-mvl/src/main.rs`

**Tests:** `crates/cargo-mvl/tests/pipeline.rs`

#### Scenario: Full check pipeline

- GIVEN a crate using `#[total]`, `#[refine(...)]`, and `#[effect(...)]` attributes
- WHEN a user runs `cargo mvl check`
- THEN all installed tool crates MUST run in sequence AND diagnostics MUST be rendered in a single output stream with per-tool origin markers

### Requirement 7: Diagnostic quality [MUST]

All tool crates MUST emit diagnostics that: (a) use `proc_macro2::Span` for accurate source locations, (b) render through rustc's standard `Diagnostic` API so `cargo` displays them consistent with normal compiler errors, (c) include the offending attribute in the diagnostic caret, (d) suggest the concrete fix where mechanical (`#[decreases(...)]`, `#[declassify]`, etc.). Rust's compiler diagnostics are famously good; users will judge these tools against that bar.

**Implementation:** `crates/mvl-rust-core/src/diagnostics.rs`

**Tests:** `crates/mvl-rust-core/tests/diagnostics_ui.rs` (snapshot tests using `trybuild` or `expect-test`)

#### Scenario: Diagnostic looks like a rustc error

- GIVEN a `#[total]` violation
- WHEN the crate compiles
- THEN the emitted diagnostic MUST render with the same source-caret formatting as a rustc error AND MUST include the attribute span AND MUST propose a concrete fix

### Requirement 8: Independent publishing [MUST]

Each of the six tool crates (five tools plus `cargo-mvl`) MUST publish independently to crates.io. Users MUST be able to install any subset of them. `mvl-rust-core` MUST publish as a library crate; the tool crates MUST NOT re-export its internals as part of their public API.

**Implementation:** `Cargo.toml` (workspace + each crate's `Cargo.toml`), `.github/workflows/publish-*.yml`

**Tests:** `.github/workflows/ci.yml` (build + test matrix per crate)

#### Scenario: Independent install

- GIVEN a Rust project that installs only `rust-total`
- WHEN `cargo add rust-total` runs
- THEN the project MUST NOT be forced to pull `rust-refine`, `rust-effect`, `rust-ifc`, `rust-limit`, or `cargo-mvl`

### Requirement 9: Version alignment with `mvl-spec` [MUST]

The workspace's version MUST equal `mvl-spec/VERSION` at release checkpoints. Alignment MUST be verifiable via `mvl-spec/tools/check-versions.py` with a flag pointing at a local `mvl-rust` checkout (implementation: extend the script to accept `--mvl-rust-dir`, mirroring `--tree-sitter-dir`).

**Implementation:** `Cargo.toml` (`[workspace.package] version = "..."`), inherited by each crate

**Tests:** CI check invoking `check-versions.py`

#### Scenario: Aligned at release

- GIVEN `mvl-spec/VERSION` at `0.1.2` and `mvl-rust/Cargo.toml` workspace version at `0.1.2`
- WHEN `check-versions.py --target 0.1.2 --mvl-rust-dir <mvl-rust>` runs
- THEN it MUST exit 0

### Requirement 10: Documentation [MUST]

Each crate MUST publish API documentation to `docs.rs` automatically on release (default `cargo publish` behaviour). The workspace repo MUST host prose-style concept guides under `docs/` covering: (a) overview / when to use which tool, (b) integration recipes (`mvl-rust` in an existing Rust codebase, `mvl-rust` alongside Kani / Creusot / Prusti), (c) FAQ. Prose docs MUST be published to `mvl-lang.org/rust/` or similar path via CI.

**Implementation:** `docs/` directory, `.github/workflows/publish-docs.yml`

**Tests:** doctests in every crate MUST run under `cargo test`

#### Scenario: docs.rs renders correctly on publish

- GIVEN a `cargo publish` of a tool crate
- WHEN docs.rs finishes building
- THEN the rendered docs page MUST include the top-level module doc AND the attribute's usage examples

### Requirement 11: Ferrocene qualified-subset compatibility [SHOULD]

`mvl-rust` SHOULD compile and run under the Ferrocene toolchain (Rust qualified for DO-178C, IEC 62304, ISO 26262, IEC 61508). CI SHOULD include a Ferrocene target alongside stable Rust and MSRV. Where the qualified subset of Ferrocene forbids a construct that `mvl-rust` uses internally, that construct MUST be replaced or gated.

**Implementation:** `.github/workflows/ferrocene.yml`, adjustments in `mvl-rust-core` where flagged

**Tests:** full test suite runs green under Ferrocene

**Blocked on:** access to a Ferrocene toolchain in CI (may need a private Ferrous Systems partnership); not a Phase 1 requirement

#### Scenario: Ferrocene-hosted CI green

- GIVEN a workspace commit
- WHEN the Ferrocene CI job runs
- THEN it MUST build and test all crates without errors

### Requirement 12: Examples and integration tests [MUST]

The `examples/` directory MUST contain at least one real Rust crate per attribute demonstrating: (a) the attribute in isolation, (b) the attribute in combination with the others, (c) explicit failure cases with expected diagnostics (using `trybuild`).

**Implementation:** `examples/rust-limit-demo/`, `examples/rust-total-demo/`, etc.

**Tests:** examples MUST be verified in CI

#### Scenario: All examples compile and demonstrate the intended behaviour

- GIVEN the examples suite
- WHEN CI runs
- THEN each example MUST either compile cleanly (positive cases) or fail with the expected diagnostic (negative cases via `trybuild`)

## Design decisions locked

| Decision | Value | Alternative considered |
|---|---|---|
| Repo layout | Cargo workspace, one crate per tool + `mvl-rust-core` + `cargo-mvl` | Five separate repos |
| Attribute style | Rustc proc-macros with `syn` | Function-like macros; `rustc_ast` internals |
| Solver integration (Phase 3) | Shell out to `mvl solve --json` initially; migrate to library link | Reimplementation in `mvl-rust-core` |
| Diagnostic emission | Rustc `Diagnostic` API via `proc_macro2::Span` | Custom formatter |
| CI toolchains | Stable + MSRV; Ferrocene added when accessible | Nightly-only |
| Publish target | crates.io + docs.rs | Internal registry |
| Publish sequence | rust-limit → rust-total → rust-refine → rust-effect → rust-ifc | Big-bang release |

Downstream ADRs (`0001-solver-integration.md`, `0002-attribute-grammar.md`, `0003-ferrocene-toolchain.md`) will document these once the initial code is landing.

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
