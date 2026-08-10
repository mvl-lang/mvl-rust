# Architecture: The Shared Solver Model

`mvl-rust` presents as five independent tools — `rust-limit`, `rust-total`,
`rust-refine`, `rust-effect`, `rust-ifc` — but that's the surface. Underneath,
there's one shared verification engine, and each tool decides how much of its
scope routes through it.

## Three tiers, not five silos

The five tools fall into three architectural tiers:

| Tier | Tools | What they do | Solver involvement |
|------|-------|--------------|-------------------|
| **Syntactic** | `rust-limit` | Pure syntactic gate | None — pattern matching on AST |
| **Structural** | `rust-effect`, `rust-ifc` | Type-level containment | None — flat set/lattice tracking |
| **Proof** | `rust-refine`, `rust-total` (partial) | Construct a proof or fall back | Yes — layered solver with Γ |

### Syntactic: `rust-limit`

Not a verifier at all. A linter. It pattern-matches on the AST to reject
`unsafe`, `dyn Trait`, and explicit lifetimes. No solver, no analysis, no
hypothesis context. The guarantee is: "code that passes `rust-limit` is in the
subset the other tools can reason about."

### Structural: `rust-effect` and `rust-ifc`

These are typical annotation-based compiler validation — the same category as a
strict type checker or a borrow-checker extension. Per ADR-0003 §3: *"no
hypothesis context, no solver, no cross-procedural state."*

- **`rust-effect`**: flat effect-set containment. A function annotated with
  `#[mvl::effect(FileRead)]` must only call things whose declared effects are
  a subset. No proof, just set math.
- **`rust-ifc`**: label-lattice tracking through types. `Tainted[String]` flows
  to `Tainted[String]` but not to `Clean[String]` without an explicit
  `#[mvl::relabel]`. No proof, just lattice climbing.

### Proof: `rust-refine` and the solver

`rust-refine` is the real outlier. It has:

1. **A layered solver** — native L1–L4 linear arithmetic (Fourier-Motzkin,
   interval, congruence closure, LRA), optional Z3 at L5.
2. **A hypothesis context (Γ)** — postconditions from prior calls propagate
   into subsequent proof obligations.
3. **A documented fallback** — obligations the solver can't close become
   runtime `assert!` (ADR-0006), not a silent gap.

This is "verification first, testing's usual role narrowed" — testing catches
what wasn't proven, but the boundary is explicit and shrinking.

## `rust-total` crosses the boundary

Here's where the architecture gets interesting. `rust-total` *used* to sit
cleanly in the "structural" tier — presence checks, no proof:

- Panic-freedom: syntactic scan for `.unwrap()`, `panic!`, indexing without
  bounds, etc.
- Termination: required a `#[mvl::decreases(measure)]` but only checked that
  the measure existed, not that it actually decreased.

**That changed.** Termination now routes through `rust-refine`'s solver:

| Check | How it's verified | ADR |
|-------|-------------------|-----|
| Recursive termination | `discharge_entailment` via `rust-refine` | ADR-0009 |
| Loop termination | Same solver, same hypothesis context | ADR-0010 |

The decreases measure isn't just declared — it's *proven* to decrease, using
the same `Γ` derivation from `#[mvl::requires]` that `rust-refine` uses for
refinement types. So `rust-total` is now a **hybrid**: panic-freedom stays a
lint, termination is a genuine borrowed proof.

## The architectural reading

The five-tool split was never "one tier of verification per tool." It turns out
to be:

> **One shared verification engine, and each tool decides how much of its
> surface routes through it.**

The solver is the actual asset. Its reach can grow into any tool's scope
without that tool growing its own solver. When `rust-total` needed real
termination proofs instead of presence checks, it didn't build a termination
prover — it borrowed `rust-refine`'s.

This is a more interesting reading of "maximum verification" than five
independent silos. The ceiling isn't per-tool complexity — it's how much of
each tool's surface can be rerouted through the shared proof engine.

## What this means for adoption

- **Start with `rust-limit`** — it's zero-cost, zero-solver, just shrinks your
  surface.
- **Add `rust-effect`/`rust-ifc`** — still zero-solver, structural checks
  only.
- **Add `rust-refine` contracts** — now you're proving things. Obligations that
  don't close fall back to runtime `assert!`.
- **Add `rust-total` with decreases** — your termination proofs share the same
  solver layer. No new infrastructure.

The incremental adoption path maps onto the architectural tiers: syntax →
structure → proof. And at the proof tier, the engine is shared — contracts,
refinements, and termination all feed the same `discharge_entailment`.

---

*See also: [ADR-0003](../.openspec/adr/0003-structural-no-solver.md) (no solver for
effect/IFC), [ADR-0006](../.openspec/adr/0006-fallback-runtime-assert.md) (unproven →
runtime assert), [ADR-0009](../.openspec/adr/0009-recursive-termination.md) (recursive
termination via solver), [ADR-0010](../.openspec/adr/0010-loop-termination.md) (loop
termination via solver).*
