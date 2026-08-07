# rust-total-demo

Demonstrates `rust-total`'s panic-risk scan and `decreases`-attribute
presence check (spec 003 Requirements 1 and 3) against a compliant crate
and a violating crate.

- `compliant/` — `factorial` (recursive, with a correct
  `#[mvl::decreases(n)]`) and an exhaustive `match` over a `TrafficLight`
  enum. `cargo mvl-total` exits 0 against it.
- `violating/` — recursive `factorial` missing `#[mvl::decreases(...)]`,
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
./target/debug/cargo-mvl-total examples/rust-total-demo/violating/src/main.rs   # exit 1, 4 diagnostics
```
