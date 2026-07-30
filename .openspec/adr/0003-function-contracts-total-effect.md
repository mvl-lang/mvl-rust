---
status: Accepted
date: 2026-07-29
---

# ADR-0003: Function Contracts — `total` and `effect`

## Context

ADR-0001 §1 establishes attributes as the carrier for verification information.
Two of the five tools use that carrier in its simplest form: an attribute on a
function declares a property, and the tool checks the function's body against
it. No hypothesis context, no solver, no cross-procedural state.

This ADR records that shape, because it is the *baseline* the other two
annotation tools deviate from — `rust-refine` needs a solver and a hypothesis
context (ADR-0005), and `rust-ifc` puts its information in types rather than
attributes (ADR-0004). Establishing the simple pattern first makes those
deviations legible as deviations.

Both tools were scoped deliberately small in v1 (issues #6, #9). The scope cuts
are the interesting content here — each is a place where `syn`'s lack of type
information forced a choice between a useless tool and an incomplete one.

## Decision

### 1. The shape: declare on the function, check the body, report locally

```
#[mvl::total]                    → rust-total: this body cannot panic and terminates
#[mvl::decreases(measure)]       → rust-total: measure for a recursive total function
#[mvl::effect(Log, Clock)]       → rust-effect: this body performs at most these effects
```

Each is a `syn::visit::Visit` pass over `ItemFn`. A violation is a
`Level::Error` `Diagnostic` at the offending span. There is **no** shared state
between the two tools, and neither consults the solver.

The property is **checked, not assumed**: unlike `rust-refine`, which propagates
a callee's declared postcondition into a caller's hypothesis context (ADR-0005),
neither tool here treats a declaration as a premise for reasoning elsewhere. A
`#[mvl::total]` claim that fails produces an error; it never becomes a fact
another proof rests on. That is why these two tools have no analogue of
ADR-0006's Γ-soundness problem.

### 2. `#[mvl::total]` — two independent checks

**Panic freedom** (`checks/panic_freedom.rs`). Flags, inside a `#[mvl::total]`
function: `.unwrap()`, `.expect(…)`, `panic!`/`todo!`/`unimplemented!`, raw
indexing (`xs[i]`), and division/modulo.

**Deliberately excluded: general arithmetic overflow.** Without type
information, flagging `+`/`-`/`*` for overflow would flag nearly all numeric
code, making the tool useless. This is a documented v1 gap, not an oversight.

Division and modulo have the *same* syntactic-only limitation — a float divisor
cannot panic, and this cannot tell floats from integers — but are kept in scope
anyway, because `/` and `%` are far rarer in ordinary code than `+`/`-`/`*`, so
the false-positive rate is tolerable. **The dividing line is the false-positive
rate, not a principle**, and it is worth recording as such: a tool that cries
wolf on every addition is not a stricter tool, it is an ignored one.

**Termination** (`checks/termination.rs`). Requires `#[mvl::decreases(measure)]`
on any `#[mvl::total]` function that directly calls itself.

v1 checks **presence only** — it does not prove the measure actually decreases.
Only *direct* self-recursion is detected; mutual recursion between two functions
is out of scope. So `#[mvl::decreases]` is currently a *documentation
obligation with a syntactic trigger*, not a termination proof. Recording this
plainly matters: a reader who assumes otherwise has a false guarantee, and the
attribute's name invites that assumption.

### 3. `#[mvl::effect(…)]` — flat sets, exact matching, same-file only

Two rules:
- A caller must declare every effect its same-file callees declare.
- A pure function must not call an effectful one.

**Absence of the attribute means the empty set**, identical to an explicit
`#[mvl::effect()]`. This mirrors MVL's "no hidden effects" principle: not
declaring an effect is a positive claim of purity, not an absence of
information.

That conflation is load-bearing and has a known cost. It means *unannotated* and
*annotated-as-pure* are indistinguishable — which is exactly why `rust-effect`
cannot be used as the purity oracle `rust-refine`'s reflexivity rule needs (#45,
and ADR-0005 §Consequences).

**ADR-0008 settles what follows from that, and keeps this decision unchanged.**
A tri-state signal is required but not sufficient: an *explicit*
`#[mvl::effect()]` is not a purity licence either, because this section's claim
is checked only against same-file resolvable calls, and effects reach a function
through the routes that are silently unresolvable. So the third state belongs in
the licence reader rather than here — an obligation and a licence have opposite
safe defaults, and this section is the obligation.

v1 scope, deliberately smaller than MVL's own effect system
(`mvl-lang/mvl#846`):
- **Flat, exact-set matching.** No subsumption hierarchy (`effect Log > Clock`).
- **Same-file, free functions only.** A call to anything else is silently
  unresolvable and is not flagged either way — ADR-0001's shared boundary.
- **No effect polymorphism**, no effect variables, no handler discharge.

### 4. Both inherit ADR-0001's scope boundary rather than working around it

Neither tool attempts to compensate for missing type information. Where a
construct is invisible, the tool is silent rather than guessing — ADR-0001 §5's
"imprecise is acceptable, unsound is not", applied here as: **a missing
diagnostic is preferable to a wrong one**, because both tools emit only
`Level::Error` and a false error fails the build on correct code.

## Consequences

- **`#[mvl::total]` is weaker than its name.** It means "contains no *syntactically
  obvious* panic construct and, if directly recursive, carries a `decreases`
  attribute". It does not mean panic-free, and it does not mean terminating.
  Anything reading `total` as a guarantee — including a downstream assurance
  claim — is over-reading it.
- **`#[mvl::partial]` is parsed and unclaimed** (ADR-0001). The natural reading is
  "the dual of `total`", i.e. an explicit opt-out. Nothing implements it. Either
  `rust-total` claims it as an explicit-partiality marker or it should be
  removed.
- **`rust-total` and ADR-0006 collide.** If `#[mvl::requires]`/`#[mvl::ensures]`
  become active proc macros that inject `assert!`, a `#[mvl::total]` function
  carrying a residual refinement obligation *becomes panicking*. `assert` is not
  in `PANICKING_MACROS`, so it would not be flagged — an allow-list accident,
  not soundness. **MVL has no such conflict**: its `total` is termination-only
  and it has no `Panic` effect at all. This conflict is introduced by the port,
  so there is no upstream answer to inherit; ADR-0006 resolves it via a
  per-function opt-out.
- **The division/modulo rule will produce false positives on float code**, by
  construction. Accepted on frequency grounds. If float-heavy code becomes a
  target, this rule needs types and therefore a different architecture.
- **Effects cannot serve as a purity signal for the solver** (§3). #45's
  original design named `rust-effect` as the source of that signal; the
  dependency edge runs the wrong way (`rust-effect` depends on `mvl-rust-core`,
  not the reverse) *and* the two-state model would be insufficient anyway.
- **No cross-procedural effect inference.** A function that calls an
  unresolvable callee (method, cross-file, macro body) may perform arbitrary
  effects while declaring none, with no diagnostic. The subset (ADR-0002)
  narrows this by rejecting `dyn Trait` and unreviewed macros, but does not
  close it — cross-file calls remain legal and invisible.

## Links

- `mvl-lang/mvl-rust`#6 (`rust-total`), #9 (`rust-effect`, v1 scope decision)
- `mvl-lang/mvl-rust`#45 (purity signal — blocked by §3's two-state model)
- [`mvl-lang/mvl`#846](https://github.com/mvl-lang/mvl/issues/846) (upstream
  effect-system epic — the fuller system this is a subset of)
- Spec `001-system-overview` Requirements 2 and 4
- ADR-0001 (annotation model, scope boundary, greenfield rule)
- ADR-0002 (the subset that narrows the unresolvable-callee hole)
- ADR-0004 (information flow — the tool that does *not* use this shape)
- ADR-0005 (refinement — the tool that needs a solver and a hypothesis context)
- ADR-0006 (runtime enforcement — the `#[mvl::total]` collision)
