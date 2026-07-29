---
status: Accepted
date: 2026-07-29
---

# ADR-0004: Information Flow via Types

## Context

ADR-0003 establishes the baseline annotation shape: a property is declared by an
attribute on a function and checked against that function's body. `rust-ifc`
does not fit it, and the difference is not incidental.

Information-flow control asks a question about **values**, not functions. "This
string came from an untrusted source" is a property that has to travel with the
string through every assignment, argument pass and return — not a property of
one function's body. An attribute on a function cannot express it.

Rust already has a mechanism for properties that travel with values: the type
system. So `rust-ifc` puts the label in the type — `Tainted<T>`, `Secret<T>`,
`Labeled<L, T>` — and `rustc` propagates it for free, with no analysis on our
part. What is left for a tool to check is not the flow but the **exceptions to
it**: the points where a label is deliberately added or stripped.

That inverts the tool's job relative to ADR-0003's pattern. `rust-total` and
`rust-effect` check what a body *does*. `rust-ifc` checks that a body's
*declassifications are declared*. The label itself is never checked, because the
type system already enforces it.

## Decision

### 1. The label lives in the type; the attribute is only an override

| Carrier | Role |
|---|---|
| `Tainted<T>`, `Secret<T>`, `Labeled<L, T>` | **the label** — propagated by `rustc`, not by us |
| `#[mvl::relabel(from = "…", to = "…")]` | **a declared exception** — permission to cross a label boundary in this function |

The consequence worth stating plainly: **most of the information-flow guarantee
is delivered by `rustc`, not by `rust-ifc`.** A program that never calls
`::new()` or `.into_inner()` needs no checking at all — the types cannot be
mixed. `rust-ifc` exists exclusively to police the escape hatches.

This is the opposite of ADR-0003's tools, where the attribute is the claim and
the tool does all the work.

### 2. Two recognised boundary crossings, both from purely local facts

**Declassify** — `.into_inner()`. The receiver must be a bare identifier that is
one of the **enclosing function's own parameters**, whose declared type is
`Tainted<T>`/`Secret<T>` or the two-argument `Labeled<L, T>` form.

**Classify** — `::new()`. The call's own path must directly name the label:
`Tainted::new(..)`, `Secret::new(..)`, or `Labeled::<L, _>::new(..)` with an
explicit turbofish.

Every such call must sit inside a function whose `#[mvl::relabel]` declares
exactly that transition.

**No call graph and no dataflow — deliberately, not as a shortcut.** Both
directions are recognised from syntactically-explicit local facts, because §1
means the type system has already done the propagation. Adding dataflow here
would be re-deriving what `rustc` guarantees.

### 3. Recognition is a closed name list, to hold false positives at zero

Only the literal names `Tainted`, `Secret`, `Labeled` are recognised — not "any
single-generic-argument type with an `.into_inner()`". That generalisation would
immediately catch `RefCell`, `Mutex`, `BufWriter` and every other stdlib type
with the same method name.

This is the same reasoning as ADR-0002 §3 but resolved in the **opposite
direction**, and the asymmetry is deliberate:

- `rust-limit` is a **gate** — a false rejection is safe, so it over-matches
  (`transmute` on last path segment).
- `rust-ifc` reports a **violation of a declared policy** — a false accusation of
  illegal declassification is an error on correct code, so it under-matches.

The rule generalises: *over-match where the failure mode is "you must change
your code", under-match where it is "your code is wrong".*

### 4. Label names match the spelling at the recognition site

`relabel`'s `from`/`to` strings must match the label name **exactly as spelled
where it is recognised** — for the built-in aliases that is the alias itself
(`"Tainted"`, not the underlying `TaintedLabel` marker struct); for
`Labeled<L, T>` it is `L`'s own name verbatim.

A string-matched name rather than a resolved type is an ADR-0001 consequence: no
type information, so no way to relate an alias to its marker struct.

## Consequences

- **Three v1 gaps, all in the direction of silence.** A value that becomes
  labeled via an intermediate `let`, a generic helper, or a field access is not
  recognised as a declassification source. A bare `Labeled::new(..)` without
  turbofish does not reveal `L` syntactically. Neither is flagged. Consistent
  with ADR-0001 §5 — but note the direction differs from the other tools' gaps:
  here silence means **a real declassification goes unpoliced**, which is a
  missing check on a security property, not merely a missed proof.
- **The guarantee is conditional on discipline the tool cannot see.** If code
  obtains a labeled value by any route other than a direct parameter — the gaps
  above — the label can be stripped with no diagnostic. So the security claim is
  "declassifications *of parameters* are declared", not "declassifications are
  declared".
- **`#[mvl::label]` is parsed and unclaimed** (ADR-0001). The natural reading —
  attach a label to a type or field declaratively — is precisely what §1 says
  the *type* should do, so it may be genuinely redundant rather than merely
  unimplemented. Decide and remove, or claim it.
- **`rust-ifc` is the only tool whose primary mechanism is the Rust type
  system.** It therefore benefits most from ADR-0002's subset: `dyn Trait` and
  unreviewed macros are exactly the constructs that would let a labeled value
  cross a boundary invisibly, and rules 2 and 4 already reject them.
- **It never visits `ItemFn` for attributes the way ADR-0003's tools do**, so it
  is structurally unaffected by ADR-0006's injection decision — there is no
  predicate to lower to a runtime check. Of the four annotation tools, this is
  the only one runtime enforcement does not touch.
- **Scaling to a real lattice is not an extension of this decision.** Flat,
  string-compared label names with no partial order means no "Secret flows to
  Public is forbidden but Public to Secret is fine" reasoning. A lattice needs
  either resolved types or a declared ordering, and would supersede §4.

## Links

- `mvl-lang/mvl-rust`#10 (`rust-ifc`, v1 scope and the design history for why no
  call graph or dataflow is needed)
- Spec `001-system-overview` Requirement 5
- ADR-0001 (annotation model; the no-type-information boundary §4 works around)
- ADR-0002 (the subset — rules 2 and 4 protect this tool's mechanism directly)
- ADR-0003 (the function-contract shape this tool deliberately does not use)
- ADR-0006 (runtime enforcement — does not apply here, see Consequences)
