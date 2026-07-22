---
domain: workspace
version: 0.1.0
status: draft
date: 2026-07-22
---

# 001 — System Overview

The `mvl-rust` workspace is a second implementation of MVL's language guarantees, expressed as Rust attribute macros and lint passes. It exists so that Rust codebases can adopt MVL's guarantees incrementally, so that MVL's semantics are validated against a completely independent parser and typechecker, and so that certified-domain adoption can ride Ferrocene's existing DO-178C qualification.

## Vision

**MVL's ideas belong in Rust too.** Refinement types, effect tracking, totality, and information flow control are not MVL-specific — they are properties any statically-typed language can express. Proving they work in Rust is what turns MVL from "a project's opinion" into "a family of language guarantees that happens to have two implementations."

Concretely: an engineering team working on a Rust codebase should be able to add `#[refine(x > 0)]` to a new function and have the same discharge machinery MVL's compiler uses close the obligation. If the same team eventually wants to migrate to MVL, the semantics are already the same — only the syntax differs. If the team stays on Rust, they still have every guarantee MVL was pitched to deliver.

The LLM angle is load-bearing. LLMs already generate excellent Rust and know the attribute idiom. Getting an LLM to produce `#[refine(x > 0)] fn f(x: i32) -> i32 { x + 1 }` is a much smaller ask than getting it to produce syntactically-correct MVL. `mvl-rust` is where the LLM-native verified-code story lands soonest against the largest audience.

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
│   └── rust-ifc/            #[label(l)] — information flow labels + declassification
├── examples/                real Rust code showing each attribute in isolation and combined
├── docs/                    published as GitHub Pages or on mvl-lang.org
├── Cargo.toml               workspace root
└── README.md
```

Each `crates/rust-*` publishes independently to `crates.io`. All depend on `mvl-rust-core` for shared infrastructure.

### Dependency on `mvl-lang/mvl-spec`

`mvl-rust` tracks a specific version of the MVL spec (`mvl-spec/VERSION`). Every attribute macro's semantics MUST match the corresponding MVL construct at that spec version. Drift is caught by `mvl-spec/tools/check-versions.py`, which recognises `mvl-rust` as a versioned artefact.

### Dependency on `mvl-lang/mvl` (the compiler)

The `rust-refine` verifier reuses the MVL compiler's L1–L5 dispatcher for obligation discharge. Concrete integration is TBD (candidates: shell out to `mvl solve`, link the solver as a Rust library, replicate the dispatcher in `mvl-rust-core`). The choice is a downstream ADR.

## Requirements

### Requirement 1: The qualified subset [MUST]

`rust-limit` MUST provide a lint pass that identifies uses of Rust language features outside the qualified subset defined in `mvl-spec` Wave 2b (see [rust-limit-linter design](https://github.com/iheitlager/my-brain/blob/main/work/projects/mvl/rust-limit-linter.md)). The lint pass MUST run as `cargo clippy` extension or standalone binary.

**Implementation:** `crates/rust-limit/src/lints/`

**Tests:** `crates/rust-limit/tests/qualified_subset.rs`

#### Scenario: Forbidden construct rejected

- GIVEN a Rust file using `unsafe` blocks
- WHEN `cargo mvl-limit` runs
- THEN the linter MUST emit a diagnostic naming `unsafe` as outside the qualified subset

#### Scenario: Whitelisted construct accepted

- GIVEN a Rust file using only permitted constructs (safe references, `Result`, `Option`, non-recursive lifetimes)
- WHEN `cargo mvl-limit` runs
- THEN the linter MUST exit with status 0 and no diagnostics

### Requirement 2: Totality attribute [MUST]

`rust-total` MUST provide `#[total]` on `fn` declarations and MUST verify that:
- Every match on an enum is exhaustive
- No panicking construct is reachable (`unwrap`, `expect`, indexing without bounds proof, arithmetic that can overflow in `#[deny(overflow)]` context)
- Recursion is bounded by a decreasing measure

**Implementation:** `crates/rust-total/src/verifier.rs`

**Tests:** `crates/rust-total/tests/totality.rs`

#### Scenario: Non-exhaustive match rejected

- GIVEN `#[total] fn f(x: Option<i32>) -> i32 { match x { Some(n) => n } }`
- WHEN the crate compiles
- THEN the compilation MUST fail with "non-exhaustive match under #[total]: variant `None` not handled"

#### Scenario: Terminating recursion accepted

- GIVEN a `#[total]` function that recurses on a `decreases` measure the verifier can prove strictly decreases
- WHEN the crate compiles
- THEN the compilation MUST succeed

### Requirement 3: Refinement attribute [MUST]

`rust-refine` MUST provide `#[refine(pred)]` on function parameters and return types, and MUST discharge the resulting obligations through the same layered dispatch (L1 trivial → L2 intervals → L3 path enumeration → L4 Cooper QE → L5 SMT → runtime) that the MVL compiler uses.

**Implementation:** `crates/rust-refine/src/dispatch.rs`

**Tests:** `crates/rust-refine/tests/refinements.rs`

#### Scenario: Simple bound proven at L2

- GIVEN `#[refine(x >= 0 && x < 100)] fn f(x: i32) -> #[refine(y >= 0)] i32 { x }`
- WHEN the crate compiles
- THEN the compilation MUST succeed AND the verifier MUST report the discharge layer as L2 (intervals)

#### Scenario: Uncloseable obligation surfaces as runtime check

- GIVEN a refinement over an opaque function output
- WHEN the crate compiles
- THEN the compilation MUST succeed AND `rust-refine` MUST emit a runtime assertion at the site with attribution to the unclosed obligation

### Requirement 4: Effect attribute [SHOULD]

`rust-effect` SHOULD provide `#[effect(list)]` on function declarations declaring the effects the function performs. Effect tracking MUST be structural: a caller of an effectful function inherits its effects unless they are handled.

**Implementation:** `crates/rust-effect/src/`

**Tests:** `crates/rust-effect/tests/effect_propagation.rs`

#### Scenario: Effect propagation

- GIVEN `#[effect(Console)] fn print_line(s: &str)` and a caller `fn wrap(s: &str) { print_line(s) }`
- WHEN the crate compiles
- THEN compilation MUST fail with "caller `wrap` lacks declared effect Console"

### Requirement 5: Information flow attribute [SHOULD]

`rust-ifc` SHOULD provide `#[label(l)]` on type declarations and MUST enforce a Denning-lattice information flow discipline (Public ≤ Tainted ≤ Secret; declassification via explicit `declassify` calls).

**Implementation:** `crates/rust-ifc/src/`

**Tests:** `crates/rust-ifc/tests/lattice.rs`

#### Scenario: Cross-label flow rejected

- GIVEN a `#[label(Secret)] String` value flowing into a `#[label(Public)] String` binding
- WHEN the crate compiles
- THEN compilation MUST fail with an IFC violation naming the source and sink

### Requirement 6: Independent publishing [MUST]

Each of the five tool crates MUST publish independently to crates.io. Users MUST be able to install any subset of them. `mvl-rust-core` MUST publish as a library crate; the tool crates MUST NOT re-export its internals.

**Implementation:** `Cargo.toml` (workspace + each crate's `Cargo.toml`), `.github/workflows/publish-*.yml`

**Tests:** `.github/workflows/ci.yml` (build + test matrix per crate)

#### Scenario: Independent install

- GIVEN a Rust project that installs only `rust-total`
- WHEN `cargo add rust-total` runs
- THEN the project MUST NOT be forced to pull `rust-refine`, `rust-effect`, `rust-ifc`, or `rust-limit`

### Requirement 7: Version alignment with `mvl-spec` [MUST]

The workspace's version MUST equal `mvl-spec/VERSION` at release checkpoints. Alignment MUST be verifiable via `mvl-spec/tools/check-versions.py` with `--tree-sitter-dir` replaced by an equivalent flag pointing at a local `mvl-rust` checkout.

**Implementation:** `Cargo.toml` (`[workspace.package] version = "..."`), inherited by each crate

**Tests:** CI check invoking `check-versions.py`

#### Scenario: Aligned at release

- GIVEN `mvl-spec/VERSION` at `0.1.2` and `mvl-rust/Cargo.toml` workspace version at `0.1.2`
- WHEN `check-versions.py --target 0.1.2 --tree-sitter-dir <mvl-rust>` runs
- THEN it MUST exit 0

## Ideas (not yet requirements)

The following are noted for future consideration and are NOT part of the current spec:

- **`cargo mvl` meta-command.** A single `cargo mvl check` that runs all five verifiers in one pass, with a unified diagnostic renderer. Reduces user friction.
- **IDE integration via rust-analyzer.** Attributes exposed as inline diagnostics without requiring `cargo build`. May be a rust-analyzer plugin or a compile-flag driven fast path.
- **Cross-crate solver caching.** If a workspace uses `rust-refine` on hundreds of functions, memoize discharge results between builds. Solver invocations are the slowest part.
- **`#[refine]` on struct fields.** Currently only function signatures. Struct-field refinements would need MIR-level tracking to catch violations at construction sites.
- **`#[axiom]` for user-declared lemmas.** Explicit assumed facts the solver can use, with runtime checks in debug builds. Design overlap with `#[refine]`.
- **Interoperability with existing verification tools.** Kani, Creusot, Prusti — probably not competitors, more like adjacent tools. Some ADR-worthy questions about whether we should call out or feed into them.

## Cross-refs

- `mvl-lang/mvl-spec` Wave 2b (in `claude-aviation-software.md` on iheitlager/my-brain): the "Rust bolt-on adoption path" argument.
- `iheitlager/my-brain` `work/projects/mvl/paper6-verified-rust.md`: the Paper 6 sketch this workspace realises.
- `iheitlager/my-brain` `work/projects/mvl/rust-limit-linter.md`: the `rust-limit` design.
