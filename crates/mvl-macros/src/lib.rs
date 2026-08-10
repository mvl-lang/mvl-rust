//! Attribute macros re-exported by the `mvl` crate — see that crate's docs
//! for the full picture (usage convention, why this crate exists, IFC
//! types). Split out because a `proc-macro = true` crate can only export
//! proc-macro items; `mvl` needs to also export ordinary types (`Tainted`,
//! `Secret`) and functions (`trust`), so those live in `mvl` itself, which
//! re-exports these attributes alongside them — the same split
//! `tokio`/`tokio-macros` uses. **Don't depend on this crate directly;
//! depend on [`mvl`](https://docs.rs/mvl).**
//!
//! Attributes attach only at the function-item level, never to individual
//! parameters — Rust's grammar disallows attribute *macros* (as opposed to
//! built-in attributes) in parameter position entirely
//! ("expected non-macro attribute, found attribute macro"), so `requires`/
//! `ensures` reference parameters by their real names and `ensures` uses
//! the fixed identifier `result` for the return value.
//!
//! # Two kinds of attribute, and only two
//!
//! **`requires` and `ensures` are active** (#53): they inject a runtime
//! `assert!`. Annotated code no longer compiles to the same thing as
//! unannotated code, and **this amends ADR-0001 §2**, which made every
//! attribute inert. See the `inject` module for the mechanism and ADR-0006 §4–§5 for
//! why enforcement — rather than reporting alone — is what makes Γ sound.
//!
//! **Everything else stays a pass-through.** `total`, `decreases`, `effect`,
//! `label` and `relabel` are read by tools that report; none of them has a
//! runtime obligation to enforce. They discard their argument tokens exactly
//! as before.
//!
//! The consequence worth stating plainly: enforcement now requires the `mvl`
//! crate to actually be a dependency. ADR-0001 §2 promised the facade was "a
//! convenience, not a requirement", and for `requires`/`ensures` that is no
//! longer true. It fails **loud** rather than silent — without the
//! dependency the attribute does not resolve and the crate does not compile,
//! so nobody can believe they have enforcement they do not have. That
//! property is exactly why ADR-0006 §4 chose proc macros over source
//! rewriting, whose failure mode is a silently un-rewritten file.

mod inject;

use mvl_rust_core::attrs::Predicate;
use proc_macro::TokenStream;
use quote::quote;
use syn::ItemFn;

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

/// Pass-through for `mvl::loop_decreases!(measure)`, `rust-total`'s marker
/// for a `while`/`loop` termination measure (spec 003 Requirement 6,
/// ADR-0010).
///
/// A **function-like** macro, not an attribute, and deliberately not named
/// `decreases` (that name is already `#[proc_macro_attribute]` above, and a
/// crate cannot export two proc-macro items under the same name regardless
/// of kind — `E0428`). The distinct name is also the honest one: unlike
/// `#[mvl::decreases(measure)]` on a function, this can't be an attribute on
/// the loop it measures at all. A real `#[proc_macro_attribute]` cannot
/// legally attach to a `while`/`loop` expression in statement position on
/// stable Rust — confirmed by compiling it — so the measure is instead
/// named by a marker macro *invocation* as the loop body's first statement,
/// which has no such restriction:
///
/// ```
/// # fn f(mut n: u64) -> u64 {
/// while n > 0 {
///     mvl_macros::loop_decreases!(n);
///     n -= 1;
/// }
/// # n }
/// ```
///
/// (Real usage is `mvl::loop_decreases!(measure)` — this doctest lives in
/// the crate that actually defines the macro, per the module doc's split
/// with `mvl`, hence the unqualified crate name above.)
///
/// Expands to nothing — `rust-total` reads the macro *invocation*'s
/// argument tokens from source (the same way it already reads
/// `#[mvl::decreases(measure)]`'s), never this expansion.
#[proc_macro]
pub fn loop_decreases(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}
passthrough_attr!(
    effect,
    "Pass-through for `#[mvl::effect(list)]`. See the crate docs."
);
passthrough_attr!(
    label,
    "Pass-through for `#[mvl::label]`, declaring a new IFC label (lattice point). See the crate docs."
);
passthrough_attr!(
    relabel,
    "Pass-through for `#[mvl::relabel(from = ..., to = ..., audit)]`, declaring a named IFC label transition. See the crate docs."
);

/// Marks a function as deliberately **not** enforcing its own contract:
/// `#[mvl::requires]`/`#[mvl::ensures]` on it inject nothing.
///
/// This exists to make one specific decision explicit rather than
/// accidental. An injected `assert!` can abort, so on a `#[mvl::total]`
/// function it collides with a promise of panic-freedom. ADR-0003 resolves
/// that by reading `total` as *total on its domain* — an assert firing means
/// the caller broke the contract, so it is outside the promised domain — and
/// that is the default. `unchecked` is for the cases where an author needs
/// the other answer.
///
/// # Interaction with Γ
///
/// ADR-0006 §5 condition 5 requires every function whose postcondition can
/// enter Γ to be instrumented, so an opt-out looks like it should cost
/// proving power at call sites. **Today it costs nothing**, and the reason is
/// worth stating precisely rather than assuming the pessimistic case.
///
/// `rust-refine` propagates a callee's postcondition only when every one of
/// its return sites was discharged *statically* (#47). A statically proven
/// postcondition holds whether or not an assert is also present, so
/// propagating it from an `unchecked` function is sound. Instrumentation is
/// not yet load-bearing for Γ anywhere.
///
/// That changes the moment the gate is relaxed to accept "is instrumented" as
/// grounds for propagation — the follow-up to #53. At that point `unchecked`
/// functions **must** be excluded from propagation, or condition 5 is
/// violated outright.
///
/// # Attribute order
///
/// Works in either position relative to `requires`/`ensures`, which needs two
/// mechanisms because attribute macros expand **outside-in** and each one
/// consumes itself:
///
/// - Written *above* them, this expands first and strips them from the item,
///   so they never expand.
/// - Written *below* them, they expand first and find this still in the
///   attribute list — see `is_unchecked`.
///
/// One mechanism alone silently fails for the other order. An author should
/// not have to know expansion order to get an opt-out that works, and a
/// silently ineffective opt-out is the worst possible failure here: the
/// author believes the assert is gone and it is not.
#[proc_macro_attribute]
pub fn unchecked(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let Ok(mut function) = syn::parse::<ItemFn>(item.clone()) else {
        // Not a function item: leave it alone rather than erroring. The
        // attribute is meaningless here, and `requires`/`ensures` will
        // produce the real diagnostic.
        return item;
    };
    function
        .attrs
        .retain(|attr| !path_names_one_of(attr.path(), &["requires", "ensures"]));
    quote!(#function).into()
}

/// Whether `path`'s last segment matches one of `names`.
///
/// Matched on the **last segment**, not [`syn::Path::get_ident`] — real usage
/// is the fully-qualified `#[mvl::requires]`/`#[mvl::ensures]`/
/// `#[mvl::unchecked]`, and `get_ident` returns `None` for any multi-segment
/// path, so a naive `get_ident` check would silently miss every real
/// occurrence.
fn path_names_one_of(path: &syn::Path, names: &[&str]) -> bool {
    path.segments
        .last()
        .is_some_and(|segment| names.iter().any(|name| segment.ident == name))
}

/// `#[mvl::requires(pred)]` — a whole-function precondition, **enforced**.
///
/// Injects `assert!(pred)` as the first statement of the body, so a caller
/// that violates it aborts at the callee's entry rather than proceeding into
/// a function whose assumptions do not hold. See the crate docs for why this
/// is an `assert!` and not a `debug_assert!`.
#[proc_macro_attribute]
pub fn requires(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand(attr, item, inject::inject_requires)
}

/// `#[mvl::ensures(pred)]` — a whole-function postcondition referencing the
/// fixed `result` identifier, **enforced**.
///
/// Injects an assertion at every point the function produces its value —
/// the tail expression *and* every explicit `return` — with `result` bound
/// to the produced value.
#[proc_macro_attribute]
pub fn ensures(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand(attr, item, inject::inject_ensures)
}

/// Shared plumbing: parse the item, parse the predicate, apply `inject`.
///
/// A malformed predicate becomes a compile error at the attribute's own
/// span. That is a deliberate change from the pass-through era, where the
/// tokens were discarded unparsed and a typo was silently accepted — the
/// author got no verification and no warning either.
fn expand(
    attr: TokenStream,
    item: TokenStream,
    inject: fn(&mut syn::Block, &Predicate),
) -> TokenStream {
    // Parsed first so a predicate error below can still emit the function
    // alongside the diagnostic: the item itself is fine, and every other
    // call site in the crate should not also error with "cannot find
    // function" on top of the one real typo.
    let mut function = match syn::parse::<ItemFn>(item) {
        Ok(function) => function,
        Err(err) => return err.to_compile_error().into(),
    };

    let predicate = match syn::parse::<Predicate>(attr) {
        Ok(predicate) => predicate,
        Err(err) => {
            let compile_error = err.to_compile_error();
            return quote!(#compile_error #function).into();
        }
    };

    // Handles `#[mvl::unchecked]` written *below* this attribute, where it
    // is still in the list when this expands. The above-case is handled by
    // `unchecked` itself stripping these attributes -- see its docs for why
    // both mechanisms are needed.
    if is_unchecked(&function) {
        return quote!(#function).into();
    }

    inject(&mut function.block, &predicate);
    quote!(#function).into()
}

/// Whether the function carries `#[mvl::unchecked]` (in any spelling).
fn is_unchecked(function: &ItemFn) -> bool {
    function
        .attrs
        .iter()
        .any(|attr| path_names_one_of(attr.path(), &["unchecked"]))
}
