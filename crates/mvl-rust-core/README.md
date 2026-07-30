# mvl-rust-core

Internal shared infrastructure for [mvl-rust](https://github.com/mvl-lang/mvl-rust)'s
tool crates: the attribute grammar, `syn`-based AST walker, diagnostic
emission layer, and the layered solver backing `rust-refine`'s obligation
proofs.

**Not meant to be depended on directly:**

- Writing code annotated with `#[mvl::...]` attributes? Depend on
  [`mvl`](https://docs.rs/mvl).
- Consuming a checker as a library (e.g. calling `check_source` directly)?
  Depend on that tool crate (e.g. [`rust-refine`](../rust-refine)) — none of
  this crate's types are re-exported as part of those crates' own public API
  (spec Requirement 8), so relying on them here is relying on an
  implementation detail with no stability guarantee.

## License

Apache-2.0
