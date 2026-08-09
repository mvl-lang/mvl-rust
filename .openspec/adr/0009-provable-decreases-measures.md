---
status: Accepted
date: 2026-08-09
---

# ADR-0009: Provable `decreases` Measures — Closing the Presence-Only Gap

## Context

ADR-0003 §"Termination" shipped `#[mvl::decreases(measure)]` checking the
attribute's **presence** only: a directly-recursive `#[mvl::total]` function
must carry the attribute, but nothing checks that `measure` actually
decreases on any recursive call. Spec 003 Requirement 3 encoded this as a
hard requirement: *"The tool MUST check the attribute's presence only. It
MUST NOT be read as proving the measure decreases."*

That wording was never a decision to stay presence-only. It came from
`5e4d994` ("docs: reconcile `#[mvl::total]`'s claimed vs actual guarantees",
closing #74/#75) — a documentation-honesty pass whose job was to stop the
README/lib.rs/spec overclaiming a proof the v1 code didn't perform. Fixing
the *claim* to match the implementation was the wrong half of the fix; the
implementation should have been brought up to match the claim `total`'s name
already made. The same commit's own ADR-0003 addition says as much:

> "This ADR does not decide whether the two should converge ... that is a
> design question for a future ADR, not a documentation fix."

Convergence was explicitly left open, not rejected. This ADR is that future
ADR, and it decides: **prove it.**

`mvl-lang/mvl` already does the equivalent for its own `total fn` —
mvl#2233 (`reject-unprovable-decreases`) rejects a `decreases` measure its
extractor cannot show strictly decreases, rather than accepting it silently.
mvl's checker has real rustc-level type information and a full solver;
`rust-total` has neither the type information nor a bespoke termination
solver of its own. But it is **not** starting from nothing: ADR-0001's
Consequences record "no type information, in any tool" as a root-cause
boundary (`syn` gives an AST, not types or name resolution — real types
would need `rustc_private` and a pinned nightly, which the five-independent-
`cargo`-subcommand architecture in §3 rules out), and that boundary is about
**rustc types specifically**, not about arithmetic reasoning in general.
ADR-0001 §3 separately names "the solver (`mvl-rust-core`)" as deliberately
*shared* infrastructure, alongside `MvlAttr` and `Diagnostic`/`Level` — "no
shared analysis state between tools" (spec 001 Requirement 3) means tools
don't exchange per-run results with each other, not that each tool needs its
own reasoning engine. `mvl_rust_core::solver::native` already proves linear-
arithmetic claims over bare `syn::Expr` for `rust-refine`'s
`requires`/`ensures` — no rustc types involved — and `rust-total` already
depends on `mvl-rust-core`. An earlier draft of this ADR missed that and
proposed a bespoke, hardcoded shape-matcher (`param - <literal>`,
`param / <literal>`) instead; the decision below routes through the existing
solver rather than duplicating a smaller version of it.

## Decision

### 1. A `decreases` measure must name a parameter directly

`measure` MUST be a bare identifier that resolves to one of the function's
own parameters — `#[mvl::decreases(n)]`, not `#[mvl::decreases(n - m)]` or
`#[mvl::decreases(n.len())]`. This is not a solver limitation; it's a scope
limit on what the obligation being built can talk about. The check compares
one thing: the value passed at the recursive call site against the
parameter's value *at function entry*. A computed measure (`n - m`, a field
access, a method call) would need its own entry-point value captured and
carried across the call, which is a different, larger obligation shape
`rust-refine`'s own `requires`/`ensures` model doesn't build either (spec 003
is call-site/return-site only, no separate "value at entry" binding).
Scoping to a bare parameter keeps the obligation exactly `<call argument> <
<parameter>`, i.e. a direct entailment `mvl_rust_core::solver::native`
already knows how to answer.

A measure that isn't a bare parameter identifier is now rejected, not
silently accepted — this is new; presence-only accepted anything that parsed
as an expression.

### 2. Every direct recursive call must discharge `<argument> < <measure>`

At the parameter position `measure` names, the tool builds the entailment
obligation `<call's argument> < <measure>` and discharges it through
`mvl_rust_core::solver::native::discharge_entailment` — the same native
linear-arithmetic backend (`L1`–`L4`: trivial checks, interval arithmetic,
bounded-quantifier expansion, Fourier-Motzkin elimination) `rust-refine`
already uses for `requires`/`ensures`. The function's own
`#[mvl::requires(...)]` clauses (the `Predicate::Expr` ones; a quantified
`requires` is skipped as a hypothesis, narrowing what can be proved but never
widening it incorrectly) are supplied as hypotheses.

Verified empirically against the actual backend before writing this:

| Goal | Hypotheses | Result |
|---|---|---|
| `(n - 1) < n` | none | `Proven` at `L4` |
| `(fuel - k) < fuel` | `k > 0` | `Proven` at `L4` |
| `(n + 1) < n`, `(n) < n` | none | `Violated`, with a counterexample |
| `(n / 2) < n` | none, **and** `n > 0` | `Runtime` (unprovable either way) |

This is strictly more general than a hardcoded shape list: subtraction of a
positive literal is provable unconditionally (a linear tautology), and
subtraction of a *symbolic* amount is provable given a `#[mvl::requires]`
bound on it — `#[mvl::decreases(fuel)]` with a recursive call passing
`fuel - k`, given `#[mvl::requires(k > 0)]`, is now a real proof, not
something a fixed pattern list could ever recognize. Division and modulo are
outside the solver's linear-arithmetic system entirely — not "sometimes
provable", genuinely unrepresentable — so no hypothesis rescues them; the
`n / 2` row above is `Runtime` regardless of what's supplied. Since
`#[mvl::decreases]` has no runtime-enforcement fallback (unlike
`requires`/`ensures`, ADR-0006), both `Violated` and `Runtime` outcomes are
rejections, not warnings.

### 3. Rejection, not silence, on anything unproven

Spec 003 Requirement 3 is amended from "presence only, MUST NOT be read as
proof" to: **the tool MUST discharge the entailment obligation in §2 and
reject the call if the result is `Violated` or `Runtime`.** This is the
opposite safe direction from panic-freedom's "missing diagnostic is
preferable to a wrong one" (ADR-0003 §4) — deliberately so: panic-freedom's
false-positive cost was flagging *ordinary, correct* arithmetic (Requirement
2's rationale), where a false rejection here only lands on a `decreases`
clause the solver cannot vouch for, which is exactly the case where silence
previously let an actually-non-decreasing measure through unnoticed.
Rejecting the unproven case is the same "false rejection is the safe
direction for a gate" principle ADR-0001 §5 already applies elsewhere
(rust-limit's `transmute` rule, rust-refine's declaration-site coherence
check) — this section brings termination in line with it instead of being
the one place still trading soundness for adoption-friction.

### 4. Still direct self-recursion only

Mutual recursion between two functions stays out of scope, unchanged from
ADR-0003. Proving descent across a mutual-recursion cycle needs a shared
measure argument between two signatures — a bigger step than tightening the
single-function case, and not part of this ADR.

## Consequences

- **This is a breaking change**, by design. Any `#[mvl::decreases(measure)]`
  that previously passed on presence alone and whose descent
  `discharge_entailment` cannot prove now fails — most visibly, any
  division/modulo-based measure, which was never actually justified and is
  now correctly rejected rather than assumed. `examples/rust-total-demo` and
  `crates/rust-total/tests/` are updated in the same change: `factorial`'s
  `n - 1` is proved (no change needed), a new `count_up` fixture demonstrates
  a non-decreasing measure being rejected, and new tests cover a symbolic
  decrement proved via `#[mvl::requires]` and the same decrement rejected
  without it.
- **`rust-total` gains a dependency on `quote`** (already used elsewhere in
  the workspace, e.g. `rust-refine`) to build the `<argument> < <measure>`
  comparison expression from the two already-parsed `syn` nodes.
- **`#[mvl::total]`'s termination claim gets closer to its name**, but is
  still not equivalent to mvl's `total fn`: no mutual recursion, no computed
  measures, and only as much numeric reasoning as `mvl_rust_core::solver`'s
  `L1`–`L4` native backend provides (no `L5`/Z3 dispatch for this check,
  and — same unbounded-ℤ caveat as everywhere else in this solver, ADR-0001
  Consequences — no awareness of the parameter's actual machine width or
  overflow behavior). ADR-0003's "Relationship to MVL's `total fn`"
  comparison table is updated for the "Recursion proof" row; the remaining
  gaps stay real and stay documented, not implied to be closed.
- **Division-based recursion (`n / 2`-style) has no path to acceptance today**
  short of extending the native solver itself (a divisibility atom, or an
  `L5`/Z3 dispatch for this obligation) — not a scoped follow-up like
  ADR-0002 rule 4's macro allowlist, since there's no shape to add to a list;
  the solver would need new reasoning power it doesn't have.

## Links

- ADR-0003 §"Termination", §"Relationship to MVL's `total fn`" (the decision
  superseded here, and the "future ADR" pointer this resolves)
- ADR-0001 §3 (the solver as deliberately-shared infrastructure; "no shared
  analysis state" is about tools not exchanging per-run results, not about
  each tool needing its own reasoning engine), Consequences ("No type
  information, in any tool" — about rustc types, not about the native
  solver's own arithmetic reasoning), §5 (imprecise is acceptable, unsound is
  not; false rejection is the safe direction for a gate)
- ADR-0005 (refinement obligations — the `L1`–`L5` layered dispatcher this
  ADR reuses rather than re-implements)
- Spec 003 Requirement 3 (amended by this ADR)
- `mvl-lang/mvl`#2233 (`reject-unprovable-decreases` — the equivalent
  decision on mvl's own side, made with a solver that also handles
  division/modulo, which this one does not)
- `5e4d994` (the commit that introduced the presence-only wording as a
  documentation fix, and left convergence as an open question)
- Tests: `crates/rust-total/tests/totality.rs`; examples:
  `examples/rust-total-demo/{compliant,violating}/`
