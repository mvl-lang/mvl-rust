---
status: Accepted
date: 2026-07-30
---

# ADR-0008: Purity as a Licence — Tri-State Effects, and Why That Is Not Enough

## Context

L1 reflexivity decides `t == t` structurally: same tree, therefore same value. That inference is invalid for an impure term, and it fails in **both** directions — `span(gen(), gen())` against `requires(lo <= hi)` would be `Proven`, dropping a check that can genuinely fail, and `requires(a != b)` would be `Violated`, a compile error on a valid call. Both reproductions are recorded in #44.

The fix was `is_call_free`: reflexivity fires only on terms containing no calls. It is sound in the direction that matters, and imprecise — a call to a genuinely pure function is refused a conclusion it deserves. #45 asks for the real signal that would lift the restriction.

The obvious source is `#[mvl::effect(…)]`, and #45's first proposal was for `rust-effect` to expose a purity query. That is not implementable, for reasons verified against `main` on 2026-07-30:

| Blocker | Evidence |
|---|---|
| Dependency cycle | `rust-effect` depends on `mvl-rust-core` (`Cargo.toml:15`); `mvl-rust-core` has zero workspace `path` deps by design (spec 001 Requirement 8). |
| Nothing to consult | `rust-effect`'s entire surface is `pub mod checks`. No `is_pure`/`purity`/`pure` function exists in the crate; the per-function effect table is a `HashMap` local to `check_source` and dropped on return. |
| *absent* and *pure* are the same value | ADR-0003 §3: absence of the attribute **is** the empty set. Asserted by `explicit_empty_effect_attr_is_pure`. |
| No context channel to the solver | `Obligation` carries the predicate as a re-parsed `String`; `NativeBackend` is a stateless unit struct; `is_call_free` sits ~4 frames below either entry point behind a shared `classify_clause`. |

The third is the one that matters semantically, and it is genuinely load-bearing rather than an oversight. ADR-0003 §3 makes absence a *positive claim* of purity so that "no hidden effects" is enforceable: if absence meant "unknown", an unannotated function calling an effectful one could not be flagged, and every unannotated function would become a hole in the effect discipline.

So the same annotation is asked for two incompatible things, and #45 proposes splitting the states to resolve it.

**It does not resolve it.** Splitting is necessary and not sufficient, and that is this ADR's substantive finding.

## Decision

### 1. Obligations and licences have opposite safe defaults

This is why one annotation cannot serve both readers, and it is not a vocabulary problem.

`rust-effect` reads the annotation to **impose an obligation**: *declare your effects, or you are claiming purity and I will error if you call something effectful.* For an obligation, defaulting absence to the **strong** claim (pure) is the safe direction — it makes the check stricter and catches more.

`rust-refine` would read it to **grant a licence**: *this term is pure, so I may assume two occurrences have the same value.* For a licence, defaulting to the strong claim is the **unsafe** direction — it grants more than was established.

Same syntactic fact, opposite safe defaults. Any design that reads one annotation for both purposes must therefore state which modality it is in, and cannot inherit the other's default.

### 2. Purity gains a third state, and the states are named for the modality

| annotation | `rust-effect` (obligation) | purity licence |
|---|---|---|
| `#[mvl::effect(Console)]` | effectful | **denied** |
| `#[mvl::effect()]` | pure — empty set | **not granted** (see §3) |
| *absent* | pure — empty set, unchanged | **denied** — unknown, not pure |

The left column is deliberately unchanged. ADR-0003 §3 stands as written, spec 003 Requirement 4 keeps its scenario, and `explicit_empty_effect_attr_is_pure` keeps asserting what it asserts. **The tri-state lives in the licence reader, not in the effect checker** — which also means this ADR requires no change to `rust-effect` at all, and so does not reintroduce the cycle.

`#[mvl::effect(pure)]` is **not** the spelling. `EffectAttr` is an unvalidated list of bare idents, so that parses as the effect set `{"pure"}` — an *effectful* function carrying a user-declared effect named "pure", whose every unannotated caller `rust-effect` would then reject. The original acceptance criteria in #45 had this inverted.

### 3. An explicit `#[mvl::effect()]` is still not a purity licence

The finding that makes the tri-state insufficient. `rust-effect`'s purity claim is checked only against **same-file, resolvable, free-function calls**. Everything else is silently unresolvable — and effects reach a function through exactly those routes.

Verified on 2026-07-30. Both of these are annotated pure, and both return a different value on each call:

```rust
#[mvl::effect()]
fn wall_clock() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

#[mvl::effect()]
fn counter(c: &std::cell::Cell<i64>) -> i64 {
    c.set(c.get() + 1);
    c.get()
}
```

`rust-effect` exits 0 on both. So do `rust-limit` and `rust-total` — **no tool in the workspace closes this gap.** If reflexivity trusted the explicit annotation, `(wall_clock()) == wall_clock()` would be `Proven`, dropping a check that can genuinely fail: precisely the #44 regression, arriving through the *explicit* annotation rather than through absence.

This is a third blocker, independent of the two #45 records, and it is the most fundamental because it undermines the proposed oracle itself rather than the vocabulary around it.

### 4. What a licence would actually require

A sound purity licence needs all four. Only the first is what #45 asks for.

1. **Absence distinguished from declared-empty** — §2, the tri-state.
2. **A declaration whose claim is checkable**, or is accepted as trusted. `#[mvl::effect()]` today is neither: it is a claim the workspace cannot verify and does not mark as unverified. Either the check must see through method and cross-file calls — which is `syn`'s hard boundary under ADR-0001 §4, not a missing feature — or purity must be an explicitly *trusted* assertion, recorded as such in the assurance output the way a residual is (`ObligationRecord.kind`, #56, and ADR-0006 §5's principle that enforcement does not confer the right to call something proven).
3. **A type signal.** Reflexivity is unsound for floats: `x == x` is false for NaN, and `syn` carries no type information. `#[mvl::effect()] fn f() -> f64` clears every purity hurdle and still breaks the rule. Recorded in spec 006 Requirement 2's consequences.
4. **Determinism, which is not the same as effect-freedom.** An effect set describes what a function *does to the world*; reflexivity needs to know that two evaluations *agree*. A function reading an immutable global declares no effect and is not a candidate for observing change; a function returning `HashMap` iteration order declares none either and is. The vocabulary MVL has is about effects; the property reflexivity needs is determinism.

### 5. The gate belongs in `rust-refine`, and it is a rewrite rather than a solver change

When the four conditions above are met, the mechanism follows from what already exists — no new dependency edge and no solver signature change:

- `rust_refine::checks::FnFacts` already resolves same-file free-function callees, already scans attributes with `MvlAttr::try_from_attribute`, and `MvlAttr::Effect` already lives in `mvl-rust-core::attrs`, which `rust-refine` already imports. An `effects` field on `FnFacts` costs nothing structurally.
- Pre-screening happens at the two existing lookup sites in `obligations_for_call` / `propagate_postcondition`.
- A term cleared as pure is **rewritten into a single opaque symbol before the obligation is built** — `substitute_exprs` already does exactly this shape of rewrite. The solver then sees a call-free term and needs no context channel, no new parameter on ~8 functions, and no `Obligation` field surviving re-parse.

`is_call_free` **stays** in `native.rs` as the solver's own floor. `mvl-rust-core` must remain sound standalone: `reflexivity_does_not_fire_on_a_term_containing_a_call` and the declaration-site case assert `Runtime` with no notion of who `f` is, and that must keep holding. The licence is `rust-refine` choosing to present a different term, not the solver relaxing a rule.

### 6. Not scheduled

No implementation is authorised by this ADR. Condition 2 is a research question about `syn`'s boundary or a decision to accept trusted assertions; condition 4 is a semantic gap in MVL's own vocabulary. Both are larger than the imprecision they would remove.

The current behaviour is a runtime check rather than a wrong answer — imprecise, and ADR-0001 §5's standard is that imprecise is acceptable where unsound is not. `is_call_free` stays.

## Consequences

- **`rust-effect` is not the purity oracle, and after this ADR it is not expected to become one.** #45's original framing is closed off rather than deferred. The blocker was never that the query was missing; it is that the claim behind it is unverifiable within `syn`'s view.
- **`#[mvl::effect()]` is a trust boundary that is not currently marked as one.** This is the closest thing to a live defect here: the workspace accepts an unverifiable purity claim and reports nothing about having done so. #56 established that a residual must be visibly residual; the same argument applies to a trusted assertion, and nothing implements it. Worth its own ticket independent of reflexivity.
- **Reflexivity over calls stays disabled indefinitely**, and #45's acceptance criteria are superseded: the criterion "a callee annotated `#[mvl::effect()]` is `Proven`" is **unsound** as written, per §3.
- **Determinism has no vocabulary in MVL.** Surfaced here because reflexivity needs it, but it is a gap in the effect system's expressiveness that will resurface anywhere "same input, same output" is required.
- **The tri-state was described as the prerequisite; it is one of four.** Recording the other three is the point of this ADR — the cheap-looking change would have shipped an unsound licence.

## Links

- #45 (this ADR's subject), #44 (the two reproductions that introduced `is_call_free`), #42 (return-site obligations, which made reflexivity over calls non-hypothetical)
- ADR-0001 §4–§5 (the `syn` scope boundary; imprecise-but-sound), ADR-0002 (qualified subset), ADR-0003 §3 (absence == empty set, and its stated cost), ADR-0005 §2 (coherence vs entailment), ADR-0006 §1 (L1's rules)
- spec 003 Requirement 4 (the effect semantics kept unchanged), spec 006 Requirement 2 (`is_call_free`, and the float consequence)
- Audit: `~/wc/my-brain/projects/mvl/refinement-solver-audit.md` §3 (identity and its permutations)
