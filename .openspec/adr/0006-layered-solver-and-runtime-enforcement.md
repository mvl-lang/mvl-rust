---
status: Accepted
date: 2026-07-29
---

# ADR-0006: Layered Solver Completion and Runtime Enforcement

## Context

ADR-0005 settles *what question* is asked at each program point. This ADR settles
*what answers the layer stack can produce* and *what happens to the ones it
cannot*.

It exists because those two turned out to be the same decision. A solver that
cannot close every obligation is fine; a solver that cannot close every
obligation **and has no account of the remainder** is not — and that is the state
the workspace is in. Splitting layer completion from enforcement is what let the
gap open.

Written after a full audit of `mvl-lang/mvl` (~10,400 lines of solver and
contracts), spec `018-refinement-solver`, four upstream ADRs, the ticket history,
and `~/papers/paper_refinements`. Three findings drive everything below.

**1. The normative standard requires five layers *and* a runtime assertion.**
Upstream spec `018` makes L1–L5 all **MUST**, with L5 feature-gated and falling
through gracefully. This workspace's own spec `001-system-overview`:192 adds:

> compilation MUST succeed AND `rust-refine` **MUST emit a runtime assertion at
> the site** with attribution to the unclosed obligation

`rust-refine` prints the *sentence* "inserting a runtime check" as a
`Level::Note` and inserts nothing. That contradiction is the direct cause of #47:
Γ was built on a claim the diagnostics made and the code did not honour.

**2. Upstream does not inject on residual — it always enforces.** `assert!` is
emitted for *every* runtime-checkable `requires`/`ensures`, unconditionally,
regardless of proof outcome. Codegen never sees a proof result. The static solver
is an **early-error layer on top of universal runtime enforcement**, not a filter
deciding what to emit. That is what makes upstream's Γ-propagation sound — the
assumed fact is backed by a check nobody can disable.

Upstream's *implementation* of that intent has at least seven holes (explicit
`return` paths bypass the `ensures` assert; inline parameter `where` refinements
emit nothing; `-> T where p` emits a comment; trait-impl contracts are dropped at
lowering; MC/DC instrumentation short-circuits it; the LLVM backend emits no
contract checks; unlowerable predicates are silently dropped). So there is no
working mechanism to port — only an intent to implement, and room to implement it
better.

**3. The paper's own evaluation undercuts the five-layer story.** Across 174
obligations in 8 case studies: L1 63.2%, L2 6.9%, **L3 0.6% (one obligation)**,
**L4 2.3% (four)**, L5 19.5%, runtime 7.5%. L5 is by far the largest missing
win; L3 and L4 are near-vestigial. Offsetting this, all six ported upstream
*L5* fixtures already close at L4 here (ADR-0005 §Consequences), so the real L5
residue is narrower than 19.5%.

## Decision

### 1. `L4` keeps its name; the spec is corrected to describe what it is

`L4` is **Fourier–Motzkin elimination over ℚ**, plus integer tightening of strict
inequalities (`t < 0 → t + 1 ≤ 0`), plus a divisibility test for
*single-variable* equalities. It is **not** Cooper's algorithm: `Constraint` has
no divisibility variant, so Cooper's central atom is unrepresentable, and there
is no LCM normalisation, B-set, or infinite projection anywhere.

Upstream's spec, module docstring and stats key all say "Cooper"; upstream's own
`## Algorithm` section and the paper say Fourier–Motzkin. **The paper is the
accurate source here and the spec overclaims** — the inverse of the usual
direction.

**Decision:** keep the `L4` label and the `L4:cooper` stats key for wire
compatibility with upstream tooling, and correct the *prose* — spec `018` R4's
description, and any docstring in this workspace repeating the wrong name.
Tracked upstream as [`mvl-lang/mvl`#2022](https://github.com/mvl-lang/mvl/issues/2022).

**Real Cooper is deferred, deliberately.** It would close the ℚ/ℤ gap (§2 bug
class) and the parity cases natively, but it is a rewrite of `is_unsat` rather
than an extension, and finding 3 prices L4's entire contribution at four
obligations in 174. Recorded as a distinct future layer, not a gap in this one.

### 2. Acting on a Fourier–Motzkin `Satisfiable` verdict is a bug, not a layer

FM decides **ℚ**-satisfiability. `FM-UNSAT ⟹ ℤ-UNSAT`, so concluding `Proven`
from UNSAT is sound. The converse is **not**: `2*x >= 1 && 2*x <= 1` is
rationally satisfiable (`x = ½`) with no integer solution, and
`2*x == 2*y + 1` likewise.

`discharge_l4` currently maps `SatOutcome::Satisfiable` to `Proven { L4 }`, so
both of those report **Proven** today. Upstream acts only on UNSAT and is sound.
This is an unsoundness the port introduced, and it violates spec `018`'s own
success criterion ("zero incorrect `Proven`").

**Decision:** `Satisfiable` from FM may never produce `Proven`. Coherence claims
that rest on it must fall to `Runtime`. This is a correctness fix, not a scope
change, and it precedes everything else in this ADR.

### 3. `L5` via feature-gated Z3 is the one capability addition

Per finding 3, L5 is the largest static-coverage win available and the only layer
wholly absent — no dependency, no feature flag, no encoding, and `Layer::L5` is
never constructed.

**Decision:** implement L5 as a **feature-gated** Z3 binding in `mvl-rust-core`,
matching upstream's spec `018` R5 contract: feature off ⇒ `Runtime` immediately
with no build dependency; `unknown` or timeout (1 s default) ⇒ `Runtime`; `unsat`
on the negated obligation ⇒ `Proven { L5 }`.

Two blockers to record, neither in #37's body:
- `SolverBackend`'s `Obligation` carries a re-parsed `String` predicate with no Γ
  field, so that path structurally cannot carry a hypothesis context (ADR-0005
  §Consequences). L5 must sit behind `entail_expr`, not behind the trait.
- `Predicate::Expr` is an arbitrary `syn::Expr`, so the encoder needs its own
  type and overflow story — there is no type information to lean on (ADR-0001).

**L3 path enumeration stays deferred.** This workspace's L3 is bounded-quantifier
expansion; upstream's is path enumeration. They are different mechanisms sharing
a label. At 0.6% of obligations, closing the difference is not worth it now — but
the *prose* must stop claiming path enumeration (ADR-0005 already corrects this;
`solver/mod.rs`'s module doc still needs it).

### 4. Enforcement: active proc macros, `assert!` always, with a per-function opt-out

ADR-0001 §2 makes the attributes inert. That is what has to change for a residual
obligation to mean anything.

**Decision: option A — `#[mvl::requires]`/`#[mvl::ensures]` become active proc
macros that wrap the function body.** `requires` prepends its assertion;
`ensures` binds the body's value and asserts before returning it.

Why this and not the alternatives:

| Option | Rejected because |
|---|---|
| **Source rewriting / "the linter is a formatter"** | Fails soundness **fail-silently** — validity depends on whether someone ran the rewriter, and nothing in the source records it. Non-idempotent, drifts against the attribute it mirrors, and changes the input all five tools read, making the gate a function of invocation order. Upstream precedent is decisive: `mvl fmt` has zero contract awareness, there is no `--fix` anywhere in that toolchain, and the one thing `mvl harden` writes is *tests*. |
| **rustc driver / MIR pass** | Highest ceiling — real types would fix the float and unbounded-ℤ unsoundnesses, and it is the only option reaching call sites, indexing, struct construction and cross-crate resolution. Contradicts ADR-0001 §3's five-binary architecture and needs pinned nightly `rustc_private`. Not a v0.x move. |
| **`build.rs` codegen** | Breaks spans, IDE support and incremental builds; shipped source ≠ compiled source. |

The proc macro is the only option in Rust that reproduces upstream's
load-bearing property — **callee-side, unconditional, all-paths** — and the
machinery already exists: `mvl-macros` is a real `proc-macro` crate whose
attributes are two-line pass-throughs, and the predicate parser is written.
Wrapping the whole body means it covers explicit-`return` paths, which upstream's
own Rust backend provably does not.

**Amendment (#53, review follow-up): "all-paths" excludes the `?` operator.**
`?`'s early return has no `Expr::Return` node for `mvl-macros` to rewrite — only
an `Expr::Try` wrapping the fallible expression — so a function returning early
via `foo()?` is not instrumented. This mirrors `rust-refine`'s own static checker,
which is equally blind to `?`, so no unsound Γ claim results from the gap. It is
still a real, silent hole in runtime enforcement for any function using `?`,
tracked in spec 007's Known Limitations rather than left implicit.

**`assert!`, not `debug_assert!`, and not elidable.** Upstream explicitly
rejected `debug_assert` (#672, with tests asserting its absence), and §5 below
shows why: a check that compiles out under release breaks the assumption the
solver relies on.

**A per-function opt-out attribute is provided**, and exists to resolve the
`#[mvl::total]` collision explicitly rather than by accident. An injected
`assert!` makes a `#[mvl::total]` function panicking; `rust-total` says it is not
(ADR-0003 §Consequences). `assert` is absent from its `PANICKING_MACROS`
allow-list, so it would not even be flagged — an allow-list accident, not
soundness. **Upstream has no such conflict**: its `total` is termination-only and
it has no `Panic` effect, so there is no answer to inherit.

An opted-out function is a **hole Γ must account for** — §5 condition 5.

### 5. Γ's soundness invariant, stated once

> **A fact is admitted to Γ only if it has been established, or is an obligation
> some other program point is required to discharge.**

ADR-0005 §3.3 relies on this and never stated it, which is why it has been
rediscovered one violation at a time (#38 → #40 → #42 → #43/#44 → #45). Stating
it terminates that chain.

**Injection makes propagation sound, but only under five conditions**, each
independently checkable. An assert at the callee's return converts "P holds of
the result" into "either P holds, or the process aborted before the result
existed" — so any execution *reaching* the call site satisfies P, and Γ is sound
for **partial correctness modulo abort**.

1. **Every return path instrumented, not just the tail.** Violated by upstream's
   own Rust backend; satisfiable here by wrapping the whole body.
2. **Present in every build profile.** Not `debug_assert!`, not elidable.
3. **The predicate is runtime-evaluable in the post-state.** Quantifiers and
   ghost state are not. Upstream propagates such predicates into Γ anyway — an
   unpatched instance of this same failure class.
4. **Γ reasons about the value the assert observed** — requires per-name
   invalidation on rebinding. **Already met** (#40).
5. **Every function whose postcondition can enter Γ is instrumented.** Fail-loud
   with a proc macro (unresolved attribute); **fail-silent** with source
   rewriting, which is the decisive argument in §4. An opted-out function (§4)
   fails this condition and must therefore be excluded from propagation.

**Until injection lands, `Runtime` means unenforced everywhere the tool speaks**
— in Γ, in diagnostics, and in the assurance JSON. Concretely, and independent of
injection:
- A postcondition that reaches only `Runtime` must **not** be propagated into a
  caller's Γ (#47's own preferred direction — it reduces what the tool claims).
- The three "inserting a runtime check" diagnostic strings must stop asserting an
  action the code does not take.
- Spec `001-system-overview`:192's MUST is amended to require *reporting* an
  unclosed obligation, since §4's mechanism is the proc macro rather than
  `rust-refine`.

**Injection buys soundness, not the right to keep calling it a proof.** An
obligation closed against a runtime-enforced premise must not print `proven at
L4`. Provenance has to carry the distinction — including in the assurance JSON,
where residuals used to serialise into a struct named
`ProvenObligationRecord`. #56 removed that misnomer: the type is
`ObligationRecord`, and `is_proof()` requires an entailment question *and* a
non-`runtime` layer, so a residual cannot read as proven. The taint this section
asks for — marking an obligation closed *against a runtime-enforced premise* —
is still owed, and is a different thing from the residual itself: it belongs
with #53's enforcement work.

## Consequences

- **Sequencing is forced, and it is not "L5 first".** §5's honesty fixes and §2's
  unsoundness are independent of everything else and precede it: adding proving
  power to a Γ whose soundness condition is unstated makes the audit harder, not
  easier. Order: §5 reporting fixes → §5 invariant + Γ audit (#47) → §2 → §4
  injection → §3 L5.
- **§4 changes the runtime behaviour of existing annotated code.** It breaks
  `crates/mvl/tests/passthrough.rs` by design, and contradicts the facade crate's
  documented "unaffected by whether this crate is even a dependency". ADR-0001 §2
  is amended by this ADR, not merely extended.
- **Enforcement becomes dependent on the `mvl` crate being a dependency.** A user
  who drops it gets an unresolved-attribute compile error — fail-loud, therefore
  acceptable (§5 condition 5).
- **An abort replaces a silent wrong answer.** For this project's target domains
  that is the right trade, but it is a stated decision, not an accident. There is
  no profile in which a Γ-load-bearing check may be elided (§5 condition 2).
- **Quantified predicates cannot be injected** (§5 condition 3). `forall i in
  [lo..hi]` needs lowering to a loop or must be excluded — mirroring upstream's
  `is_runtime_checkable` returning false for quantifiers. Excluded predicates
  must then be excluded from Γ too.
- **Declaration-site obligations have no runtime analogue.** `discharge_predicate`
  asks "is this predicate satisfiable" — there is no program point to assert at.
  Those stay static-only, which is fine: a self-contradictory `requires` is
  already an error.
- **§1 leaves a knowingly misleading stats key.** `L4:cooper` names an algorithm
  that is not implemented, kept for wire compatibility. Anyone reading the
  assurance JSON as evidence of Presburger-complete reasoning is over-reading it,
  and the corrected spec prose is the only thing preventing that.
- **Several Γ-construction unsoundnesses remain and are now in scope of §5's
  audit** rather than being discovered individually: pattern bindings that do not
  invalidate, loop-carried mutation, and arity-mismatch parameter capture in
  postcondition propagation.

## Links

- `mvl-lang/mvl-rust`#37 (L5/Z3 — §3), #45 (purity signal), #47 (Γ invariant —
  §5), #48 (return-point doc invariant)
- `mvl-lang/mvl-rust`#42 (return-site obligations — what made propagation
  checkable at all), #40 (Γ invalidation — §5 condition 4)
- [`mvl-lang/mvl`#2022](https://github.com/mvl-lang/mvl/issues/2022) (`L4`
  naming — §1)
- Upstream spec `018-refinement-solver` (the L1–L5 standard); upstream ADR-0025
  (function contracts), ADR-0055 (atom normalisation), ADR-0056 (bounded
  quantifiers), ADR-0057 (regex membership)
- Spec `001-system-overview` Requirement 3 (the runtime-assertion MUST amended
  by §5)
- ADR-0001 §2 (attribute inertness — amended by §4), ADR-0003 (the
  `#[mvl::total]` collision), ADR-0005 (obligations, Γ, and the layer stack this
  completes)
- Audit source material: `~/wc/my-brain/projects/mvl/refinement-solver-audit.md`
