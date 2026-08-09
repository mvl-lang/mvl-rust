# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project did not tag per-PR releases before `0.1.3`; the `[0.1.2]` entry
below backfills everything merged while the workspace sat at that version.

## [Unreleased]

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
