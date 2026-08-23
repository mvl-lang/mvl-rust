# OpenSpec — MVL for Rust

Specifications, architectural decisions, and design documents for `mvl-rust` — the Rust-side implementation of MVL semantics as five bolt-on tools plus shared infrastructure.

## What this repo is

`mvl-rust` is a Cargo workspace holding a second implementation of MVL's language guarantees, expressed as attribute macros and lint passes on ordinary Rust. It exists so that:

1. MVL's semantics are proven language-independent rather than parser-specific.
2. Engineering teams can adopt MVL's guarantees without rewriting their Rust code — they add attributes on new code and enable a linter on existing code.
3. LLMs that already generate excellent Rust can generate *verified* Rust by producing attributes alongside code.
4. The certified-domain adoption path can ride Ferrocene's DO-178C qualification rather than requiring a new qualification for MVL itself.

The upstream language reference lives in [`mvl-lang/mvl-spec`](https://github.com/mvl-lang/mvl-spec). The reference compiler is [`mvl-lang/mvl`](https://github.com/mvl-lang/mvl). This repo tracks whichever spec version it verifies against; alignment is checked via `mvl-spec/tools/check-versions.py`.

## ISPE Philosophy

Same discipline as `mvl-lang/mvl`:

- **Intent:** GitHub issues (epics + stories)
- **Spec:** `.openspec/specs/` — requirements with scenarios, implementation links, test links
- **Program:** `crates/*/src/` — Rust implementation traced back to spec requirements
- **Executable:** `cargo build` + `cargo test` in the workspace root

Every requirement in a spec MUST have:
- `**Implementation:**` link to the source file
- `**Tests:**` link to the test location
- At least one `#### Scenario:` in Given-When-Then format

## Local OpenSpec conventions (not upstream)

Upstream OpenSpec defines `proposal.md`, delta specs (ADDED/MODIFIED/REMOVED markers), `design.md`, and `tasks.md`. This repository — matching `mvl-lang/mvl` — uses a local flavor:

- **`specs/NNN-name/spec.md`** — requirements per numbered spec, in sheerpower-style format with implementation + test links per requirement. NOT delta specs.
- **`adr/NNNN-title.md`** — architectural decisions. Where cross-cutting design lands. Substitutes for the upstream `design.md` convention when the decision is durable.
- **`patterns/NNN-name.md`** — reusable code patterns.
- **GitHub issues** — carry both the *intent* work (what upstream calls `proposal.md`) and the *implementation checklist* (what upstream calls `tasks.md`). Epics for scope; stories for individual deliverables.

Rationale for dropping delta specs and `tasks.md`:

- Delta specs make sense when specs are the durable coordination surface. Here, GitHub issues are already the coordination surface, and delta-encoding requirement changes duplicates what commit history already captures.
- `tasks.md` makes sense when the checklist needs to live in the repo alongside code. GitHub issues + PR checkboxes cover that role with better discoverability and no extra file to keep synchronised.

## Specs

| # | Spec | Focus | Status |
|---|------|-------|--------|
| [001](specs/001-system-overview/spec.md) | System Overview | Architecture, the attribute model, the five-tool dispatcher, independence, greenfield rule | Active |
| [002](specs/002-qualified-subset/spec.md) | The Qualified Subset | `rust-limit` — six syntactic rejections gating the other four tools | Implemented |
| [003](specs/003-function-contracts/spec.md) | Function Contracts | `total` and `effect` — declare on the function, check the body | Implemented |
| [004](specs/004-information-flow/spec.md) | Information Flow via Types | `rust-ifc` — the label lives in the type; `relabel` polices the escape hatches | Implemented |
| [005](specs/005-refinement-obligations/spec.md) | Refinement Obligations | Three program points, two questions, the hypothesis context Γ | Implemented |
| [006](specs/006-layered-solver/spec.md) | The Layered Solver | L1–L4 shipped; L5 absent | Partial |
| [007](specs/007-runtime-enforcement/spec.md) | Runtime Enforcement | Residual obligations — 0/7 scenarios evidenced | Not implemented |
| [008](specs/008-reporting/spec.md) | Reporting Across the Assurance Levels | Diagnostics, the shared schema, obligation identity, aggregation | Implemented |
| [009](specs/009-mcdc/spec.md) | MC/DC Obligations | `rust-mcdc` — decision scanning, `n+1` vectors, tagged-test discharge; mutation engine kept library-only | Implemented |

## ADRs

Read in order — 0001 is the parent the rest specialise.

| # | ADR | Decision | Status |
|---|-----|----------|--------|
| [0001](adr/0001-annotation-driven-verification.md) | Annotation-Driven Verification Over Greenfield Rust | Host language is unmodified Rust; verification rides on inert `mvl::` attributes; no dependency on `mvl-lang/mvl`; greenfield only — no grandfathering, no exceptions | Accepted |
| [0002](adr/0002-qualified-subset.md) | The Qualified Subset | `rust-limit` gates the other four tools with six syntactic rejections, each tied to a tool it would otherwise break; no reviewed-exception protocol | Accepted |
| [0003](adr/0003-function-contracts-total-effect.md) | Function Contracts — `total` and `effect` | The baseline shape: declare on the function, check the body, report locally; no solver, no hypothesis context, never a premise | Accepted |
| [0004](adr/0004-information-flow-via-types.md) | Information Flow via Types | The label lives in the type and `rustc` propagates it; `#[mvl::relabel]` is only a declared exception at the two boundary crossings | Accepted |
| [0005](adr/0005-refinement-obligations.md) | Refinement Obligations and the Native Solver | Three program points, two questions, one native backend; obligations discharged against a hypothesis context Γ. *Supersedes the original 0001 and 0002.* | Accepted |
| [0006](adr/0006-layered-solver-and-runtime-enforcement.md) | Layered Solver and Runtime Enforcement | `L4` is Fourier–Motzkin (not Cooper); `L5` via feature-gated Z3; enforcement by active proc macros with `assert!` always and a per-function opt-out; Γ's soundness invariant stated once | Accepted |
| [0007](adr/0007-assurance-vocabulary.md) | Assurance Vocabulary | Assurance is the argument; verification, traceability and evidence are the three levels below it; compliance is downstream, not a fourth level | Accepted |

## Patterns

(none yet)

## The five tools

| Crate | Attribute | Purpose | Depends on |
|---|---|---|---|
| `rust-limit` | *(lint pass, no attribute)* | Enforce the qualified subset of Rust (Wave 2b design in `mvl-spec` / [rust-limit-linter.md](https://github.com/iheitlager/my-brain/blob/main/work/projects/mvl/rust-limit-linter.md)) | `mvl-rust-core` |
| `rust-total` | `#[total]` | Totality: exhaustive matches, terminating recursion | `mvl-rust-core` |
| `rust-refine` | `#[refine(pred)]` | Refinement types, discharged via layered dispatch | `mvl-rust-core`, MVL solver |
| `rust-effect` | `#[effect(list)]` | Effect algebra tracking on function signatures | `mvl-rust-core` |
| `rust-ifc` | `#[label(l)]` | Information flow labels + declassification | `mvl-rust-core` |

Shared infrastructure lives in `mvl-rust-core` (AST walker, attribute grammar, obligation solver bindings, diagnostic emission). Each tool crate publishes independently to crates.io.

## Publish order

The five tools ship independently; users install what they want. Recommended shipping sequence:

1. `rust-limit` — cheapest, no new semantics, gates the others
2. `rust-total` — smallest new surface
3. `rust-refine` — MVL's headline feature; requires the L1–L5 dispatch behind the attribute
4. `rust-effect` — needs the full effect algebra
5. `rust-ifc` — has adjacent prior art (Labeled IO, HLIO), still design-heavy
