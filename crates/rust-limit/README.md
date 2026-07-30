# rust-limit

Qualified-subset lint pass for [mvl-rust](https://github.com/mvl-lang/mvl-rust):
rejects Rust constructs outside the subset the other four tools can verify
(spec Requirement 1). Whole-file, not attribute-gated — every other tool
assumes this check has already passed.

Rejected: `unsafe`, `dyn Trait`, explicit lifetimes beyond `'static`/`'_`,
`transmute`, and any macro outside a small allowlist.

## Quick example

```rust
use rust_limit::lints::check_source;

let compliant = r#"
    fn checked_div(n: i32, d: &i32) -> Option<i32> {
        if *d == 0 { return None; }
        Some(n / *d)
    }
"#;
assert!(check_source(compliant).unwrap().is_empty());

let violating = "unsafe fn danger() {}";
assert!(!check_source(violating).unwrap().is_empty());
```

## As a `cargo` subcommand

```bash
cargo mvl limit src/main.rs
```

## Docs

- [Concept guide: when to use which tool](https://github.com/mvl-lang/mvl-rust/blob/main/docs/overview.md)
- [Full attribute/tool reference](https://github.com/mvl-lang/mvl-rust#the-five-tools)
- [docs.rs](https://docs.rs/rust-limit) for the API

## License

Apache-2.0
