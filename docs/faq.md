# FAQ

## Is mvl-rust a new language?

No. It's ordinary Rust plus attribute macros
(`#[mvl::total]`, `#[mvl::requires(...)]`, ...) and five `cargo` subcommands
that scan for them. Code without the attributes is unaffected; the
attributes themselves (except `requires`/`ensures`) are no-ops at runtime —
all the actual checking happens in the separate tools, parsing the same
source independently with `syn`.

## Does adding an mvl-rust attribute change my program's behavior?

For `#[mvl::total]`, `#[mvl::decreases]`, `#[mvl::effect]`, `#[mvl::label]`,
and `#[mvl::relabel]` — no. They're pass-through markers; the compiled code
is identical with or without them.

For `#[mvl::requires(pred)]`/`#[mvl::ensures(pred)]` — yes, deliberately:
they inject a real `assert!` at the function's entry (`requires`) or every
return point (`ensures`). A caller that violates the contract at runtime
aborts there rather than proceeding on a broken assumption. This is what
makes the compile-time proof and the runtime behavior agree with each
other — a `#[mvl::requires]` that `rust-refine` can't prove at a given call
site still gets checked, just later, at the callee's boundary.

## What happens if `rust-refine` can't prove my precondition?

It falls through the layered solver (`L1` → `L2` → `L3` → `L4` → optionally
`L5`) to the runtime `assert!` `mvl` already injected. Nothing is silently
accepted, and nothing is silently rejected either — an obligation that
can't be proven compile-time-safe is enforced at runtime instead, and
`cargo mvl prove` will report which layer (if any) actually closed it.

## Do I need Z3 installed?

No, not by default. `rust-refine`'s `L5` (Z3-backed SMT dispatch for
genuinely nonlinear obligations) is a Cargo feature, off by default:

```bash
cargo build                               # no Z3 required, ever
cargo build --features rust-refine/z3     # opts into L5
```

Most obligations that look like they'd need an SMT solver actually close at
`L1`-`L4`, natively, with no external dependency at all.

## Can I use mvl-rust alongside Kani/Creusot/Prusti?

Yes — see the [integration guides](integration/existing-rust.md) for the
specific coexistence notes per tool. In short: attribute names don't collide
because `mvl-rust` is always invoked fully-qualified (`#[mvl::requires]`,
never a bare `use`), and the tools check different, mostly non-overlapping
properties at different costs.

## Which functions does `rust-effect` actually check?

Only calls it can resolve within the same file (spec Requirement 4, v1
scope). A call through a trait object, a different module, or an external
crate is invisible to it — which also means a function with no
`#[mvl::effect(...)]` at all is only a *verified* purity claim within that
same-file boundary; beyond it, it's unverified (tracked by
[issue #67](https://github.com/mvl-lang/mvl-rust/issues/67)).

## Is `cargo mvl mcdc`/`coverage` implemented?

Not yet — both need `cargo-llvm-cov`, an external tool with its own
install/toolchain story, tracked by
[issue #15](https://github.com/mvl-lang/mvl-rust/issues/15).

## Where do I report a bug or ask a question?

[github.com/mvl-lang/mvl-rust/issues](https://github.com/mvl-lang/mvl-rust/issues).
