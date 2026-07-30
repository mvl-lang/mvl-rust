# rust-refine

`#[mvl::requires(pred)]`/`#[mvl::ensures(pred)]` refinement obligations for
[mvl-rust](https://github.com/mvl-lang/mvl-rust). Proves as much as it can at
compile time and falls through to the `mvl`-crate-injected runtime `assert!`
for the rest — never silently accepting what it can't decide.

## Attributes

| Attribute | Meaning |
|---|---|
| `#[mvl::requires(pred)]` | Whole-function precondition, enforced. |
| `#[mvl::ensures(pred)]` | Whole-function postcondition referencing `result`, enforced. |
| `#[mvl::unchecked]` | Opts a function out of `requires`/`ensures` runtime enforcement. |

## The layered solver

A precondition/postcondition is proved by a stack of layers, cheapest first,
each falling through to the next on anything it can't decide:

| Layer | Technique |
|---|---|
| `L1` | Tautology / constant-fold |
| `L2` | Per-variable integer interval containment |
| `L3` | Bounded-quantifier expansion |
| `L4` | Fourier–Motzkin elimination (linear arithmetic) |
| `L5` *(optional, `z3` feature)* | Z3 SMT, `Int`/QF-NIA only |
| `Runtime` | Injected `assert!`, checked when the program actually runs |

`L5` is off by default — `cargo mvl` and plain `cargo build` need no Z3
install. Enable it with `cargo build --features rust-refine/z3` (needs a
system Z3 install; see the workspace `Makefile`'s `test-z3` target).

## Quick example

```rust
use mvl_rust_core::diagnostics::Level;
use rust_refine::checks::check_source;

// A satisfiable obligation is reported at `Level::Note` ("proven at L1"),
// not silently dropped — "compliant" means "no `Level::Error`".
let compliant = r#"
    #[mvl::requires(n > 0)]
    fn positive_only(n: i32) -> i32 { n }

    fn caller() -> i32 { positive_only(5) }
"#;
assert!(check_source(compliant).unwrap().iter().all(|d| d.level != Level::Error));

let violating = r#"
    #[mvl::requires(n > 0)]
    fn positive_only(n: i32) -> i32 { n }

    fn caller() -> i32 { positive_only(-1) }
"#;
assert!(check_source(violating).unwrap().iter().any(|d| d.level == Level::Error));
```

## As a `cargo` subcommand

```bash
cargo mvl refine src/main.rs
cargo mvl prove src/main.rs   # emit the obligation trace as assurance-JSON
```

## Docs

- [Concept guide: when to use which tool](https://github.com/mvl-lang/mvl-rust/blob/main/docs/overview.md)
- [Full attribute/tool reference](https://github.com/mvl-lang/mvl-rust#the-five-tools)
- [ADR-0005/ADR-0006](https://github.com/mvl-lang/mvl-rust/tree/main/.openspec/adr) — the obligation model and layered-solver design
- [docs.rs](https://docs.rs/rust-refine) for the API

## License

Apache-2.0
