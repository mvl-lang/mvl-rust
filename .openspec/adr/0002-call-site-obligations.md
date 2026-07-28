---
status: Accepted
date: 2026-07-28
---

# ADR-0002: Call-Site Obligations Against a Hypothesis Context

## Context

`rust-refine` v0.1 (#8) discharged every `#[mvl::requires]`/`#[mvl::ensures]`
predicate as a standalone question: **is this predicate internally coherent** —
satisfiable, as opposed to self-contradictory like `x >= 10 && x < 5`? That was a
deliberate simplification, adopted because no call graph existed and `syn`-only
scanning has no type information.

Real MVL asks something else. Obligations there arise at *program points* — call
sites, return statements, struct construction, explicit coercions — and are
discharged against a hypothesis context Γ that accumulates parameter
refinements, branch-condition narrowing, and callees' postconditions. Confirmed
directly against its solver source rather than its ADR prose: every layer takes
the call-site argument and Γ, not a bare predicate. The Z3 entry point is

```rust
try_z3(pred: &RefExpr, arg: &Expr, var_refs: &HashMap<String, Option<RefExpr>>, ...)
```

and its query is `Γ ∧ ¬pred(arg)`, with `unsat` ⇒ proven
(`mvl-lang/mvl`, `src/mvl/checker/solver/layer5.rs`). All 13 fixtures under its
`tests/solver/layer5/` are Γ-shaped; `01_chained_hypotheses.mvl` names Γ as the
reason that layer exists at all.

So the gap wasn't a missing decision procedure. It was a missing *question*.

This surfaced while scoping L5/Z3 (#37): a Z3 backend behind a coherence-only
obligation model could only check standalone satisfiability, which L1/L2/L4
already close for the linear cases. Γ is what makes the deeper layers earn their
keep — so it lands first (#38), and #37 builds on it.

## Decision

**Both questions, at their own program points, through two entry points on the
same backend.**

| Site | Question | Entry point |
|---|---|---|
| Declaration (`#[mvl::requires]` on `f`) | is the predicate coherent? | `discharge_predicate` |
| Call site (`g(args)` in `f`'s body) | does `Γ_f` entail `g`'s precondition with `args` substituted? | `discharge_entailment` |

A declaration site has no arguments to reason about, so coherence is the only
question available there — it is not a weaker approximation of entailment, it is
a different and independently useful check (a self-contradictory `requires` is a
real defect, and stays an error). Keeping it also means this change adds
obligations without altering any existing outcome.

Call-site outcomes mirror real MVL's own literal-vs-symbolic split in `impl_z3`:

| Query | Meaning | Result |
|---|---|---|
| `Γ ∧ ¬goal` UNSAT | holds for every value Γ permits | `Proven` |
| `Γ ∧ goal` UNSAT | fails for every value Γ permits | `Violated` |
| neither | may hold, may not | `Runtime` |

### Γ's contents

1. The caller's own `requires` clauses — its parameters' refinements.
2. Branch narrowing — `if c { … }` adds `c`, the `else` arm adds `¬c`, a `while`
   body adds its condition. Γ is a stack, so a fact never outlives the block
   that established it.
3. Postcondition propagation — after `let y = g(x);`, `g`'s `ensures` enters Γ
   with `result := y` and `g`'s parameters bound to the actual arguments.
   Assumed rather than re-derived, as in any modular verifier: `g`'s obligation
   to establish its own postcondition is a separate obligation.

### Two implementation choices worth recording

**Negation stays inside the existing constraint fragment.** `¬goal` for a
conjunction is a disjunction, which the `Le`/`Eq` representation cannot hold.
Rather than adding disjunction support, `¬(c₁ ∧ … ∧ cₙ)` is checked one disjunct
at a time — `Γ ∧ ¬cᵢ` UNSAT for every `i` ⇒ proven — and over the integers
`¬(t ≤ 0)` is `t ≥ 1`, so each disjunct is a single inequality. Fourier-Motzkin
needed no new machinery. An equality goal has no such form, so L4 cannot negate
one; L2's point interval covers that case instead.

**Hypotheses may be dropped; goal clauses may not.** A hypothesis outside the
linear fragment (an opaque call, a non-linear term) is skipped rather than
failing the query. That is sound in both directions: fewer facts make `Γ ∧ ¬goal`
easier to satisfy (harder to prove) and `Γ ∧ goal` easier to satisfy (harder to
call violated). It costs precision, never correctness. Every *goal* clause must
be decided for `Proven`.

### Scope

Same boundary `rust-effect` (#9) draws for the same reason — no type information,
no cross-file resolution:

- Call resolution is same-file, free functions only. Anything else is silently
  unresolvable and produces no obligation.
- `match`-arm patterns don't narrow Γ; only `if`/`else`/`while` conditions do.
- Calls inside macro invocations are invisible (`syn` keeps a macro body as an
  opaque token stream).
- A quantified `requires` is a usable *goal* but not a Γ hypothesis — Γ clauses
  are `&&`-flattened expressions and a quantifier has no such form.

Each is asserted by a test so it stays a deliberate boundary rather than becoming
an unnoticed hole.

## Consequences

- Spec Requirement 3's third scenario ("Genuine violation rejected", `f(5)`
  against `#[refine(x < 0)]`) is satisfied for the first time — it always
  described a call site, which the coherence-only model could not see.
- **A divergence from real MVL, verified rather than assumed.** Its
  chained-hypothesis fixtures (`tests/solver/layer5/01,03,04,06,08`) need
  **L5/Z3** there, because its L2 requires constant bounds and its L4 handles
  linear-expression arguments rather than bare variables whose hypotheses
  reference other variables. Ported here, they close at **L4 with no SMT solver
  at all** — our L4 runs Fourier-Motzkin over `Γ ∪ {¬goal}` directly. This is
  the independent-implementation premise (epic #1) producing exactly the kind of
  signal it exists to produce, and it shrinks what remains for #37 to genuinely
  non-linear obligations.
- L5 (#37) now has the Γ-shaped interface its real counterpart expects, and can
  be added as another layer behind `discharge_entailment` rather than as a
  reframing of the whole obligation model.
- `mvl-lang/mvl`'s `tests/solver/layer5/` becomes a reusable cross-validation
  corpus for this workspace.
- ADR-0001's "v0.1 scope" (L1+L2 native, L3–L5 deferred) is unchanged as a
  *solver-layer* decision. This ADR changes what question those layers answer,
  not which layers exist or where they live. The no-dependency-on-`mvl-lang/mvl`
  constraint is likewise untouched: this reads its source as a design reference,
  which is what an independent reimplementation is supposed to do.
- Not attempted here, and still open: return-site obligations (does the body
  establish its own `ensures`?), struct construction, coercions, `match`-arm
  narrowing, and cross-file resolution.

## Links

- `mvl-lang/mvl-rust`#38 (this decision)
- `mvl-lang/mvl-rust`#37 (L5/Z3 — builds on this)
- `mvl-lang/mvl-rust`#8 (`rust-refine` v0.1), #9 (same-file call-graph precedent)
- ADR-0001 (solver integration story — native backend, no shared solver)
