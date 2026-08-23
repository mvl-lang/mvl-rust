---
status: Accepted
date: 2026-08-23
---

# ADR-0011: A Sound Purity Licence for the "Resolved-Pure Closure" Subset

## Context

ADR-0008 ("Purity as a Licence") closed #45 as superseded: lifting `is_call_free`'s reflexivity gate needs four conditions, and ADR-0008 §6 left it **deliberately unscheduled**, because two of the four — condition 2 (a checkable claim, or one explicitly trusted-and-marked) and condition 4 (determinism, "a gap in MVL's own vocabulary") — looked too large relative to the imprecision removed.

#67 shipped after ADR-0008 (v0.3.1), unrelated to reflexivity: `rust-effect` now tracks `unresolved_calls` per function and flags an explicit `#[mvl::effect()]` as unverified when that count is `> 0`. Issue #103 asked whether this closes conditions 2 and 4 for a narrow subset, as a side effect rather than by original design. This ADR records the review (#103) and the resulting licence.

## The claim, reviewed

**"`#[mvl::effect()]` AND `unresolved_calls == 0`, on a file that has already passed both `rust-limit`'s and `rust-effect`'s gates, is a checkable, deterministic-by-construction purity claim.**"

Verified against ADR-0008 §3's own two counterexamples — `wall_clock()` (calls `SystemTime::now()`, a multi-segment path, unresolved) and `counter()` (calls `Cell::get`/`set`, method calls, unresolved) — both are caught by `unresolved_calls > 0` on the function itself, no recursion needed.

Four attacks were run against the claim (#103):

1. **Interior mutability without a method call.** The only safe-Rust channel is `unsafe` (`static mut`, raw pointer writes). `rust-limit` rejects `unsafe` in every form (blocks, `unsafe fn`, `unsafe impl`, `unsafe trait`). Every tool in this workspace already assumes `rust-limit` gated the file first (`cargo mvl check`'s fixed order) — this licence relies on the same standing assumption, not a new one. Macro invocations are a separate, pre-existing blind spot (opaque to `syn`), unrelated to this proposal.
2. **Recursion / call chains.** Verified empirically: a 3-function chain and a resolved-pure recursive function all resolve cleanly through `rust-effect`'s existing `CallVisitor`, independent of declaration order.
3. **Does `rust-refine` need to re-derive recursive purity itself?** No — found during review that `rust-effect`'s own propagation check already guarantees, as a precondition of the file passing its gate with zero errors, that every same-file call target of an `#[mvl::effect()]`-declared function is itself effect-empty. `rust-refine` only needs, per call site: the callee is `#[mvl::effect()]`, the callee's own `unresolved_calls == 0`, and the file's `rust-effect` gate passed clean. No transitive re-verification needed.
4. **Does this reintroduce #44?** No. The #44 reproduction (`span(gen(), gen())`) uses an *unannotated* `gen()`; the licence only ever pre-clears a call whose callee carries an explicit `#[mvl::effect()]`. `native.rs`'s `is_call_free` and the entailment tests pinning it are untouched — this licence is a `rust-refine`-side term rewrite applied *before* the solver runs, never a solver change.

## Decision

### The licence

A same-file call `g(...)` inside a function `f` may be treated as pure — rewritten into a single opaque symbol via `substitute_exprs` before the obligation reaches the solver (ADR-0008 §5's mechanism, unchanged) — when **all** of:

1. The file has already passed `rust-limit`'s qualified-subset gate (no `unsafe`) and `rust-effect`'s propagation gate (zero error diagnostics). This is `cargo mvl check`'s existing fixed order (`limit → total → refine → effect → ifc`); a standalone `cargo mvl-refine` invocation on ungated source is **not** covered by this licence and must not apply it.
2. `g` carries an explicit `#[mvl::effect()]` (declared-empty, not absent — ADR-0008 §2's tri-state table is unchanged; absence is never read as a licence).
3. `g`'s own `unresolved_calls == 0`, computed the same way `rust-effect`'s `CallVisitor` computes it (single-segment `Expr::Path` resolution against same-file functions; any `Expr::MethodCall` or multi-segment/unresolvable `Expr::Call` counts against it).
4. `g`'s declared return type, read syntactically from its signature (no inference), is not `f32`/`f64` — reflexivity is unsound for floats (`x == x` is false for NaN) regardless of purity, and `syn` carries no type information to check otherwise. Denying the licence for any return type this syntactic check can't rule out as non-float is the conservative direction.

Determinism (ADR-0008 §4's condition 4) is not separately re-verified — it follows from conditions 1-3 by construction: a function meeting them can only be built from literals, arithmetic, comparisons, and calls to other functions meeting the identical property, with condition 1 closing off every other channel to external or mutable state.

### What this does not change

- ADR-0008 §2's tri-state table (`absent` stays a *denied* licence, unchanged).
- `is_call_free` and `native.rs`'s standalone soundness — this licence never touches the solver; it changes what term `rust-refine` hands it.
- `rust-effect`'s own semantics or defaults (ADR-0003 §3 stands as written).
- Call resolution scope — still same-file, free functions only, per the existing boundary every tool in this fan-out shares.

### Implementation

Implemented in #110: `rust_refine::checks::FnFacts` gained `effect_pure`/`unresolved_calls`/`float_return` fields (the last two filled by a second collect-then-walk pass over `find_obligations`'s already-two-pass structure, mirroring `rust-effect`'s `CallVisitor`), and `obligations_for_call`/`propagate_postcondition` now rewrite a licensed call into an opaque symbol (keyed on the call's own token text, so two occurrences of the same call converge on the same symbol) before the built predicate is stored. The invocation-order precondition is documented rather than code-enforced — `rust-refine`'s `check_source`/`find_obligations` have no channel to receive "the other gates already ran," and threading one in would be new cross-tool coupling this workspace has otherwise avoided (ADR-0001 §3); the module doc on `rust_refine::checks` and the standalone binary's usage text both carry the warning instead.

## Consequences

- Reflexivity over calls becomes provable for a real, if narrow, class of same-file pure helper functions — the first time ADR-0008's four conditions are jointly satisfied for any subset.
- The licence is **conditional on tool invocation order**. A workspace or CI setup that runs `rust-refine` standalone, without first running `rust-limit` and `rust-effect`, must not apply it. This is a new, explicit cross-tool dependency that didn't exist before (each tool was previously safe to run in any order or in isolation, per ADR-0001 §3's "no shared analysis state") — worth flagging clearly in any implementation's own doc comments and in `cargo mvl check`'s usage text.
- `rust-refine` needs its own `unresolved_calls`-equivalent computation. Since its call-site resolver is already a deliberate copy of `rust-effect`'s collect-then-walk shape (ADR-0008 §5), the two should agree by construction if the copy stays faithful — but as copied rather than shared code, a future edit to one resolver could silently drift from the other. Worth a shared-helper extraction or an explicit cross-tool equivalence test in the implementation ticket, not resolved here.
- The float exclusion (condition 4 above) is syntactic and conservative — a function returning a generic or type-aliased float won't be caught, understated rather than overstated safety. Acceptable per ADR-0001 §5 (imprecise, not unsound).

## Links

- #103 (this ADR's subject, and the review it records)
- #45 (superseded by ADR-0008, not reopened by this ADR — a different, narrower claim)
- #67 (the shipped fix this licence leans on)
- #44 (the reproductions this licence must not, and does not, reintroduce)
- ADR-0008 (the design this addends), ADR-0001 §3/§5 (no shared analysis state; imprecise-but-sound), ADR-0002 (the qualified subset `rust-limit` enforces)
