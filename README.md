# mvl-rust

A second, independent implementation of [MVL](https://mvl-lang.org)'s compile-time
guarantees — expressed as Rust attribute macros plus five `cargo` lint/checker
subcommands, rather than a new language. Existing Rust code stays Rust; you
add attributes to the functions you want checked, and run the tools that
verify what those attributes claim.

Two facets, one workspace:

- **The Gate** — five tools that scan annotated source and fail the build on
  a violation.
- **The Assurance Platform** — the same tools, in `--emit-*-json` mode,
  producing structured evidence (obligation traces, diagnostics) that CI
  dashboards and audit tooling can consume.

## The five tools

| Crate | `cargo mvl` subcommand | Checks | Attribute(s) |
|---|---|---|---|
| [`rust-limit`](crates/rust-limit) | `limit` | Code stays inside the Rust subset the other tools can verify (no `unsafe`, no `dyn Trait`, ...) | — (whole-file) |
| [`rust-total`](crates/rust-total) | `total` | Syntactic panic-risk scan, plus a `decreases`-measure provability check on direct recursion — see [Known Limitations](.openspec/specs/003-function-contracts/spec.md#known-limitations) | `#[mvl::total]` or `#[mvl::partial]` (whole-file, one required per function — ADR-0012), `#[mvl::decreases(measure)]` |
| [`rust-refine`](crates/rust-refine) | `refine` | Preconditions/postconditions hold, proved at compile time where possible | `#[mvl::requires(pred)]`, `#[mvl::ensures(pred)]` |
| [`rust-effect`](crates/rust-effect) | `effect` | A caller declares every effect its callees declare | `#[mvl::effect(list)]` |
| [`rust-ifc`](crates/rust-ifc) | `ifc` | Labeled data is only declassified through a declared transition | `#[mvl::label]`, `#[mvl::relabel(from, to, audit)]` |

[`cargo-mvl`](crates/cargo-mvl) is the meta-command wiring all five together
(`cargo mvl check`), plus assurance-JSON emission (`prove`, `test`,
`assurance`). [`mvl`](crates/mvl) is the small facade crate that makes the
attributes above resolve on stable `rustc` and carries the IFC runtime types;
[`mvl-macros`](crates/mvl-macros) and [`mvl-rust-core`](crates/mvl-rust-core)
are internal — depend on `mvl`, not on those directly.

## Quick start

```bash
cargo install cargo-mvl
cargo mvl check src/main.rs
```

Add attributes to the functions you want checked, then re-run. Most tools are
opt-in per function or per attribute; `rust-limit` (the qualified-subset
lint) and `rust-total` (panic-freedom/termination) are both whole-file —
`rust-total` requires every function to explicitly declare `#[mvl::total]`
or `#[mvl::partial]` (ADR-0012), so adopting it means annotating every
function in a scanned file, not just the ones you want checked.

## Docs

- [`docs/overview.md`](docs/overview.md) — when to reach for which tool
- [`docs/integration/existing-rust.md`](docs/integration/existing-rust.md) —
  adopting mvl-rust incrementally in a codebase that predates it
- [`docs/integration/`](docs/integration) — coexistence with Kani, Creusot,
  Prusti
- [`docs/faq.md`](docs/faq.md)
- Per-crate docs.rs pages (linked from each crate's own README) for API
  details and runnable examples
- [`.openspec/README.md`](.openspec/README.md) — vision, workspace layout,
  specs, and ADRs (the design source of truth this README summarizes)
- [mvl-lang.org](https://mvl-lang.org) — the MVL language itself

## License

Apache-2.0
