# rust-total

`#[mvl::total]` verifier for [mvl-rust](https://github.com/mvl-lang/mvl-rust):
a syntactic panic-risk scan, a silent-swallow scan for discarded call
results (spec 003 Requirement 7, #117), plus a `decreases`-measure
provability check on direct recursion (spec 003 Requirement 3, ADR-0009)
and on `while`/`loop` (spec 003 Requirement 6, ADR-0010). Only functions
carrying `#[mvl::total]` are scanned; everything else is untouched. This is
still weaker than the name suggests — see
[Known Limitations](https://github.com/mvl-lang/mvl-rust/blob/main/.openspec/specs/003-function-contracts/spec.md#known-limitations)
for what is and isn't proved, including how this differs from MVL's own
`total fn`.

## Attributes

| Attribute | Meaning |
|---|---|
| `#[mvl::total]` | Claims the function is panic-free and terminates. The tool checks for syntactically obvious panic constructs, silent discarding of a call's result (`let _ = <call>;`, `drop(<call>)`, `.map(\|_\| ())`), and, on direct recursion or a `while`/`loop`, that a decreasing measure is present and provably decreases — it does not prove panic-freedom. |
| `#[mvl::decreases(measure)]` | Required on recursive `#[mvl::total]` functions. `measure` must be a bare parameter identifier; every direct recursive call's argument for it must discharge `<argument> < <measure>` as `Proven` through the native linear-arithmetic solver `rust-refine` also uses (subtraction of a literal or a `requires`-bounded amount qualifies; division/modulo never does) — anything unproven is rejected (ADR-0009). |
| `mvl::loop_decreases!(measure)` | Required as the first statement of any `while`/`loop` body in a `#[mvl::total]` function. A **function-like macro**, not an attribute — a real attribute macro cannot legally attach to a loop expression on stable Rust (ADR-0010). Same provability rule as `decreases`, applied to the loop's one, unconditional, top-level assignment of `measure`. |

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
