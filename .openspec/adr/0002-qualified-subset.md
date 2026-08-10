---
status: Accepted
date: 2026-07-29
---

# ADR-0002: The Qualified Subset — Contracting Rust for Verifiability

## Context

ADR-0001 establishes that verification attaches to ordinary Rust through inert
attributes, and that four of the five tools consume those attributes on
functions. That leaves a hole: **the attributes say what to verify, but nothing
says what the verifier is allowed to encounter.**

Rust's language surface is much larger than MVL's. `unsafe` reinterprets memory;
`dyn Trait` defers the callee to runtime; an arbitrary macro expands to anything.
Each of these is legitimate, idiomatic Rust — and each removes the ground the
other four tools stand on. An `#[mvl::ensures]` on a function whose body contains
`transmute` is not a weaker guarantee than one without; it is *no* guarantee,
delivered in the same font.

So the subset is not a style opinion. It is the precondition under which the
other tools' output means anything, and it has to be enforced *before* they run.

`rust-limit` is also the one tool with **no annotation surface at all** — there
is nothing to opt into. It is a whole-file syntactic lint. That structural
difference is why it gets its own ADR rather than sharing ADR-0003's
function-contract model.

## Decision

### 1. `rust-limit` runs first, unconditionally, over the whole file

`cargo mvl check` orders the tools `limit → total → refine → effect → ifc`
(ADR-0001 §3). `rust-limit` gates the rest: a file outside the subset is
rejected before any obligation is discharged, so the later tools never have to
reason about what they cannot model.

It is a **pure syntactic lint** — `syn::parse_file` plus one
`syn::visit::Visit` pass per rule, no type information, no name resolution
(ADR-0001's shared boundary). Every violation is `Level::Error`; there is no
warning tier.

### 2. Six checks, each tied to a specific tool it would otherwise break

Implemented one module per rule in `crates/rust-limit/src/lints/`. The rationale
column is the load-bearing part — a rule whose justification is "this is bad
style" does not belong here.

| # | Rejected | What it would break |
|---|---|---|
| 1 | **`unsafe`** — blocks, `unsafe fn`, `unsafe impl`, `unsafe trait` | Everything. `unsafe` is precisely the escape hatch from what the type system can check, and every tool's reasoning is downstream of the type system holding. Invariants that live only in the author's head are not verifiable by construction. |
| 2 | **`dyn Trait`**, including nested (`Box<dyn Any>`) | `rust-refine` and `rust-effect`. Obligations and effect rows attach to *concrete* signatures. A `dyn` call site does not syntactically know which implementation runs, so there is no single `requires` to discharge or effect list to check. |
| 3 | **Named lifetimes** beyond `'static` and `'_` | `rust-refine`. A named lifetime parameter usually encodes a cross-reference invariant ("the result lives as long as the input") that is itself a refinement obligation — one nothing currently models. Restricting to elision keeps signatures inside what the solver can see. |
| 4 | **Macro invocations** outside a curated allowlist | `rust-refine` and `rust-effect` most acutely. `syn` keeps a macro body as an opaque token stream, so a call, an `unsafe` block, or an effectful operation inside one is *invisible* — not rejected, invisible. A syntactic pass cannot see through expansion. |
| 5 | **`transmute`** (matched on last path segment) | Everything, and most directly. It reinterprets bits with no compiler-checked relationship between source and target type — the single shortest path to violating every guarantee the other tools establish. |
| 6 | **Raw address-of** (`&raw const` / `&raw mut`) | Same family as #1. Once a raw address exists, every subsequent dereference is an unsafe operation by construction, and no tool has a story for pointer aliasing or provenance. |

Current allowlist for #4: `println`, `print`, `format`, `write`, `writeln`,
`vec`, `assert`, `assert_eq`, `assert_ne`, `matches`, `panic`, `todo`,
`unimplemented`, `loop_decreases` (`mvl::loop_decreases!`, ADR-0010 —
expands to nothing, same shape as the rest of this list). Deliberately
small, and expected to grow as real use surfaces macros that provably
expand to nothing outside rules 1–3, 5–6.

`macro_rules!` **definitions** are exempt — only invocations are restricted.
Defining a macro is fine; invoking an unreviewed one is not. Derive and
attribute macros are a different `syn` syntax form and are **not covered** by
rule 4.

### 3. Two rules are knowingly imprecise, in the safe direction

- **#3 (lifetimes)** is the weakest-justified of the six and the most likely to
  loosen. Its rationale is a placeholder for reasoning `rust-refine` does not
  yet do; it should be revisited when refinements need to describe borrowed
  data, not before.
- **#5 (`transmute`)** matches the callee path's **last segment**, so it catches
  re-exported and aliased spellings — at the cost of a false positive on an
  unrelated function that happens to be named `transmute`. Deliberate: ADR-0001
  §5 says precision may be traded for soundness, never the reverse, and a false
  *rejection* is the safe direction for a gate.

### 4. Not a coding standard

Explicit non-goal. `rust-limit` is not MISRA-for-Rust and does not overlap
`clippy`. It targets exactly the constructs that break the other four tools.
Anything that is merely a code-quality concern is `clippy`'s job and is already
available for free.

### 5. No reviewed-exception protocol — and #19 is resolved as *rejected in this
form*

Every check is an unconditional reject. Ferrocene's certified-subset lint solves
the escape-hatch problem with a two-tier design
(`#[ferrocene::prevalidated]` for a reviewed function,
`#[allow(ferrocene::unvalidated)]` for a reviewed call site), and #19 tracks
adopting that shape.

**Under ADR-0001 §5, that shape is not adopted.** An exception attribute would
make the subset's guarantee "someone looked at it", which is not a property the
four downstream tools can compose over — they would have to treat every
annotated function as potentially containing anything, which is the state the
subset exists to prevent.

What #19's underlying need *does* justify, and what it should be re-scoped to:

- **Growing the rule set's precision** so that fewer legitimate programs are
  rejected — extending the rule-4 allowlist, and revisiting #3 per §3.
- **A rationale-carrying exclusion at the file or crate boundary** rather than
  the construct boundary, if a qualified stdlib or a vendored dependency has to
  sit outside the subset. That is a *scoping* decision (this file is not
  verified) rather than an *exception* (this file is verified except here), and
  it does not weaken any claim the tools make about files inside the boundary.

This is a genuine divergence from Ferrocene, taken knowingly: Ferrocene
qualifies a toolchain for use on existing code, which makes reviewed exceptions
unavoidable. ADR-0001 §5 targets greenfield code, which makes them unnecessary.

## Consequences

- **The subset is the precondition for every other ADR's claims.** A guarantee
  from `rust-refine`, `rust-total`, `rust-effect` or `rust-ifc` is conditional
  on `rust-limit` having passed on the same file. Nothing in the architecture
  enforces that ordering outside `cargo mvl check` — a user invoking
  `cargo mvl-refine` directly gets refinement output on unrestricted Rust, with
  no warning that the precondition is unmet.
- **Rule 4 is the one that bites in practice.** Idiomatic Rust uses far more
  macros than the starter allowlist admits (`#[derive]` is exempt, but
  `thiserror`, `tracing`, `serde_json::json!`, test macros are not). Expect this
  list to be the most frequently edited part of the tool.
- **Rule 3 rejects a large class of correct programs.** Any function returning a
  borrow tied to a named input lifetime is out. Combined with ADR-0001's note
  that `impl` methods are largely unanalysed, the practically verifiable surface
  today is roughly: free functions over owned scalars.
- **`unsafe impl` / `unsafe trait` are the only reason `rust-limit` visits
  `ItemImpl`/`ItemTrait`.** It is therefore the only tool that sees `impl`
  blocks at all — the annotation-consuming tools do not (ADR-0001).
- **Rejecting #19's form leaves a real need unmet.** If a qualified-stdlib story
  or a vendored dependency later forces the issue, the answer is the
  file/crate-boundary exclusion in §5, and it needs its own ADR — not an
  attribute.

## Links

- `mvl-lang/mvl-rust`#4 (`rust-limit`), #18 (implementation)
- `mvl-lang/mvl-rust`#3 (scope boundary — `syn` only, no `rustc_private`)
- `mvl-lang/mvl-rust`#19 (escape hatch — **rejected in its proposed form**, see §5)
- `mvl-lang/mvl-rust`#12 (Ferrocene qualified-subset epic — the compatibility
  question §5 diverges from)
- Spec `001-system-overview` Requirement 1
- Tests: `crates/rust-limit/tests/qualified_subset.rs`;
  examples: `examples/rust-limit-demo/{compliant,violating}/`
- ADR-0001 (annotation model and the greenfield rule this depends on)
