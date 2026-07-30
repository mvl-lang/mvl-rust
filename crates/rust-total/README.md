# rust-total

`#[mvl::total]` verifier for [mvl-rust](https://github.com/mvl-lang/mvl-rust):
panic-freedom and terminating recursion (spec Requirement 2). Only functions
carrying `#[mvl::total]` are scanned; everything else is untouched.

## Attributes

| Attribute | Meaning |
|---|---|
| `#[mvl::total]` | Promises the function is panic-free and (if recursive) terminates. |
| `#[mvl::decreases(measure)]` | Names the expression that must strictly decrease on every recursive call — required on recursive `#[mvl::total]` functions. |

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
