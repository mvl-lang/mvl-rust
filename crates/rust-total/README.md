# rust-total

`#[mvl::total]` verifier for [mvl-rust](https://github.com/mvl-lang/mvl-rust):
a syntactic panic-risk scan, plus a `decreases`-measure provability check on
direct recursion (spec 003 Requirements 1 and 3, ADR-0009). Only functions
carrying `#[mvl::total]` are scanned; everything else is untouched. This is
still weaker than the name suggests — see
[Known Limitations](https://github.com/mvl-lang/mvl-rust/blob/main/.openspec/specs/003-function-contracts/spec.md#known-limitations)
for what is and isn't proved, including how this differs from MVL's own
`total fn`.

## Attributes

| Attribute | Meaning |
|---|---|
| `#[mvl::total]` | Claims the function is panic-free and (if recursive) terminates. The tool checks for syntactically obvious panic constructs and, on direct recursion, that a `decreases` measure is present and provably decreases — it does not prove panic-freedom. |
| `#[mvl::decreases(measure)]` | Required on recursive `#[mvl::total]` functions. `measure` must be a bare parameter identifier, and every direct recursive call must pass a recognized strictly-decreasing argument for it (`measure - <positive literal>` or `measure / <literal >= 2>`) — anything else is rejected (ADR-0009). |

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
