# rust-limit-demo

Demonstrates `rust-limit`'s qualified-subset lint pass (spec Requirement 1)
against a compliant crate and a violating crate.

- `compliant/` — uses only permitted constructs (safe references,
  `Result`/`Option`, no explicit lifetimes beyond `'static`/`'_`, only
  allowlisted macros). `cargo mvl-limit` exits 0 against it.
- `violating/` — demonstrates every forbidden construct: `unsafe` blocks,
  `dyn Trait` (including `Box<dyn Any>`), explicit lifetimes, macros outside
  the curated allowlist, `std::mem::transmute`, and raw address-of
  (`&raw const`/`&raw mut`). `cargo mvl-limit` exits 1 against it, with one
  diagnostic per violation.

Both are standalone crates (excluded from the main workspace via the root
`Cargo.toml`'s `exclude`), since `violating/` is intentionally outside
`rust-limit`'s rules — it isn't broken Rust, just Rust the qualified subset
doesn't permit.

## Try it

From the repository root:

```sh
cargo build -p rust-limit --bin cargo-mvl-limit
./target/debug/cargo-mvl-limit examples/rust-limit-demo/compliant/src/main.rs   # exit 0
./target/debug/cargo-mvl-limit examples/rust-limit-demo/violating/src/main.rs   # exit 1, 9 diagnostics
```
