# mvl-macros

Internal proc-macro crate: the attribute macros re-exported by
[`mvl`](../mvl). Split out because a `proc-macro = true` crate can only
export proc-macro items, and `mvl` also needs to export ordinary types
(`Tainted`, `Secret`) and functions (`trust`) — the same split
`tokio`/`tokio-macros` uses.

**Don't depend on this crate directly — depend on
[`mvl`](https://docs.rs/mvl) instead.**

## License

Apache-2.0
