//! No-op pass-through attribute macros, re-exported by the `mvl` crate —
//! see that crate's docs for the full picture (usage convention, why this
//! crate exists, IFC types). Split out because a `proc-macro = true` crate
//! can only export proc-macro items; `mvl` needs to also export ordinary
//! types (`Tainted`, `Secret`) and functions (`trust`), so those live in
//! `mvl` itself, which re-exports these attributes alongside them — the
//! same split `tokio`/`tokio-macros` uses. Don't depend on this crate
//! directly; depend on `mvl`.
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
    decreases,
    "Pass-through for `#[mvl::decreases(measure)]`. See the crate docs."
);
passthrough_attr!(
    effect,
    "Pass-through for `#[mvl::effect(list)]`. See the crate docs."
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
    "Pass-through for `#[mvl::label]`, declaring a new IFC label (lattice point). See the crate docs."
);
passthrough_attr!(
    relabel,
    "Pass-through for `#[mvl::relabel(from = ..., to = ..., audit)]`, declaring a named IFC label transition. See the crate docs."
);
