# rust-mcdc

Simulated MC/DC for stable Rust. Unbundles classic MC/DC into its two
halves and maps each onto tools this workspace already has:

- **Obligations** (deterministic, the gate): `cargo mvl-mcdc scan` walks
  source with `syn` and extracts every *decision* -- an `if`/`while`
  condition, or a `match` guard -- flattened into its leaf conditions and
  the `&&`/`||` operators joining them. An exhaustive `match` itself is
  recorded as compiler-void: no test obligation, since Rust's exhaustiveness
  check already covers every arm. If that exhaustiveness comes from a `_`
  catch-all rather than every variant being named, the obligation is also
  flagged `wildcard_risk` -- compiler-void still holds (no test can add
  more coverage than the compiler already guarantees), but a `_` arm can
  silently absorb a variant added later with no compiler signal, so it's
  surfaced as a totality-hiding risk distinct from a fully-named match.

Discharge is **tagged tests** (scan → generate → run → harvest): a human
or an LLM session writes the `n + 1` vectors as real tests, tagging each
one `mcdc__<obligation-id>__v<N>` in its name. `cargo mvl-mcdc harvest`
joins `obligations.json` against `cargo test`'s plain-text output and
counts an obligation discharged once `vectors_required` distinct vector
numbers each have a passing tagged test. This is a real reporting tool
against an *already-existing* test suite, once its relevant tests carry
that tag -- not merely a fallback for people who forgot to tag.

Nightly `-Z coverage-options=mcdc` is deliberately not in this loop;
it's an occasional calibration audit against the simulation, never a
gate.

A condition-mutation engine (force each leaf `→true`/`→false`, flip each
`&&`↔`||`, see if a real `cargo test` run fails) also lives in this crate
as a **library-only** capability (`rust_mcdc::mutate`/`discharge`) -- not
wired into any CLI or Makefile target. Re-running the entire test suite
once per mutant, with no per-mutant timeout, was too disruptive for
everyday use against a real codebase. It stays in the crate in case a
future, more targeted use needs it; nothing here calls it today.

## Usage

```bash
# Obligation scan -- deterministic, read-only, safe anywhere.
cargo run --bin cargo-mvl-mcdc -- scan -o target/mcdc/obligations.json src/lib.rs

# Write tests tagged mcdc__<id>__v<N> (run `generate` first to see the
# ids/vector counts), then:
cargo run --bin cargo-mvl-mcdc -- generate --obligations=target/mcdc/obligations.json
cargo run --bin cargo-mvl-mcdc -- harvest --obligations=target/mcdc/obligations.json --run-dir=.
```

Makefile targets wrap all of this: `make mcdc-scan`/`mcdc-generate`/
`mcdc-run`/`mcdc-harvest` (or `make mcdc` for the full pipeline) require
`MCDC_SRC`/`MCDC_RUN_DIR` pointing at the *target* codebase (this
workspace is the tool, not its own default target). `make test-mcdc`
runs this crate's own test suite, same pattern as the other `test-*`
targets.

`cargo mvl mcdc <FILE>...` runs the obligation scan in-process and emits
it as assurance-JSON's `McdcSection`. `harvest`/`generate` need a
`--run-dir` and shell out to `cargo test`, so they're only exposed
through the standalone `cargo-mvl-mcdc` binary, not `cargo-mvl`'s
source-text-only dispatcher.

## Known scope limits

- `if let`/`while let` (and each `let` leaf of a stable-Rust `let`-chain) is
  treated as an opaque single leaf, not decomposed into its own
  sub-conditions — but it still counts as its own decision/leaf toward
  `vectors_required`, same as any other leaf.
- No per-mutant timeout: a mutant that turns a loop guard into `true` can
  block `cargo test` indefinitely.
- One file at a time; no crate-wide obligation index yet (`.openspec/mcdc/index.yaml`,
  proposed in issue #85, is a follow-up).
