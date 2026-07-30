# mvl

Attributes and IFC types so `mvl-rust` annotations resolve on stable
`rustc`. Add this as a dependency and use `#[mvl::...]` attributes on the
functions you want the `mvl-rust` tools ([`rust-limit`](../rust-limit),
[`rust-total`](../rust-total), [`rust-refine`](../rust-refine),
[`rust-effect`](../rust-effect), [`rust-ifc`](../rust-ifc)) to check.

All verification logic lives in those separate `cargo mvl-*` tools, scanning
the same source independently with `syn` — this crate exists only to make
annotated code compile, and (for `requires`/`ensures`) to enforce contracts
at runtime, and to carry the real IFC label types at runtime.

## Attribute reference

| Attribute | Enforced by | Behavior when this crate is present |
|---|---|---|
| `#[mvl::total]` | `rust-total` | No-op pass-through |
| `#[mvl::decreases(measure)]` | `rust-total` | No-op pass-through |
| `#[mvl::requires(pred)]` | `rust-refine` | **Active** — injects `assert!(pred)` |
| `#[mvl::ensures(pred)]` | `rust-refine` | **Active** — injects an assertion at every return point |
| `#[mvl::unchecked]` | `rust-refine` | Opts a function out of `requires`/`ensures` enforcement |
| `#[mvl::effect(list)]` | `rust-effect` | No-op pass-through |
| `#[mvl::label]` | `rust-ifc` | Declares a zero-sized marker type (a lattice point) |
| `#[mvl::relabel(from, to, audit)]` | `rust-ifc` | No-op pass-through (the transition body is your own code) |

Always invoke fully-qualified (`#[mvl::total]`), never via `use` — this is
meant to read like a namespaced built-in (`#[tokio::main]`, `#[rustfmt::skip]`),
not a new keyword.

## Quick example

```rust
let raw: mvl::Tainted<String> = mvl::Labeled::new("from the environment".to_string());
let trusted: String = mvl::trust(raw, "LOG-PATH-001");
assert_eq!(trusted, "from the environment");
```

## Docs

- [Concept guide: when to use which tool](https://github.com/mvl-lang/mvl-rust/blob/main/docs/overview.md)
- [docs.rs](https://docs.rs/mvl) for the full API

## License

Apache-2.0
