# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project did not tag per-PR releases before `0.1.3`; the `[0.1.2]` entry
below backfills everything merged while the workspace sat at that version.

## [Unreleased]

## [0.4.0] - 2026-08-21

### Added

- `rust-mcdc` (#85): sixth tool, simulated MC/DC for stable Rust. `syn`-based obligation scanner extracts every decision (`if`/`while` condition, `match` guard), flattens `&&`/`||` chains into leaf conditions, and computes MC/DC's `n + 1` minimum vector count; an exhaustive `match` is recorded compiler-void. Two independent discharge paths over the same `obligations.json`: mutation (`cargo mvl-mcdc discharge` forces each leaf true/false and flips each operator, checking whether `cargo test` fails — `discharged ⇔ compiler-void ∨ all-condition-mutants-killed`) and tagged tests (`scan → generate → run → harvest`: a human/Claude writes the vectors as tests named `mcdc__<id>__v<N>`, `harvest` joins them against `cargo test`'s plain-text output, no mutation). `cargo mvl mcdc` wires the obligation scan in-process; both discharge paths need `--run-dir` and stay on the standalone `cargo-mvl-mcdc` binary. New Makefile targets (`mcdc`, `mcdc-scan`, `mcdc-generate`, `mcdc-run`, `mcdc-harvest`, `mcdc-discharge`, `test-mcdc`).

## [0.3.1] - 2026-08-21

### Fixed

- `rust-effect` (#67): an explicit `#[mvl::effect()]` is a positive purity claim, but the checker only verifies it against same-file, resolvable, free-function calls. A function with unresolvable calls (method calls, cross-file calls) now gets a `Level::Note` diagnostic saying the claim is unverified rather than staying silent. Also fixes `rust-effect`'s gate mode, which previously failed the build on *any* diagnostic rather than only `Level::Error`, which would have made this new `Note` incorrectly fail passing builds.

## [0.3.0] - 2026-08-10

### Added

- ADR-0010 / spec 003 Requirement 6: `rust-total` now checks `while`/`loop` termination, closing a real gap where an unconditional `loop { n += 1; }` inside a `#[mvl::total]` function was accepted with zero diagnostics. A new function-like macro, `mvl::loop_decreases!(measure)`, names the measure as the loop body's first statement — a real attribute macro (the `#[mvl::decreases(...)]` shape) cannot legally attach to a `while`/`loop` expression on stable Rust. The loop's one, unconditional, top-level assignment to the measure is proved via the same native solver call `#[mvl::decreases]` uses for recursion, with the function's own `#[mvl::requires(...)]` clauses as hypotheses.
- `rust-limit`'s macro allowlist (ADR-0002 rule 4) gained `loop_decreases`, needed for `mvl::loop_decreases!` to pass the qualified-subset gate that runs before every other tool.
- `examples/rust-total-demo` now covers loop termination end to end: `sum_to`/`countdown_loop` (compliant) and `spins_forever`/`shadowed_loop_measure`/`unbounded_countdown_loop`/`halve_loop` (violating).

### Fixed

- `crates/rust-total/src/checks/termination.rs`'s `measure_is_shadowed` generalized to operate on any `syn::Block`, shared with the new loop check rather than duplicated.

## [0.2.0] - 2026-08-09

### Added

- ADR-0009: `#[mvl::decreases(measure)]` now proves descent instead of checking presence. `measure` must be a bare parameter identifier; every direct recursive call's argument for it is discharged as the entailment obligation `<argument> < <measure>` through `mvl_rust_core::solver::native` — the same native `L1`–`L4` linear-arithmetic backend `rust-refine` uses for `requires`/`ensures`. The function's own `#[mvl::requires(...)]` clauses are supplied as hypotheses, so a symbolic decrement can be proved (e.g. `decreases(fuel)` with a call passing `fuel - k`, given `requires(k > 0)`), not just a literal constant.
- A `decreases` measure that is rebound anywhere in the function body (a `let`, closure parameter, match arm, for-loop pattern) is now rejected outright — found by manual edge-case probing after the above: with no name resolution, a shadowed measure could be "proven" to decrease while the function actually never terminates.
- `examples/rust-total-demo` now covers every `decreases` case, good and bad: literal descent, symbolic descent via `requires`, a missing measure, a non-decreasing measure, a shadowed measure, an unbounded symbolic decrement, and division (which terminates at runtime but is never provable). Gained its own scoped `Makefile`.

### Fixed

- Spec 003 and ADR-0003 no longer describe `#[mvl::partial]` as "parsed and unclaimed" — it was removed entirely (not claimed) when #54 closed, and referencing it as a live, if-inert attribute was stale documentation.

### Changed

- This is a deliberate breaking change to `#[mvl::decreases]`'s acceptance criteria (ADR-0009 Consequences): any measure that previously passed on presence alone and isn't provable under the native solver's linear-arithmetic fragment — most visibly, anything division/modulo-based — now fails.

## [0.1.5] - 2026-08-09

### Added

- `examples/rust-limit-idioms-demo/`: a focused before/after narrative for `rust-limit`'s qualified-subset lint (ADR-0002 rule 2) — `idiomatic/` dispatches through `Vec<Box<dyn Shape>>` (rejected), `contracted/` eliminates `dyn Trait` for a closed `enum` + `match` (accepted, and `rust-refine` actually discharges its `requires`/`ensures`).

### Fixed

- `examples/rust-limit-demo/violating`'s `bit_pattern` function silenced rustc's `unnecessary_transmutes` lint (`#[allow(unknown_lints, unnecessary_transmutes)]`) instead of rewriting away the `transmute` call the fixture exists to demonstrate.

## [0.1.4] - 2026-08-07

### Fixed

- `mvl-rust-core`'s schema-stability test can now tell a wire-shape change
  from a doc-comment-only edit: `committed_schema_matches_the_derived_schema`
  compares the derived and committed schemas twice — once with `description`
  fields stripped (a mismatch means a real shape change, bump
  `ASSURANCE_SCHEMA_VERSION`) and once as-is (a mismatch here, only reached
  once the shape check passes, means only doc-comment text drifted —
  regenerate via `bless_committed_schema`, do not bump). Previously the two
  were conflated in one assertion, so any doc-comment edit on a type
  reachable from `AssuranceReport` failed the test with advice to consider
  bumping the version (closes #64).

### Docs

- Per-crate docs.rs polish: every crate gets a `README.md` and a
  runnable doctest against its real API; new prose concept guides
  under `docs/` (overview, integration recipes for Kani/Creusot/Prusti,
  FAQ) written to drop into `mvl-lang.github.io` as a subsite (#73,
  closes #11).
- Qualified `#[mvl::total]`'s "panic-freedom and terminating recursion"
  claim across `README.md`, `crates/rust-total`'s README/lib.rs/Cargo.toml,
  spec 003's Overview, the `rust-total-demo` example README, and the
  `docs/` prose guides (`overview.md`, `integration/kani.md`) — none of
  them now overclaim what the checks actually establish. Fixed the
  ambiguous "spec Requirement 2" citations
  (wrong number for the termination check, and confusable with MVL's own
  Requirement 2) in `rust-total`'s docs and tests. Documented that
  `#[mvl::total]` and MVL's `total fn` are different predicates in both
  directions, in spec 003's Known Limitations and a new ADR-0003 section
  (closes #74, closes #75).

## [0.1.3] - 2026-07-30

### Added

- `rust-refine`: L5 — feature-gated Z3 SMT dispatch for entailments L1–L4
  can't close, narrowed to `Int`/QF-NIA (#72, closes #37).

### Fixed

- L5's Z3 encoding drops an unencodable hypothesis clause instead of
  failing the whole query, matching L1–L4's existing "sound in both
  directions" hypothesis-dropping behavior (#72 follow-up).

## [0.1.2] - 2026-07-23

Initial workspace and the full v0.1 tool suite. Version stayed at `0.1.2`
through all of the below; entries are backfilled from merged-PR history,
listed in merge order.

### Added

- Cargo workspace bootstrap, CI matrix, and shared `mvl-rust-core`
  solver/AST-walker infrastructure (#16, #17).
- `rust-limit` v0.1 — qualified-subset lint pass (#20).
- `rust-total` v0.1 — panic-freedom and termination checks (#25).
- `cargo-mvl` v0.1 — Gate meta-command, then `prove`/`test`/`assurance`
  subcommands (#26, #34).
- Shared assurance-JSON schema and `--emit-assurance-json` emission across
  tools (#28, #29).
- `rust-refine` v0.1 — native L1/L2 obligation dispatch (#30); v0.2 —
  bounded quantifier predicates via L3 expansion (#36); L4 — linear
  arithmetic via Fourier-Motzkin elimination (#39); call-site obligations
  against a hypothesis context Γ (#40, #38); return-site obligations for
  `ensures` (#46, #42); Γ propagation relaxed to accept enforcement (#71,
  #69).
- `rust-effect` v0.1 — same-file effect propagation checking (#32).
- `rust-ifc` v0.1 — local declassification/classification enforcement
  (#33).
- `mvl-macros`: `requires`/`ensures` as active proc macros enforcing
  residual obligations, Phase A (#70, #53).
- ADR-0001 (solver integration story with `mvl-lang/mvl`) and ADR-0008
  (purity as a licence) (#27, #68, #45); ADR set restructured around the
  plugin model (#52); eight specs formalized with scenario-level ISPE
  traceability (#57).

### Fixed

- Fully-qualified `#[mvl::...]` attribute paths now recognized (#24).
- Equality goals proved via L1 reflexivity and an L4 split (#44, #43).
- Fourier-Motzkin `Satisfiable` no longer wrongly yields `Proven` (#61,
  #49).
- A postcondition enters Γ only once its return site has closed (#62,
  #47).
- Γ-construction soundness, and the rationals named explicitly in
  `SatOutcome` (#63, #50, #60).
- Obligation IDs and a coherence/proof conflation (#66, #51, #56).

### Changed

- Assurance dashboard: one meaning per name, three levels, each runnable
  and gated in CI on its own (#58, #59).

### Chore

- Pass-through proc-macro crate so `mvl-rust` attributes compile (#23).
- Three doc/dead-code fixes (#65, #55, #48, #54).
