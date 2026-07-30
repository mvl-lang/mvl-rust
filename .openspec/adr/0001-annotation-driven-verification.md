---
status: Accepted
date: 2026-07-29
---

# ADR-0001: Annotation-Driven Verification Over Greenfield Rust

## Context

`mvl-rust` reimplements MVL's verification semantics as a set of tools that run
over **ordinary Rust source**. The epic's premise (#1) is that MVL's semantics
are *language-independent*, and the way to demonstrate that is a second,
genuinely separate implementation that can disagree with the first.

Two things had to be settled before any tool could be written, and neither is
specific to a single tool:

1. **How does verification attach to Rust?** Rust has no `where`-clause
   refinements, no effect rows, no security labels. MVL does. Something has to
   carry that information into a Rust source file.
2. **What code is this for?** A verifier that must accommodate arbitrary
   existing Rust is a different project from one that verifies code written to
   be verifiable. The two lead to opposite designs at almost every decision
   point.

This ADR answers both, and records the independence premise that the rest of
the workspace inherits. It is the parent of ADR-0002 (the subset), ADR-0003
(`total`/`effect`), ADR-0004 (information flow) and ADR-0005 (refinement
obligations).

## Decision

### 1. The host language is unmodified Rust; verification rides on attributes

**No dialect, no fork, no new syntax, no preprocessor.** A file under
`mvl-rust`'s tools is a Rust file that `rustc` compiles unchanged. Verification
information is carried by attribute macros in the `mvl::` namespace:

| Attribute | Argument | Owner |
|---|---|---|
| `#[mvl::requires(p)]` | predicate | `rust-refine` (ADR-0005) |
| `#[mvl::ensures(p)]` | predicate over `result` | `rust-refine` (ADR-0005) |
| `#[mvl::total]` | — | `rust-total` (ADR-0003) |
| `#[mvl::decreases(e)]` | measure | `rust-total` (ADR-0003) |
| `#[mvl::effect(…)]` | effect list | `rust-effect` (ADR-0003) |
| `#[mvl::relabel(…)]` | label | `rust-ifc` (ADR-0004) |
| `#[mvl::refine(…)]` | predicate | **unclaimed** — see Consequences |
| `#[mvl::partial]` | — | **unclaimed** |
| `#[mvl::label]` | — | **unclaimed** |

Parsed centrally by `MvlAttr::try_from_attribute`
(`crates/mvl-rust-core/src/attrs.rs`), recognised by **last path segment**, so
`#[mvl::requires]`, `#[requires]` and `#[some_alias::requires]` all resolve.
Attributes the workspace does not own return `None` rather than erroring, so a
function carrying `#[derive]`, `#[inline]` or any third-party attribute is
skipped rather than rejected.

### 2. The attributes are inert to the compiler

`crates/mvl-macros` is a real `proc-macro` crate whose attributes are
pass-throughs: each returns the annotated item unchanged and **discards its own
argument tokens**. Consequences, all deliberate as of this ADR:

- Annotated code compiles and runs **identically** whether or not any
  verification tool has been run.
- Verification is **out-of-band**: the five tools read source text with `syn`
  and print diagnostics. Nothing they do reaches `rustc`.
- The `mvl` facade crate is a **convenience, not a requirement** — it exists to
  make the attribute names resolvable, and the tools scan the same source
  whether or not it is a dependency.

This is what makes adoption cheap and removal free. It is also the source of
the workspace's largest open gap: an obligation the tools cannot discharge is
**reported, not enforced**. That gap is out of scope here and is the subject of
ADR-0006.

**Amendment (#53): no longer true of `#[mvl::requires]`/`#[mvl::ensures]`.**
Those two now expand to a real `assert!` (ADR-0006 §4) rather than discarding
their tokens — the gap this section named is what ADR-0006 closes for them.
What survives this section, precisely:

- **The five verification tools are entirely unaffected** — `rust-limit`,
  `rust-total`, `rust-refine`, `rust-effect`, `rust-ifc` still read source text
  with `syn` and never reach `rustc`. Nothing about *their* architecture
  changed; what changed is the attribute *mechanism* for two of the six
  attributes. Whether any of the five tools has ever run remains completely
  irrelevant to what a build produces — the macro expands the same way either
  way, since it is not one of the five tools and consults none of them.
- **`total`, `decreases`, `effect`, `label`, `relabel` remain exactly as
  described here** — inert pass-throughs, discarding their tokens, read only by
  the tools that report on them.
- **The facade is no longer merely a convenience for `requires`/`ensures`.**
  Enforcement needs `mvl-macros`' real expansion, which needs the `mvl` crate
  actually present as a dependency — drop it and the attribute doesn't resolve,
  so the crate fails to compile. That failure is deliberate: it is loud rather
  than the silent gap this section used to describe. The tools' own behavior
  — scanning the same source whether or not `mvl` is a dependency — is
  unchanged.

### 3. One dispatcher, five tools, no shared analysis state

`cargo mvl check <FILE>...` runs the five tools in a fixed order —
`limit → total → refine → effect → ifc` — as **in-process library calls**, not
subprocesses. Each tool is independently a `cargo` subcommand
(`cargo mvl-limit`, `cargo mvl-refine`, …).

Shared infrastructure is deliberately thin: `MvlAttr` (the attribute grammar),
`Diagnostic`/`Level` (the output form), and the solver (`mvl-rust-core`). There
is **no shared program representation** — each tool builds its own
`syn::visit::Visit` pass over the same AST. Tools do not exchange results.

The dispatcher takes **explicit file paths**, not a Cargo crate graph. It reads
no `Cargo.toml`, resolves no dependencies, and there is no `build.rs` anywhere
in the workspace.

### 4. No dependency on `mvl-lang/mvl` — build-time, runtime, or logical

This is the independence premise, and it is normative for the whole workspace.

An earlier draft of the solver design proposed reusing MVL's own solver — by
subprocess or as a linked library. Both were rejected, for two reasons that
generalise beyond the solver:

1. **It would not be independent verification.** If `mvl-rust` calls the same
   implementation `mvl-lang/mvl` uses, it is that implementation with a Rust
   UI, not a second one. A bug or gap in the shared code would pass
   "independent" verification silently, because there is nothing independent to
   disagree with it. **Cross-validation is the entire mechanism by which a
   divergence gets caught**, and it only works if the two are actually separate.
2. **It would put pressure on `mvl-lang/mvl`.** Rust has a larger language
   surface than MVL. Sharing an implementation creates direct pressure to grow
   upstream's codebase — which should stay scoped to MVL's smaller language —
   to accommodate a host language it was never designed for. Separation keeps
   that pressure from ever reaching upstream.

MVL's source is read as a **design reference**, and its test fixtures are
ported as **cross-validation corpora**. Neither is a dependency. Where the two
implementations disagree, the divergence is asserted by a test rather than
smoothed over — see ADR-0005 for the worked instances.

### 5. Greenfield only — no grandfathering, no exceptions

**Normative.** The target is code written to be verified. It is not a tool for
retrofitting guarantees onto an existing codebase.

Concretely:

- **Code is generated or written to conform.** When a construct fails
  verification, the resolution is a change to the code, **not** an exception in
  the tool. There is no `#[allow]`-shaped escape hatch for any verification
  attribute, and none will be added on the grounds that existing code needs it.
- **No legacy accommodation.** No compatibility mode, no "warn instead of
  error" tier for constructs the tools cannot verify, no per-crate opt-outs.
- **Precision may be traded for soundness, never the reverse.** A construct the
  tools cannot model yields *no claim* — not a weakened one. Imprecise is
  acceptable; unsound is not.
- **When upstream and this implementation disagree**, the divergence is
  recorded and tested. Neither side is silently accommodated.

The reason to state this as a rule rather than a preference: every relaxation
of it is individually reasonable and collectively fatal. A subset that admits
reviewed exceptions is a subset whose guarantee is "someone looked at it",
which is not a guarantee a tool can compose over.

## Consequences

- **Three attributes are parsed but unclaimed.** `refine`, `partial` and
  `label` are recognised by `MvlAttr` and read by **no tool**. `refine` is still
  advertised in spec `001-system-overview`'s own examples, so a reader following
  the spec writes an annotation that does nothing. Either claim them or remove
  them; leaving them parsed-but-inert is the one option that misleads.
- **No type information, in any tool.** `syn` gives an AST, not types, name
  resolution, or a module graph. Every tool inherits this boundary, and it is
  the root cause of several known unsoundnesses (float reflexivity in ADR-0005,
  method-call invisibility, unbounded-ℤ arithmetic). A tool needing types would
  need `rustc_private` and a pinned nightly, which §3's architecture rules out.
- **Same-file, free-function scope is the shared default.** No cross-file or
  cross-crate resolution. Calls to anything else are silently unresolvable and
  produce no obligation.
- **Methods in `impl` blocks are largely invisible.** Only `rust-limit` visits
  `ItemImpl`/`ItemTrait`. The annotation-consuming tools visit `ItemFn` and do
  not handle `ImplItemFn`, so an annotated method is unanalysed end to end —
  including its attributes. This is the largest practical coverage gap the
  attribute model currently has, since most idiomatic Rust is methods.
- **Verification is advisory until ADR-0006.** §2 makes the attributes inert,
  so a residual obligation is a note, not a check. Any claim the tools make
  about unproven obligations must say so — in diagnostics, in Γ, and in the
  assurance JSON.
- **The greenfield rule constrains #19.** `rust-limit`'s tracked request for a
  Ferrocene-style reviewed-exception protocol
  (`#[ferrocene::prevalidated]`-shaped) is in direct tension with §5. ADR-0002
  resolves it for the subset lint specifically.

## Links

- `mvl-lang/mvl-rust`#1 (epic — independent implementation premise)
- `mvl-lang/mvl-rust`#7 (the independence decision, originally recorded in the
  superseded ADR-0001 "Solver Integration Story")
- `mvl-lang/mvl-rust`#3 (`mvl-rust-core` shared infrastructure)
- `mvl-lang/mvl-rust`#19 (subset escape hatch — constrained by §5)
- [`mvl-lang/mvl`#2007](https://github.com/mvl-lang/mvl/issues/2007) (request
  that upstream expose a solver crate — closed as unwanted under §4)
- ADR-0002 (the qualified subset), ADR-0003 (`total`/`effect`),
  ADR-0004 (information flow), ADR-0005 (refinement obligations), ADR-0006 (layered
  solver and runtime enforcement)
