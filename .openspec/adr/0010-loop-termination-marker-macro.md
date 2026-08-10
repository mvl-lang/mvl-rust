---
status: Accepted
date: 2026-08-09
---

# ADR-0010: Loop Termination via a Marker Macro, Not an Attribute

## Context

`rust-total`'s termination check (ADR-0009) only looks at direct recursive
function calls. It has no concept of `while`/`loop` at all. Confirmed
empirically before this ADR:

```rust
#[mvl::total]
fn spins_forever() -> u64 {
    let mut n = 0;
    loop {
        n += 1;
    }
}
```

`cargo-mvl-total` accepted this with **zero diagnostics**. An unconditional
`loop`/`while` is at least as common a way real Rust code diverges as direct
self-recursion — arguably more common — so `#[mvl::total]`'s "terminates"
claim was a bigger promise than what was actually checked, and this gap
wasn't recorded anywhere as a Known Limitation; it was simply absent.

`mvl-lang/mvl`'s own `decreases` explicitly covers this case: its CHANGELOG
records *"Loop termination — decreases measure asserted to strictly
decrease each iteration"*, with its own test corpus. MVL's syntax attaches
the measure directly to the loop: `while cond decreases measure { ... }`.

## `#[mvl::decreases(measure)]` cannot do the same thing here

Verified by actually compiling it, not assumed: a real
`#[proc_macro_attribute]` — which `mvl::decreases` is — cannot legally
attach to a `while`/`loop` expression in statement position on stable Rust.

```rust
fn f(mut n: u64) -> u64 {
    #[mvl::decreases(n)]   // error[E0658]: custom attributes cannot be
    while n > 0 {          // applied to expressions (rust-lang/rust#54727)
        n -= 1;
    }
    n
}
```

This needs the unstable `stmt_expr_attributes` feature. `mvl-rust` targets
stable Rust only (`rust-version = "1.85"`), so this is a hard boundary, not
a design preference — the same category of Rust-grammar wall
`mvl-macros`'s own docs already record for parameter-position attributes
(`requires`/`ensures` reference parameters by name rather than attaching to
them directly, for the identical reason).

## Decision

### 1. A function-like macro invocation, not an attribute, names the measure

`mvl::loop_decreases!(measure)`, placed as the loop body's first statement:

```rust
#[mvl::total]
fn countdown(mut n: u64) -> u64 {
    while n > 0 {
        mvl::loop_decreases!(n);
        n -= 1;
    }
    n
}
```

Function-like macro invocations have no attribute-position restriction —
they're ordinary expressions/statements, legal anywhere a statement is.
`loop_decreases!` is a **new, distinct** proc-macro item (`mvl-macros`),
not a second definition of `decreases`: a crate cannot export two
proc-macro items under the same name regardless of kind (`E0428`,
confirmed by compiling it), and the distinct name is also the more honest
one — this is a different mechanism from the attribute, not the same
mechanism spelled differently. It expands to nothing; `rust-total` reads
the macro *invocation*'s argument tokens from source, the same way it
already reads `#[mvl::decreases(measure)]`'s.

### 2. The measure must be a bare identifier, exactly once, unconditionally mutated

Same restriction as ADR-0009 §1, for the same reason: proving anything
about a computed measure needs value/type reasoning this `syn`-only tool
doesn't have.

For provability, the check needs the loop body's *one* mutation of the
measure. Two independent scans decide this:

- A fully recursive walk counts **every** assignment (compound or plain)
  to the measure, anywhere in the body, at any nesting depth — an `if`,
  `match`, or nested loop is caught for free by ordinary recursion, no
  per-construct handling needed.
- A flat scan of the body's own top-level statement list finds the
  assignment that is *unconditional* — a direct statement of the loop
  body, not nested inside anything.

The measure is only accepted if both scans agree on exactly one mutation.
If the recursive count is exactly 1 but the top-level scan finds none, the
one mutation that exists is conditional — rejected, because a mutation
that only sometimes runs is not a sound per-iteration decrease even when
its shape would otherwise qualify. Two or more mutations anywhere reject
outright — composing multiple mutations across a loop body is out of scope
for v1, not something this checker attempts to reason about.

### 3. No operator is special-cased; the solver decides what's provable

Once the one, unconditional mutation is found, its "value after" is built
generically for every operator (`n -= k` → `n - k`, `n &= mask` → `n &
mask`, `n = expr` → `expr` as written) and handed to
`mvl_rust_core::solver::native::discharge_entailment` as the goal
`<value after> < <measure>` — the same native `L1`–`L4` backend ADR-0009
routes recursive descent through, with the function's own
`#[mvl::requires(...)]` clauses as hypotheses. This is a direct reuse of
ADR-0009's design, not a parallel implementation: subtraction of a
literal or a `requires`-bounded symbolic amount is provable; anything
outside the solver's linear-arithmetic fragment (division, bitwise ops,
multiplication) is not, and is rejected the same way ADR-0009 rejects
division for recursive measures.

### 4. Shadowing is rejected, reusing ADR-0009 §5's guard

A measure identifier rebound anywhere inside the loop body has the
identical failure mode ADR-0009 §5 found for recursion: no name
resolution means the check cannot tell a load-bearing shadow from a
harmless reuse of the same name, so both are rejected.
`termination::measure_is_shadowed` is generalized from `&ItemFn` to `&syn::Block`
and shared by both checks rather than duplicated.

### 5. Nested loops and `impl` methods are out of scope for v1

Each loop is checked independently and needs its own `loop_decreases!` —
a nested loop's own measure is unrelated to its enclosing loop's. Loops
inside `impl` methods aren't scanned at all, matching the rest of this
tool: only `ItemFn` is visited (ADR-0001).

## Consequences

- **This closes a real, previously-undocumented gap**, not a hypothetical
  one. Before this ADR, `#[mvl::total]`'s termination claim covered
  direct recursion only; an unconditional `loop {}` or a `while` whose
  condition never changes passed silently.
- **New public API surface**: `mvl::loop_decreases!`, a function-like
  macro alongside the existing attributes. Same "pass-through, no-op"
  contract as every other annotation in `mvl-macros` — it does nothing at
  runtime, and `rust-total` is the only thing that reads its argument.
- **The same solver-fragment ceiling as ADR-0009 applies here too**:
  division/modulo/multiplication-based loop measures have no path to
  acceptance today short of the native solver itself gaining that
  reasoning power — not a scoped follow-up, a solver capability gap.
- **Composing multiple mutations of the same measure is unsupported.** A
  loop that decrements a measure in one branch and increments it in
  another (net-decreasing on average, say) is rejected outright, even if
  a human could argue it terminates. Conservative by design (ADR-0001
  §5); revisiting this needs real per-path reasoning, not a bigger shape
  list.
- **Loops inside `while let`/`for` aren't covered.** Only `syn::ExprWhile`
  and `syn::ExprLoop` are visited. `for` loops over a `Range`/iterator are
  a different, likely more common case (`for i in 0..n`) that terminates
  by construction in ordinary use and arguably needs a different
  treatment entirely (recognizing bounded-range iteration) rather than a
  `loop_decreases!` marker — left for a follow-up rather than forced into
  this shape.

## Links

- ADR-0009 (the entailment-based recursion check this reuses directly:
  same solver call, same requires-derived hypotheses, same shadowing
  guard, same "no operator special-cased" philosophy)
- ADR-0001 §3 (the solver as shared infrastructure), §5 (false rejection
  is the safe direction for a gate)
- `mvl-macros/src/lib.rs`'s own doc on parameter-position attributes
  being impossible for the same underlying grammar reason
  (`stmt_expr_attributes`/attribute-macro restrictions)
- Spec 003 Requirement 6 (this ADR's amendment)
- Tracked: #82
- Tests: `crates/rust-total/tests/totality.rs`; examples:
  `examples/rust-total-demo/{compliant,violating}/`
