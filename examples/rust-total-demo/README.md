# rust-total-demo

Demonstrates `rust-total`'s panic-risk scan and its `decreases`-measure
provability check (spec 003 Requirements 1 and 3, ADR-0009) against a
compliant crate and a violating crate.

- `compliant/` — `factorial` (recursive, with a `#[mvl::decreases(n)]`
  whose recursive call `n - 1` provably decreases) and an exhaustive
  `match` over a `TrafficLight` enum. `cargo mvl-total` exits 0 against it.
- `violating/` — recursive `factorial` missing `#[mvl::decreases(...)]`
  entirely, `count_up` carrying `#[mvl::decreases(n)]` whose recursive call
  passes `n` unchanged (present but not provably decreasing, ADR-0009),
  plus raw indexing, division, and `.unwrap()`. `cargo mvl-total` exits 1,
  with one diagnostic per violation.

Both are standalone crates (excluded from the main workspace via the root
`Cargo.toml`'s `exclude`) — `violating/` is intentionally outside
`rust-total`'s rules, not broken Rust.

## Try it

From the repository root:

```sh
cargo build -p rust-total --bin cargo-mvl-total
./target/debug/cargo-mvl-total examples/rust-total-demo/compliant/src/main.rs   # exit 0
./target/debug/cargo-mvl-total examples/rust-total-demo/violating/src/main.rs   # exit 1, 5 diagnostics
```
