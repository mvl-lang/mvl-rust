# mvl-rust: which tool do I want?

`mvl-rust` is five independent `cargo` subcommands plus one attribute crate.
Each tool checks one guarantee, opts in via one attribute, and fails the
build (or reports, in assurance mode) on its own. You don't need all five —
add the ones that check something you actually want checked.

## Decision guide

| I want to guarantee... | Tool | Attribute | Nothing to add? |
|---|---|---|---|
| My code stays in the subset the other tools can verify at all | [`rust-limit`](https://docs.rs/rust-limit) | — (whole-file) | Runs on every function, no opt-in |
| This function has no obvious panic risk, and (if recursive) carries a decreases measure whose descent is proved by the native linear-arithmetic solver `rust-refine` also uses — see [Known Limitations](https://github.com/mvl-lang/mvl-rust/blob/main/.openspec/specs/003-function-contracts/spec.md#known-limitations) | [`rust-total`](https://docs.rs/rust-total) | `#[mvl::total]`, `#[mvl::decreases(measure)]` | |
| A precondition/postcondition holds — proved at compile time where possible, enforced at runtime otherwise | [`rust-refine`](https://docs.rs/rust-refine) | `#[mvl::requires(pred)]`, `#[mvl::ensures(pred)]` | |
| A caller can't forget to declare an effect its callees perform | [`rust-effect`](https://docs.rs/rust-effect) | `#[mvl::effect(list)]` | |
| Tainted/secret data is only declassified through a declared transition | [`rust-ifc`](https://docs.rs/rust-ifc) | `#[mvl::label]`, `#[mvl::relabel(...)]` | |

`rust-limit` is the odd one out: it's not opt-in per function, because the
other four tools' proofs assume the code they're looking at is already
inside the subset they can reason about (no `unsafe`, no `dyn Trait`, no
non-`'static`/`'_` explicit lifetimes). Run it first, and run it on
everything.

## Two facets, same tools

Every tool works in two modes:

- **Gate** — `cargo mvl check` / `cargo mvl <tool>` — fails the build on a
  violation. This is what you wire into CI as a required check.
- **Assurance** — `cargo mvl prove` / `assurance` — the same analysis, but
  emitting structured JSON evidence (which obligations exist, how each one
  was discharged, at which layer) instead of failing anything. This is what
  you wire into an audit trail or a compliance dashboard — it never blocks a
  merge on its own.

## Start here if...

- **You have an existing Rust codebase and want to try this incrementally** —
  see [Adopting mvl-rust in an existing codebase](integration/existing-rust.md).
- **You already use Kani, Creusot, or Prusti** — see the coexistence recipes
  under [Integration](integration/existing-rust.md); mvl-rust and those
  tools check different (and mostly non-overlapping) things, and there's no
  reason to pick just one.
- **You want the compile-time-provable subset without any of the runtime
  cost** — start with `rust-limit` + `rust-total`, both fully static, no
  injected runtime checks.
- **You're deciding whether refinement types are worth the effort** — read
  `rust-refine`'s own docs on the layered solver; most obligations that look
  like they'd need an SMT solver actually close natively, well before you'd
  need to turn on the optional Z3 backend.
