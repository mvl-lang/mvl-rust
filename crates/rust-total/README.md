# rust-total

`#[mvl::total]`/`#[mvl::partial]` verifier for
[mvl-rust](https://github.com/mvl-lang/mvl-rust): a syntactic panic-risk
scan, a silent-swallow scan for discarded call results (spec 003
Requirement 7, #117), plus a `decreases`-measure provability check on
direct recursion (spec 003 Requirement 3, ADR-0009) and on `while`/`loop`
(spec 003 Requirement 6, ADR-0010).

**Every `fn` item and `impl` method must carry exactly one of
`#[mvl::total]` or `#[mvl::partial]` (ADR-0012, #117)** — scanning is
whole-file, not opt-in. Neither present is a build-breaking error demanding
an explicit declaration; both present is a build-breaking error too. This
is still weaker than the name suggests — see
[Known Limitations](https://github.com/mvl-lang/mvl-rust/blob/main/.openspec/specs/003-function-contracts/spec.md#known-limitations)
for what is and isn't proved, including how this differs from MVL's own
`total fn`.

## Attributes

| Attribute | Meaning |
|---|---|
| `#[mvl::total]` | Claims the function is panic-free and terminates. The tool checks for syntactically obvious panic constructs, silent discarding of a call's result (`let _ = <call>;`, `drop(<call>)`, `.map(\|_\| ())`), and, on direct recursion or a `while`/`loop`, that a decreasing measure is present and provably decreases — it does not prove panic-freedom. |
| `#[mvl::partial]` | The explicit opposite of `#[mvl::total]` (ADR-0012, #117): opts a function out of every check above. Required on any function not claiming totality — there is no unannotated third state. |
| `#[mvl::decreases(measure)]` | Required on recursive `#[mvl::total]` functions. `measure` must be a bare parameter identifier; every direct recursive call's argument for it must discharge `<argument> < <measure>` as `Proven` through the native linear-arithmetic solver `rust-refine` also uses (subtraction of a literal or a `requires`-bounded amount qualifies; division/modulo never does) — anything unproven is rejected (ADR-0009). |
| `mvl::loop_decreases!(measure)` | Required as the first statement of any `while`/`loop` body in a `#[mvl::total]` function. A **function-like macro**, not an attribute — a real attribute macro cannot legally attach to a loop expression on stable Rust (ADR-0010). Same provability rule as `decreases`, applied to the loop's one, unconditional, top-level assignment of `measure`. |

## CLI

```
cargo mvl-total [--report=human|json|sarif] [--check=panic,termination,swallow] <FILE>...
```

`--check` restricts which of the three checks run against `#[mvl::total]`
functions (default: all). Names are `panic` (Requirement 1), `termination`
(Requirements 3 and 6, covering both recursion and `while`/`loop`), and
`swallow` (Requirement 7). An unrecognized name is a usage error (exit code
2), not a silent no-op. It does not affect Requirement 8's mandatory
`total`/`partial` declaration check, which always runs regardless of
`--check`.

`--report` selects the output format (default: `human`, Gate mode — fails
the build on any violation). `json` emits the project's own assurance-JSON
schema (spec Requirement 14); `sarif` emits a minimal SARIF 2.1.0 log for
CI tools that consume that format (e.g. GitHub code scanning). Both `json`
and `sarif` are non-gating "views" of the same analysis Gate mode runs —
they always exit `0`. `--emit-verification-json` remains a supported alias
for `--report=json`.

## Quick example

```rust
use rust_total::checks::check_source;

let compliant = r#"
    #[mvl::total]
    #[mvl::decreases(n)]
    fn factorial(n: u64) -> u64 {
        if n == 0 { 1 } else { n * factorial(n - 1) }
    }
"#;
assert!(check_source(compliant).unwrap().is_empty());

let violating = r#"
    #[mvl::total]
    fn boom() { panic!("not panic-free") }
"#;
assert!(!check_source(violating).unwrap().is_empty());
```

## As a `cargo` subcommand

```bash
cargo mvl total src/main.rs
```

## Docs

- [Concept guide: when to use which tool](https://github.com/mvl-lang/mvl-rust/blob/main/docs/overview.md)
- [Full attribute/tool reference](https://github.com/mvl-lang/mvl-rust#the-five-tools)
- [docs.rs](https://docs.rs/rust-total) for the API

## License

Apache-2.0
