# rust-effect

`#[mvl::effect(list)]` propagation checking for
[mvl-rust](https://github.com/mvl-lang/mvl-rust) (spec Requirement 4, v1
scope): a caller must declare every effect its same-file, resolvable callees
declare. A function with no `#[mvl::effect(...)]` at all is a claim of
purity.

> `#[mvl::effect()]` (empty list, i.e. an explicit purity claim) is currently
> unverified where the call graph leaves the file — see
> [issue #67](https://github.com/mvl-lang/mvl-rust/issues/67).

## Attributes

| Attribute | Meaning |
|---|---|
| `#[mvl::effect(list)]` | Declares the comma-separated effect set a function (transitively, within the same file) performs. |

## Quick example

```rust
use rust_effect::checks::check_source;

let compliant = r#"
    #[mvl::effect(Console)]
    fn log(msg: &str) { println!("{msg}"); }

    #[mvl::effect(Console)]
    fn report() { log("hi"); }
"#;
assert!(check_source(compliant).unwrap().is_empty());

let violating = r#"
    #[mvl::effect(Console)]
    fn log(msg: &str) { println!("{msg}"); }

    fn report() { log("hi"); }
"#;
assert!(!check_source(violating).unwrap().is_empty());
```

## As a `cargo` subcommand

```bash
cargo mvl effect src/main.rs
```

## Docs

- [Concept guide: when to use which tool](https://github.com/mvl-lang/mvl-rust/blob/main/docs/overview.md)
- [Full attribute/tool reference](https://github.com/mvl-lang/mvl-rust#the-five-tools)
- [docs.rs](https://docs.rs/rust-effect) for the API

## License

Apache-2.0
