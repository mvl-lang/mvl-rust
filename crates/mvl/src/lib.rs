//! No-op pass-through attribute macros for `mvl-rust`'s attribute grammar.
//!
//! `#[mvl::total]`, `#[mvl::requires(...)]`, `#[mvl::effect(...)]`, and
//! friends aren't real Rust syntax — nothing registers them, so annotated
//! code fails to compile on stable `rustc` ("cannot find attribute `total`
//! in this scope") without this crate. Each attribute here does nothing but
//! return the annotated item unchanged, just enough to make the name
//! resolvable. All actual verification happens separately, via the
//! `cargo mvl-*` subcommands scanning the same source with `syn` (see
//! `mvl-rust-core`, `rust-limit`) — independent of, and unaffected by,
//! whether this crate is even a dependency.
//!
//! Always invoked via a fully-qualified path (`#[mvl::total]`), never via
//! `use` — a `use mvl::total;` import reads as "extending the language,"
//! which undersells that this is meant to feel like a namespaced built-in
//! (the same idiom as `#[tokio::main]` or `#[rustfmt::skip]`), not a new
//! keyword.
//!
//! Attributes attach only at the function-item level, never to individual
//! parameters — Rust's grammar disallows attribute *macros* (as opposed to
//! built-in attributes) in parameter position entirely
//! ("expected non-macro attribute, found attribute macro"), so `requires`/
//! `ensures` reference parameters by their real names and `ensures` uses
//! the fixed identifier `result` for the return value.

use proc_macro::TokenStream;

macro_rules! passthrough_attr {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[proc_macro_attribute]
        pub fn $name(_attr: TokenStream, item: TokenStream) -> TokenStream {
            item
        }
    };
}

passthrough_attr!(
    total,
    "Pass-through for `#[mvl::total]`. See the crate docs."
);
passthrough_attr!(
    partial,
    "Pass-through for `#[mvl::partial]`. See the crate docs."
);
passthrough_attr!(
    decreases,
    "Pass-through for `#[mvl::decreases(measure)]`. See the crate docs."
);
passthrough_attr!(
    effect,
    "Pass-through for `#[mvl::effect(list)]`. See the crate docs."
);
passthrough_attr!(
    refine,
    "Pass-through for `#[mvl::refine(pred)]`. See the crate docs."
);
passthrough_attr!(
    requires,
    "Pass-through for `#[mvl::requires(pred)]`, a whole-function precondition. See the crate docs."
);
passthrough_attr!(
    ensures,
    "Pass-through for `#[mvl::ensures(pred)]`, a whole-function postcondition referencing the fixed `result` identifier. See the crate docs."
);
passthrough_attr!(
    label,
    "Pass-through for `#[mvl::label(l)]`. See the crate docs."
);
passthrough_attr!(
    declassify,
    "Pass-through for `#[mvl::declassify]`. See the crate docs."
);
