# rust-limit-idioms-demo

A single before/after narrative for `rust-limit`'s qualified-subset lint
(spec Requirement 1, ADR-0002), as opposed to `../rust-limit-demo/`'s
exhaustive one-of-each-violation crate.

- `idiomatic/` -- the common Rust fragment: a `Shape` trait with
  `Rectangle`/`Square` impls, dispatched through `Vec<Box<dyn Shape>>`. This
  is textbook, idiomatic, compiling Rust, and it's exactly the construct
  ADR-0002 rule 2 rejects: a `dyn` call site has no single concrete
  signature, so `rust-refine`/`rust-effect` have nothing to attach an
  obligation or effect row to.
- `contracted/` -- the same behavior with `dyn Trait` eliminated: a closed
  `enum Shape` dispatched through `match`. Nothing about the *behavior*
  changed -- same shapes, same perimeters, same total -- but every call site
  is now concrete. That's the payoff, made concrete rather than asserted:
  `rectangle_perimeter`/`square_perimeter` carry a
  `#[mvl::requires]`/`#[mvl::ensures]` pair that `rust-refine` actually
  discharges (linear arithmetic, native L1), which no `dyn Shape` call site
  could ever offer a signature to check against.

Both are standalone crates (excluded from the main workspace via the root
`Cargo.toml`'s `exclude`), for the same reason as `../rust-limit-demo/`:
`idiomatic/` is intentionally outside `rust-limit`'s rules -- it isn't
broken Rust, just Rust the qualified subset doesn't permit.

## Try it

From the repository root:

```sh
cargo build -p rust-limit --bin cargo-mvl-limit
cargo build -p rust-refine --bin cargo-mvl-refine

./target/debug/cargo-mvl-limit examples/rust-limit-idioms-demo/idiomatic/src/main.rs   # exit 1: dyn Trait
./target/debug/cargo-mvl-limit examples/rust-limit-idioms-demo/contracted/src/main.rs  # exit 0

./target/debug/cargo-mvl-refine examples/rust-limit-idioms-demo/contracted/src/main.rs # obligations discharged
```
