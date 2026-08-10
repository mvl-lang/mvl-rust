# rust-total-demo

Demonstrates `rust-total`'s panic-risk scan, its `decreases`-measure
provability check on recursion (spec 003 Requirement 3, ADR-0009), and its
`loop_decreases!`-measure provability check on `while`/`loop` (spec 003
Requirement 6, ADR-0010) against a compliant crate and a violating crate.

- `compliant/` — recursion: `factorial` (`#[mvl::decreases(n)]` proved via
  the literal descent `n - 1`), `countdown` (`#[mvl::decreases(fuel)]`
  proved via the *symbolic* descent `fuel - k`, given
  `#[mvl::requires(k > 0)]` as a hypothesis). Loops: `sum_to` (a `while`
  carrying `mvl::loop_decreases!(i)`, literal descent `i -= 1`) and
  `countdown_loop` (the loop analogue of `countdown` — symbolic descent
  `fuel -= k`, provable only given the same `requires` bound). Plus an
  exhaustive `match` over a `TrafficLight` enum. `cargo mvl-total` exits 0
  against it.
- `violating/` — every way a `decreases`/`loop_decreases!` obligation
  currently fails. Recursion: `factorial` missing `#[mvl::decreases(...)]`
  entirely; `count_up` passing its measure unchanged; `shadowed_measure`
  rebinding the measure identifier before recursing, so the goal is
  provable but about the wrong variable (ADR-0009 §5 — this one is real: it
  was accepted with zero diagnostics before that guard existed);
  `unbounded_countdown`, the same symbolic-descent shape as `countdown`
  minus the `requires` bound; `halve`, whose `n / 2` genuinely terminates
  at runtime but sits outside the solver's linear-arithmetic system
  entirely. Loops: `spins_forever` — the headline gap ADR-0010 closes, an
  unconditional `loop { n += 1; }` accepted with zero diagnostics before
  this check existed; `shadowed_loop_measure`, the loop analogue of
  `shadowed_measure`; `unbounded_countdown_loop`, the loop analogue of
  `unbounded_countdown`; `halve_loop`, the loop analogue of `halve`. Plus
  raw indexing, division, and `.unwrap()`. `cargo mvl-total` exits 1, with
  13 diagnostics (two apiece for `halve`/`halve_loop`: division-by-zero
  risk from panic-freedom, and the unprovable measure from termination).
  Every function that's genuinely non-terminating if actually called
  (`count_up`, `shadowed_measure`, `unbounded_countdown`,
  `spins_forever`, `shadowed_loop_measure`, `unbounded_countdown_loop`) is
  declared but not called from `main` (see the file's module doc) —
  `cargo-mvl-total`'s diagnostics come from a static scan, not execution,
  so the crate still builds and runs cleanly.

Both are standalone crates (excluded from the main workspace via the root
`Cargo.toml`'s `exclude`) — `violating/` is intentionally outside
`rust-total`'s rules, not broken Rust.

## Try it

From the repository root:

```sh
cargo build -p rust-total --bin cargo-mvl-total
./target/debug/cargo-mvl-total examples/rust-total-demo/compliant/src/main.rs   # exit 0
./target/debug/cargo-mvl-total examples/rust-total-demo/violating/src/main.rs   # exit 1, 13 diagnostics
```
