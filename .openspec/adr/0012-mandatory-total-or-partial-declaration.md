---
status: Accepted
date: 2026-08-23
---

# ADR-0012: `mvl-total` Requires an Explicit `#[mvl::total]` or `#[mvl::partial]` Declaration, Whole-File

## Context

#117 asked for `mvl-total` to grow two new checks (panic totality, silent-swallowing) beyond its existing termination check. While scoping that work (spec 003 Requirement 7, this workspace's own #117 implementation), a pre-existing architectural gap surfaced that the new checks would otherwise inherit unchanged:

`rust-total`'s scanning has always been **opt-in per function** (ADR-0001 §1, spec 003 Requirement 1): only functions carrying `#[mvl::total]` are scanned at all; an unannotated function is invisible to the tool, with zero diagnostics either way. Spec 003's own Known Limitations already names the consequence: *"Requirement 6's marker macro is unenforced if simply omitted-and-not-caught by a reviewer... This is opt-in by design (ADR-0001), not a defect specific to loops."* A reviewer who forgets to annotate a function gets silence, not a warning — the exact failure mode a totality checker exists to prevent.

Separately, spec 003's Known Limitations already documents that mvl-rust's `#[mvl::total]` is **not the same predicate** as upstream mvl's own `total fn`: *"mvl's `total` is termination-only and opt-out (`partial` is the escape hatch)."* Upstream mvl has exactly two states for every function — `total` (the default) or `partial` (the explicit opt-out) — never a third, silent, unannotated state. mvl-rust's opt-in model has no analogue of mvl's `partial` at all, and its default (unannotated = unchecked) doesn't correspond to either of mvl's two states.

The question raised during #117's scoping: should mvl-rust's `total` finally close this gap and adopt the same two-state shape mvl already has?

## Decision

**Yes. `rust-total` now requires every `fn` item and `impl` method in a scanned file to carry exactly one of `#[mvl::total]` or `#[mvl::partial]`.** There is no longer a third, silent state:

- **Neither attribute present** is a hard error: `"function must be explicitly declared #[mvl::total] or #[mvl::partial]"`.
- **Both present** is a hard error: `"function cannot be both #[mvl::total] and #[mvl::partial]"`.
- **`#[mvl::total]`** gets exactly the checks it always has (panic-freedom, termination, swallow — spec 003 Requirements 1, 3, 6, 7), narrowable via `--check` (#117 Phase 3) exactly as before.
- **`#[mvl::partial]`** is the new, explicit escape hatch: none of the three checks run against it. Behaviorally identical to today's "unannotated" case for that one function, but now a declared, grep-able choice instead of a default a reviewer can miss.

Scanning changes from "find every `#[mvl::total]`-annotated function" to **whole-file**: every `fn` item and every `impl` method is visited and required to make one of the two declarations, matching how `rust-limit`'s whole-file, non-opt-in model already works (ADR-0002) — `rust-total` was the outlier among the pipeline's Gate-mode tools, not the norm.

**This is the tool's only mode — not gated behind a flag, not rolled out gradually.** Considered and rejected: shipping this behind an opt-in flag (e.g. `--require-declaration`) so existing callers wouldn't break on upgrade. Rejected because the premise doesn't hold here — a project either runs `cargo mvl-total` or it doesn't. There is no meaningful "running it, but with the old silent gap" middle state worth preserving as a permanent option; that middle state *is* the bug ADR-0001's Known Limitations already flagged. A flag would let the gap survive indefinitely as an unadvertised default, which is the opposite of the fix's purpose. Consistent with this workspace's own convention (project memory: mvl-rust code is generated to conform, never grandfathered in from legacy code) — adopting mvl-total means adopting its current rules, not the rules as they stood before you ran it.

`#[mvl::partial]` is a genuine, permanent, first-class declaration (mirroring upstream mvl), not a deprecated migration shim to be removed later.

## Consequences

- **Breaking change**, both for `mvl-rust`'s own dogfooding (crates using `#[mvl::total]` today may have other functions in the same file with neither attribute, previously invisible and now erroring) and for any external adopter. Per this repo's semver convention, this warrants at least a minor version bump; the CHANGELOG entry must call it out explicitly as requiring a one-time pass over existing files to add `#[mvl::partial]` to every function not claiming totality.
- **`#[mvl::partial]` is new attribute-grammar surface**: `mvl-rust-core::attrs::MvlAttr::Partial` / `PartialAttr` (parsed identically to `TotalAttr` — no arguments), and a new pass-through `#[proc_macro_attribute]` in `mvl-macros` re-exported from the `mvl` facade, alongside `total`.
- **No change to what `#[mvl::total]` itself means or checks.** Every existing spec 003 requirement (1, 2, 3, 6, 7) and their tests are untouched in substance — only which functions the tool considers "in scope for a declaration" changed, from opt-in-by-annotation to mandatory-for-every-function.
- **Closes the specific gap named in spec 003's Known Limitations** (Requirement 6's marker-macro omission risk) — for the whole tool, not just loops: an omitted declaration is now a build-breaking diagnostic, not silence.
- **`rust-limit` and `rust-total` now share the same scanning shape** (whole-file, not opt-in) — worth noting in any future doc describing the pipeline's five tools uniformly, since ADR-0001 §1 previously described `rust-total`/`rust-effect`'s "simplest form" as attribute-gated in a way that no longer holds for `rust-total` specifically. `rust-effect`'s own opt-in-by-omission-means-pure model (spec 003 Requirement 4) is untouched by this ADR and is not analogous — absence there is a meaningful claim (purity), not a silent gap.
- **Example crates and this workspace's own demo fixtures** (`examples/rust-total-demo/`) need every function annotated one way or the other; anything currently relying on an unannotated helper function passing silently will now fail until updated.

## Links

- #117 (the ticket whose scoping surfaced this gap and drives the implementation)
- ADR-0001 §1 (the attribute-carrier pattern this amends for `rust-total` specifically), §5 (greenfield rule — no back-compat shim)
- ADR-0002 (`rust-limit`'s whole-file, non-opt-in model, now mirrored by `rust-total`)
- ADR-0003 (spec 003's origin; documents the pre-existing `total`/`partial` split in upstream mvl this ADR brings mvl-rust's `total` into alignment with)
- ADR-0009, ADR-0010 (Requirements 3 and 6, whose "opt-in by design" limitation this ADR closes)
