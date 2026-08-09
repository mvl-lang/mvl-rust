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
mvl's checker has type information and a solver; `rust-total` has neither
(ADR-0001 §5: `syn` only, no `rustc_private`, no name resolution). The
decision here is to close the gap as far as that boundary allows, not to
import mvl's machinery wholesale.

## Decision

### 1. A `decreases` measure must name a parameter directly

`measure` MUST be a bare identifier that resolves to one of the function's
own parameters — `#[mvl::decreases(n)]`, not `#[mvl::decreases(n - m)]` or
`#[mvl::decreases(n.len())]`. Multi-term and computed measures are real and
useful, but proving descent for them needs the same type/value reasoning
`rust-refine`'s solver has and `rust-total` deliberately doesn't (spec 001
Requirement 3: no shared analysis state between tools). Scoping to a bare
parameter keeps this a `syn`-only structural check, matching ADR-0002 rule
3's own reasoning for restricting lifetimes to what a syntactic pass can see.

A measure that isn't a bare parameter identifier is now rejected, not
silently accepted — this is new; presence-only accepted anything that
parsed as an expression.

### 2. Every direct recursive call must pass a recognized descending argument

At the parameter position `measure` names, the call's argument MUST be one
of:

- `param - <positive integer literal>` (e.g. `n - 1`, `n - 2`)
- `param / <integer literal ≥ 2>` (e.g. `n / 2`)

Anything else — the same variable unchanged, `param + 1`, `param - 0`,
a call result, an unrelated expression — is rejected as not provably
decreasing.

This is deliberately the smallest useful shape set, not an attempt at
completeness. It mirrors ADR-0002 rule 4's macro allowlist precedent
("deliberately small, and expected to grow as real use surfaces" more
shapes) rather than trying to anticipate every legitimate measure up front.
`factorial`'s `n - 1` and any binary/Euclidean-style `n / 2` recursion are
covered; anything else names its measure and gets rejected until the shape
set grows to cover it.

### 3. Rejection, not silence, on anything unanalyzable

Spec 003 Requirement 3 is amended from "presence only, MUST NOT be read as
proof" to: **provable descent for the recognized shape set is required**,
and the tool MUST reject a measure or a call site it cannot classify as
descending. This is the opposite safe direction from panic-freedom's
"missing diagnostic is preferable to a wrong one" (ADR-0003 §4) — and
deliberately so: panic-freedom's false-positive cost was flagging *ordinary,
correct* arithmetic (Requirement 2's rationale), where a false rejection
here only lands on a `decreases` clause the tool cannot yet vouch for, which
is exactly the case where silence previously let an actually-non-decreasing
measure through unnoticed. Rejecting the unanalyzable case is the same
"false rejection is the safe direction for a gate" principle ADR-0001 §5
already applies elsewhere (rust-limit's `transmute` rule, rust-refine's
declaration-site coherence check) — this section brings termination in line
with it instead of being the one place still trading soundness for
adoption-friction.

### 4. Still direct self-recursion only

Mutual recursion between two functions stays out of scope, unchanged from
ADR-0003. Proving descent across a mutual-recursion cycle needs a shared
measure argument between two signatures — a bigger step than tightening the
single-function case, and not part of this ADR.

## Consequences

- **This is a breaking change**, by design. Any `#[mvl::decreases(measure)]`
  that previously passed on presence alone and doesn't match §1–§2's shape
  set now fails. `examples/rust-total-demo` and `crates/rust-total/tests/`
  are updated in the same change to reflect the new behavior with a
  genuinely-provable measure (`factorial`'s `n - 1` already qualifies) and a
  new violating fixture for a measure that doesn't decrease.
- **`#[mvl::total]`'s termination claim gets closer to its name**, but is
  still not equivalent to mvl's `total fn`: no mutual recursion, no
  computed measures, no interval/SMT proof for the shapes it does accept
  (a literal-subtraction/division match is a syntactic recognition, not
  numeric reasoning about overflow or negative results). ADR-0003's
  "Relationship to MVL's `total fn`" comparison table is updated for the
  "Recursion proof" row; the remaining gaps stay real and stay documented,
  not implied to be closed.
- **The shape set will need to grow.** Same expectation as ADR-0002 rule 4's
  macro allowlist: real `#[mvl::decreases]` usage will surface legitimate
  measures outside `param - literal` / `param / literal` (e.g. `param - 1`
  chained through a helper, or a second parameter as a tie-breaker), each of
  which is a scoped addition to §2's list, not a reason to fall back to
  presence-only.

## Links

- ADR-0003 §"Termination", §"Relationship to MVL's `total fn`" (the decision
  superseded here, and the "future ADR" pointer this resolves)
- ADR-0001 §5 (imprecise is acceptable, unsound is not; false rejection is
  the safe direction for a gate)
- ADR-0002 §3 (rule 3's lifetime-elision precedent for scoping a check to
  what `syn` can see structurally), §4 (the allowlist-grows-over-time
  precedent for §2's shape set)
- Spec 003 Requirement 3 (amended by this ADR)
- `mvl-lang/mvl`#2233 (`reject-unprovable-decreases` — the equivalent
  decision on mvl's own side, made with a solver `rust-total` doesn't have)
- `5e4d994` (the commit that introduced the presence-only wording as a
  documentation fix, and left convergence as an open question)
- Tests: `crates/rust-total/tests/totality.rs`; examples:
  `examples/rust-total-demo/{compliant,violating}/`
