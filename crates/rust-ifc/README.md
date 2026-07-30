# rust-ifc

IFC label/relabel enforcement for
[mvl-rust](https://github.com/mvl-lang/mvl-rust) (spec Requirement 5, v1
scope): a `Labeled` value (see the [`mvl`](../mvl) crate's `Tainted`/`Secret`)
can only be classified or declassified inside a function whose own
`#[mvl::relabel(from = ..., to = ...)]` attribute declares exactly that
transition.

v1 recognizes only syntactically local, explicit facts — no call graph, no
local-variable dataflow. See the crate's own `checks` module docs for the
exact recognition rules and their deliberate limits.

## Attributes

| Attribute | Meaning |
|---|---|
| `#[mvl::label]` | Declares a new IFC label (lattice point) as a zero-sized marker type. |
| `#[mvl::relabel(from = ..., to = ..., audit)]` | Declares a named, directional transition between two labels (`_` meaning unlabeled/public). |

## Quick example

```rust
use rust_ifc::checks::check_source;

let compliant = r#"
    #[mvl::relabel(from = "Tainted", to = "_", audit)]
    fn trust(value: mvl::Tainted<String>) -> String {
        value.into_inner()
    }
"#;
assert!(check_source(compliant).unwrap().is_empty());

let violating = r#"
    fn leaks(value: mvl::Tainted<String>) -> String {
        value.into_inner()
    }
"#;
assert!(!check_source(violating).unwrap().is_empty());
```

## As a `cargo` subcommand

```bash
cargo mvl ifc src/main.rs
```

## Docs

- [Concept guide: when to use which tool](https://github.com/mvl-lang/mvl-rust/blob/main/docs/overview.md)
- [Full attribute/tool reference](https://github.com/mvl-lang/mvl-rust#the-five-tools)
- [docs.rs](https://docs.rs/rust-ifc) for the API

## License

Apache-2.0
