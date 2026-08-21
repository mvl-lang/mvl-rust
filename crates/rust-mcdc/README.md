# rust-mcdc

Simulated MC/DC for stable Rust. Unbundles classic MC/DC into its two
halves and maps each onto tools this workspace already has:

- **Obligations** (deterministic, the gate): `cargo mvl-mcdc scan` walks
  source with `syn` and extracts every *decision* -- an `if`/`while`
  condition, or a `match` guard -- flattened into its leaf conditions and
  the `&&`/`||` operators joining them. An exhaustive `match` itself is
  recorded as compiler-void: no test obligation, since Rust's exhaustiveness
  check already covers every arm.

- **Discharge** (empirical, the evidence): `cargo mvl-mcdc discharge`
  mutation-tests each decision -- forcing each leaf `→true`/`→false` and
  flipping each `&&`↔`||` -- against a real `cargo test` run. A mutant is
  *killed* when the suite fails, *survived* when it doesn't.

```
discharged ⇔ compiler-void ∨ all-condition-mutants-killed
```

A condition-stubbing mutant is killed iff some test depends on that
condition's value -- MC/DC's independence criterion, demonstrated
empirically rather than proven statically. Nightly `-Z
coverage-options=mcdc` is deliberately *not* in this loop; it's an
occasional calibration audit against the simulation, never a gate.

## Usage

```bash
# Obligation scan -- deterministic, read-only, safe anywhere.
cargo run --bin cargo-mvl-mcdc -- scan src/lib.rs

# Mutation discharge -- mutates src/lib.rs on disk one mutant at a time,
# restoring it after each `cargo test` run (and unconditionally on drop).
# Run only against a working tree you're fine losing mid-mutant to a
# killed process.
cargo run --bin cargo-mvl-mcdc -- discharge --run-dir=. --min-decisions=90 --min-conditions=80 src/lib.rs
```

`cargo mvl mcdc <FILE>...` runs the obligation scan in-process and emits
it as assurance-JSON's `McdcSection`. Mutation discharge needs a
`--run-dir` and shells out to `cargo test` per mutant, so it's only
exposed through the standalone `cargo-mvl-mcdc discharge` binary, not
`cargo-mvl`'s source-text-only dispatcher.

## Known scope limits

- `if let`/`while let` chains are treated as an opaque single leaf, not
  decomposed.
- No per-mutant timeout: a mutant that turns a loop guard into `true` can
  block `cargo test` indefinitely.
- One file at a time; no crate-wide obligation index yet (`.openspec/mcdc/index.yaml`,
  proposed in issue #85, is a follow-up).
