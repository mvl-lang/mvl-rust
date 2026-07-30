---
status: Accepted
date: 2026-07-29
---

# ADR-0005: Refinement Obligations and the Native Solver

> **Supersedes** the original ADR-0001 ("Solver Integration Story with
> `mvl-lang/mvl`", #7) and ADR-0002 ("Call-Site Obligations Against a Hypothesis
> Context", #38), merged here. The workspace-level independence premise that was
> ADR-0001 §Decision now lives in ADR-0001 §4; everything solver- and
> obligation-specific is below. No decision is reversed by the merge.

## Context

`rust-refine` is the one annotation tool that cannot check a declaration against
a body locally. ADR-0003's tools ask "does this body do something forbidden?" —
a question about syntax. A refinement asks "does this predicate hold for every
value that can reach this point?" — a question about *values*, which needs a
decision procedure and a record of what is known where.

Two decisions were originally recorded separately and belong together:

1. **Where the decision procedure comes from.** MVL has a layered dispatcher
   (`L1` trivial → `L2` intervals → `L3` → `L4` → `L5` SMT → runtime). Reusing
   it was rejected under ADR-0001 §4; this ADR records what replaced it.
2. **What question is asked, and where.** `rust-refine` v0.1 (#8) discharged
   every predicate as a standalone "is this coherent?" question, because no call
   graph existed. That was the wrong question at most program points.

Real MVL asks something else. Obligations there arise at *program points* and
are discharged against a hypothesis context Γ accumulating parameter
refinements, branch narrowing, and callees' postconditions. Confirmed against
its solver source rather than its ADR prose: every layer takes the call-site
argument and Γ, not a bare predicate. Its Z3 entry point is

```rust
try_z3(pred: &RefExpr, arg: &Expr, var_refs: &HashMap<String, Option<RefExpr>>, ...)
```

with query `Γ ∧ ¬pred(arg)`, `unsat` ⇒ proven. All 13 fixtures under its
`tests/solver/layer5/` are Γ-shaped; `01_chained_hypotheses.mvl` names Γ as the
reason that layer exists at all.

**So the original gap was not a missing decision procedure. It was a missing
question.**

## Decision

### 1. A native dispatcher, reimplemented, no upstream dependency

Per ADR-0001 §4, `mvl-rust-core` implements its own obligation dispatcher.
`ShellOutSolver` was removed rather than kept as a fallback — there is no
scenario in which shelling out to `mvl solve` is right, and dead-but-compiling
code would misrepresent the architecture to the next reader.

**v0.1 scope** was `L1` (trivial syntactic checks) and `L2` (interval
arithmetic) natively, with everything else falling through to a runtime check.

> **Scope since delivered** — `L3` landed as bounded-quantifier expansion (#31)
> and `L4` as Fourier–Motzkin elimination (#35). Note `L4` is **not** the Cooper
> quantifier elimination upstream's own docs name, an upstream naming inaccuracy
> tracked as [`mvl-lang/mvl`#2022](https://github.com/mvl-lang/mvl/issues/2022);
> ADR-0006 settles how this workspace labels it. `L5` (feature-gated Z3, `Int`/
> QF-NIA only) landed in #37, narrower in scope than originally framed — see
> ADR-0006 §3's amendment. The layer stack's completion and the runtime-check
> story are ADR-0006's subject, not this ADR's.

### 2. Three program points, two questions, one backend

| Site | Question | Entry point |
|---|---|---|
| Declaration (`#[mvl::requires]` on `f`) | is the predicate coherent? | `discharge_predicate` |
| Call site (`g(args)` in `f`'s body) | does `Γ_f` entail `g`'s precondition with `args` substituted? | `discharge_entailment` |
| Return site (`f` returns `e`) | does `Γ_f` entail `f`'s postcondition with `result := e`? | `discharge_entailment` |

A declaration site has no arguments to reason about, so coherence is the only
question available there. It is **not a weaker approximation of entailment** — it
is a different and independently useful check, since a self-contradictory
`requires` is a real defect and stays an error.

Return sites (#42) are the mirror of postcondition propagation: bind `result` to
the returned expression instead of to a `let` binding. Γ needed no changes —
branch narrowing and per-name invalidation already applied.

Outcomes mirror upstream's literal-vs-symbolic split:

| Query | Meaning | Result |
|---|---|---|
| `Γ ∧ ¬goal` UNSAT | holds for every value Γ permits | `Proven` |
| `Γ ∧ goal` UNSAT | fails for every value Γ permits | `Violated` |
| neither | may hold, may not | `Runtime` |

### 3. Γ's contents

1. The caller's own `requires` clauses — its parameters' refinements.
2. **Branch narrowing** — `if c { … }` adds `c`, the `else` arm adds `¬c`, a
   `while` body adds its condition. Γ is a stack, so a fact never outlives the
   block that established it.
3. **Postcondition propagation** — after `let y = g(x);`, `g`'s `ensures` enters
   Γ with `result := y` and `g`'s parameters bound to the actual arguments.

Propagation is **assumed rather than re-derived**, as in any modular verifier.
The return-site obligation in §2 is what discharges the other side of that
assumption — **for the `Violated` case only.** A postcondition that merely
reaches `Runtime` is still propagated as fact, which is an open soundness gap
(#47) and ADR-0006's subject. Recording it here because §3.3 is where the
unsound assumption is made.

### 4. Two implementation choices worth recording

**Negation stays inside the existing constraint fragment.** `¬goal` for a
conjunction is a disjunction, which the `Le`/`Eq` representation cannot hold.
Rather than adding disjunction support, `¬(c₁ ∧ … ∧ cₙ)` is checked one disjunct
at a time — `Γ ∧ ¬cᵢ` UNSAT for every `i` ⇒ proven — and over the integers
`¬(t ≤ 0)` is `t ≥ 1`, so each disjunct is a single inequality. Fourier–Motzkin
needed no new machinery. An equality clause's negation is a genuine disjunction
(`¬(t = 0)` is `t ≤ −1 ∨ t ≥ 1`) and is not represented directly either: since
`t = 0` holds exactly when `t ≤ 0` and `t ≥ 0` both do, each half is refuted as
its own query and both must come back unsat (#43). The same trick, one level
down.

**Hypotheses may be dropped; goal clauses may not.** A hypothesis outside the
linear fragment (an opaque call, a non-linear term) is skipped rather than
failing the query. Sound in both directions: fewer facts make `Γ ∧ ¬goal` easier
to satisfy (harder to prove) *and* `Γ ∧ goal` easier to satisfy (harder to call
violated). It costs precision, never correctness. Every *goal* clause must be
decided for `Proven`.

### 5. Scope

Same boundary ADR-0001 draws for the whole workspace, plus refinement-specific
limits. Each is asserted by a test so it stays a deliberate boundary rather than
becoming an unnoticed hole.

- Call resolution is **same-file, free functions only**. Anything else is
  silently unresolvable and produces no obligation. Methods in `impl` blocks are
  invisible, attributes and body (ADR-0001).
- **`match`-arm patterns don't narrow Γ**; only `if`/`else`/`while` conditions
  do. (A `match` arm *is* a return point since #42, but contributes no
  hypothesis.) Upstream's refinement analyzer does narrow four kinds of arm
  pattern; its `ensures` checker does not. This matches the latter.
- Calls inside macro invocations are invisible — ADR-0002 rule 4 narrows this.
- A **quantified `requires`** is a usable *goal* but not a Γ hypothesis — Γ
  clauses are `&&`-flattened expressions and a quantifier has no such form.
- **Γ is invalidated per name, not tracked through dataflow.** Rebinding
  (`let x = …`), assignment (`x = …`, `x += …`), or a mutable borrow (`&mut x`)
  retires every Γ clause mentioning that name. Deliberately blunt: the backend
  cannot see whether a callee writes through a `&mut`, so it assumes the worst.
  The alternative — keeping a fact about a value that is no longer there — is
  unsound rather than merely imprecise.
- **Arithmetic is over unbounded ℤ, with no overflow modelling.**
  `x >= i64::MAX ⊢ x + i64::MAX > 0` is `Proven`, though the Rust expression
  would overflow. Refinement predicates describe mathematical integers here, as
  in real MVL.
- **Bounded expansion is capped on the product of quantifier widths**, not each
  width independently — nesting two legal 1000-wide ranges would otherwise
  expand to a million instances, each a full entailment query.
- **Equality goals close by two independent mechanisms, neither subsuming the
  other** (#43). L1 reflexivity decides `t == t` structurally, the only route to
  a *non-linear* identity (`a * b == a * b`) since the linear fragment cannot
  represent one. The L4 split decides rearranged terms (`a + b == b + a`) that no
  tree comparison matches.
- **L1 reflexivity applies only to call-free terms.** Reflexivity is structural,
  so it is wrong in both directions for an impure term, and substitution reaches
  both: `span(gen(), gen())` against `requires(lo <= hi)` would be `Proven` —
  dropping a check that can genuinely fail — and `requires(a != b)` would be
  `Violated`, a compile error on a valid call. `is_call_free` gates the rule to
  an allow-list of shapes that cannot invoke user code. **Not a restriction
  relative to upstream**: MVL's `RefExpr` grammar cannot express a call at all,
  so the fragment *is* the upstream one — upstream is protected by a narrower
  predicate grammar, not by a better purity test. The residual gap is that
  call-free is a syntactic approximation of purity; #45 tracks the real signal,
  and ADR-0003 §3 records why `rust-effect`'s two-state model cannot supply it.
- **L1 reflexivity assumes integer semantics and is unsound for floats.**
  `x == x` is `false` for NaN, and `syn` carries no type information to exclude
  an `f64`. Upstream has the same unsoundness, unrecorded. A reason to keep the
  rule to the six comparison operators rather than generalising it; admitting
  float-typed terms needs a real type signal first.

## Consequences

- **Declaration-site `Proven` is a satisfiability claim** — *reported*
  identically to an entailment proof until #56. `discharge_predicate` answers
  "is this coherent", and the outcome entered the assurance JSON as a proven
  obligation with a layer, wire-indistinguishable from a real proof. On the
  compliant demo 7 of 16 reported obligations were of this kind, so anything
  consuming `prove.obligations[]` as evidence over-read it by more than double.
  Fixed by `ObligationClass` on the record (schema `1.1`): the underlying
  distinction stands as §2 intends, and the report now states it. The demo reads
  7 real entailment proofs of 16 records.
- **The two entry points share `classify_clause`**, so a rule added for one
  affects the other. #38 could add obligations without altering any existing
  outcome; that is no longer true in general — #43 added a rule there.
- **Cross-validation has produced real, asserted divergences from upstream**, per
  ADR-0001 §4. All six ported upstream *layer-5* fixtures close at **L4 with no
  SMT solver** (`crates/rust-refine/tests/call_sites.rs`). Three further
  deliberate divergences: a tri-state `SatOutcome` where upstream returns `bool`
  (so a complexity bail-out cannot masquerade as a proof); equalities kept in the
  Fourier–Motzkin phase where upstream drops them (`a == 5 ⊢ a − 4 > 0` needs Z3
  upstream, proves at L2 here); and `i128` coefficient carriers where upstream
  uses unchecked `i64`.
- **`SolverBackend` is not the extension point the original ADR-0001 claimed.**
  `rust-refine` calls the free functions directly, and the trait's `Obligation`
  carries a re-parsed `String` predicate with no Γ field — so it structurally
  cannot answer the entailment question. Either rework it to carry Γ or drop it.
- **No obligation identity.** Ids are `{fn}::{kind}` with no span or index, so
  two calls to one callee collide. Nothing keys on them today, but this blocks
  any keyed discharge cache — which ADR-0006's injection design would want.
- **Γ construction admits facts it has not established.** Beyond the `Runtime`
  propagation in §3, several Γ-construction paths produce a false `Proven` on
  compiling code (pattern bindings that do not invalidate, loop-carried
  mutation, arity-mismatch parameter capture). #47's diagnosis — that Γ's
  soundness condition was never written down, so it gets rediscovered one
  violation at a time — is correct and is why ADR-0006 states it once.
- **Still open at this ADR's date:** struct construction, coercions, `match`-arm
  narrowing, loop invariants, indexing bounds, and cross-file resolution. Upstream
  generates obligations at 15 program points; this implementation covers 3.

## Links

- `mvl-lang/mvl-rust`#7 (native dispatcher — originally ADR-0001), #8
  (`rust-refine` v0.1), #38 (call-site obligations — originally ADR-0002),
  #42 (return-site obligations), #31 (`L3`), #35 (`L4`), #43/#44 (equality goals
  and the call-free gate)
- `mvl-lang/mvl-rust`#37 (`L5`/Z3 — landed, narrowed in scope, see ADR-0006 §3),
  #45 (purity signal), #47 (Γ soundness invariant), #48 (return-point doc invariant)
- [`mvl-lang/mvl`#2007](https://github.com/mvl-lang/mvl/issues/2007) (superseded
  request that upstream expose a solver crate — closed under ADR-0001 §4),
  [`mvl-lang/mvl`#2022](https://github.com/mvl-lang/mvl/issues/2022) (`L4`
  naming)
- Spec `001-system-overview` Requirement 3
- ADR-0001 (annotation model, independence premise, greenfield rule)
- ADR-0003 (the simpler contract shape this one departs from)
- ADR-0006 (layer completion, runtime enforcement, and the Γ invariant)
