# rust-mcdc

Simulated MC/DC for stable Rust. Unbundles classic MC/DC into its two
halves and maps each onto tools this workspace already has:

- **Obligations** (deterministic, the gate): `cargo mvl-mcdc scan` walks
  source with `syn` and extracts every *decision* -- an `if`/`while`
  condition, or a `match` guard -- flattened into its leaf conditions and
  the `&&`/`||` operators joining them. An exhaustive `match` itself is
  recorded as compiler-void: no test obligation, since Rust's exhaustiveness
  check already covers every arm.

Discharge has **two independent paths** over the same `obligations.json`:

- **Mutation** (fully automatic): `cargo mvl-mcdc discharge` forces each
  leaf `→true`/`→false` and flips each `&&`↔`||`, against a real `cargo
  test` run. A mutant is *killed* when the suite fails, *survived* when
  it doesn't.

  ```
  discharged ⇔ compiler-void ∨ all-condition-mutants-killed
  ```

  A condition-stubbing mutant is killed iff some test depends on that
  condition's value -- MC/DC's independence criterion, demonstrated
  empirically rather than proven statically.

- **Tagged tests** (scan → generate → run → harvest): a human or an LLM
  session writes the `n + 1` vectors as real tests, tagging each one
  `mcdc__<obligation-id>__v<N>` in its name. `cargo mvl-mcdc harvest`
  joins `obligations.json` against `cargo test`'s plain-text output and
  counts an obligation discharged once `vectors_required` distinct
  vector numbers each have a passing tagged test. No mutation, no
  re-running -- just a JSON join, trusting the tag rather than deriving
  independence empirically.

Nightly `-Z coverage-options=mcdc` is deliberately in neither loop; it's
an occasional calibration audit against the simulation, never a gate.

## Usage

```bash
# Obligation scan -- deterministic, read-only, safe anywhere.
cargo run --bin cargo-mvl-mcdc -- scan -o target/mcdc/obligations.json src/lib.rs

# Path 1: mutation discharge -- mutates src/lib.rs on disk one mutant at a
# time, restoring it after each `cargo test` run (and unconditionally on
# drop). Run only against a working tree you're fine losing mid-mutant to
# a killed process.
cargo run --bin cargo-mvl-mcdc -- discharge --run-dir=. --min-decisions=90 --min-conditions=80 src/lib.rs

# Path 2: tagged-test discharge -- write tests tagged mcdc__<id>__v<N>
# (run `generate` first to see the ids/vector counts), then:
cargo run --bin cargo-mvl-mcdc -- generate --obligations=target/mcdc/obligations.json
cargo run --bin cargo-mvl-mcdc -- harvest --obligations=target/mcdc/obligations.json --run-dir=.
```

Makefile targets wrap all of this: `make mcdc-scan`/`mcdc-generate`/
`mcdc-run`/`mcdc-harvest` (or `make mcdc` for the full tagged-test
pipeline), `make mcdc-discharge` for the mutation path -- all require
`MCDC_SRC`/`MCDC_RUN_DIR` pointing at the *target* codebase (this
workspace is the tool, not its own default target). `make test-mcdc`
dogfoods the mutation path against `rust-mcdc`'s own source instead,
alongside the other `test-*` targets.

`cargo mvl mcdc <FILE>...` runs the obligation scan in-process and emits
it as assurance-JSON's `McdcSection`. Both discharge paths need a
`--run-dir` and shell out to `cargo test`, so they're only exposed
through the standalone `cargo-mvl-mcdc discharge`/`harvest` binary, not
`cargo-mvl`'s source-text-only dispatcher.

## Known scope limits

- `if let`/`while let` chains are treated as an opaque single leaf, not
  decomposed.
- No per-mutant timeout: a mutant that turns a loop guard into `true` can
  block `cargo test` indefinitely.
- One file at a time; no crate-wide obligation index yet (`.openspec/mcdc/index.yaml`,
  proposed in issue #85, is a follow-up).
