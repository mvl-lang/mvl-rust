# cargo-mvl

Single `cargo` subcommand aggregating every installed `mvl-rust` tool.

## Subcommands

```
cargo mvl check <FILE>...        run every Gate tool (limit/total/refine/effect/ifc)
cargo mvl limit|total|refine|effect|ifc <FILE>...   run a single tool

cargo mvl prove <FILE>...        rust-refine's obligation trace, as assurance-JSON
cargo mvl test [-- ARGS]         runs `cargo test`, parses pass/fail/ignored
cargo mvl assurance <FILE>...    aggregates check + prove + test into one report

cargo mvl mcdc|coverage          not yet implemented — see issue #15 (needs cargo-llvm-cov)
```

`check` and the single-tool subcommands fail the build (non-zero exit) on a
`Level::Error` diagnostic. `prove`/`test`/`assurance` never fail the build —
they emit structured JSON evidence for CI dashboards and audit tooling to
consume instead.

## Install

```bash
cargo install cargo-mvl
```

## Docs

- [Concept guide: when to use which tool](https://github.com/mvl-lang/mvl-rust/blob/main/docs/overview.md)
- [Full tool reference](https://github.com/mvl-lang/mvl-rust#the-five-tools)
- [docs.rs](https://docs.rs/cargo-mvl) for the library API

## License

Apache-2.0
