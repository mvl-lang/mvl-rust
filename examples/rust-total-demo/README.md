# rust-total-demo

Demonstrates `rust-total`'s panic-risk scan and its `decreases`-measure
provability check (spec 003 Requirements 1 and 3, ADR-0009) against a
compliant crate and a violating crate.

- `compliant/` — `factorial` (recursive, `#[mvl::decreases(n)]` proved via
  the literal descent `n - 1`), `countdown` (`#[mvl::decreases(fuel)]`
  proved via the *symbolic* descent `fuel - k`, given
  `#[mvl::requires(k > 0)]` as a hypothesis — the case a fixed shape list
  could never recognize, ADR-0009 §2), and an exhaustive `match` over a
  `TrafficLight` enum. `cargo mvl-total` exits 0 against it.
- `violating/` — every way a `decreases` obligation currently fails, one
  function each: `factorial` missing `#[mvl::decreases(...)]` entirely;
  `count_up` passing its measure unchanged; `shadowed_measure` rebinding
  the measure identifier before recursing, so the goal is provable but
  about the wrong variable (ADR-0009 §5 — this one is real: it was
  accepted with zero diagnostics before that guard existed);
  `unbounded_countdown`, the same symbolic-descent shape as `countdown`
  minus the `requires` bound, so the solver can't rule out a
  non-decreasing `k`; and `halve`, whose `n / 2` genuinely terminates at
  runtime but sits outside the solver's linear-arithmetic system entirely,
  so it's never provable regardless of hypotheses. Plus raw indexing,
  division, and `.unwrap()`. `cargo mvl-total` exits 1, with one diagnostic
  per violation (two for `halve`: division-by-zero risk from
  panic-freedom, and the unprovable measure from termination).
  `count_up`/`shadowed_measure`/`unbounded_countdown` are never actually
  terminating and so are declared but not called from `main` (see the
  file's module doc) — `cargo-mvl-total`'s diagnostics come from a static
  scan, not execution, so the crate still builds and runs cleanly.

Both are standalone crates (excluded from the main workspace via the root
`Cargo.toml`'s `exclude`) — `violating/` is intentionally outside
`rust-total`'s rules, not broken Rust.

## Try it

From the repository root:

```sh
cargo build -p rust-total --bin cargo-mvl-total
./target/debug/cargo-mvl-total examples/rust-total-demo/compliant/src/main.rs   # exit 0
./target/debug/cargo-mvl-total examples/rust-total-demo/violating/src/main.rs   # exit 1, 9 diagnostics
```
