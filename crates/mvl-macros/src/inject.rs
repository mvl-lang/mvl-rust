//! Lowering a [`Predicate`] to a runtime assertion, and placing it in a
//! function body (ADR-0006 §4, #53).
//!
//! # Why an assertion at all
//!
//! An assert at the callee's return converts "P holds of the result" into
//! "either P holds, or the process aborted before the result existed". Any
//! execution *reaching* the call site therefore satisfies P, which is what
//! makes Γ sound for **partial correctness modulo abort** — and what lets
//! ADR-0005 §3.3's postcondition propagation stop being an unbacked
//! assumption.
//!
//! # `assert!`, never `debug_assert!`
//!
//! ADR-0006 §5 condition 2: the check must be present in *every* build
//! profile. A `debug_assert!` compiles out under `--release`, which would
//! make the soundness argument above hold only in debug builds — the worst
//! possible shape for it, since the release build is the one that ships.
//! Upstream `mvl-lang/mvl` reached the same conclusion (#672, with tests
//! asserting `debug_assert`'s absence); [`tests/no_debug_assert.rs`] is the
//! equivalent here.
//!
//! [`tests/no_debug_assert.rs`]: https://github.com/mvl-lang/mvl-rust
//!
//! # Placement
//!
//! `requires` prepends; `ensures` has to observe the returned value, and a
//! function can produce that value at more than one point:
//!
//! - the **tail expression** — wrapped whole, since its value *is* the
//!   return value regardless of its shape (`if`, `match`, a block, a
//!   diverging call);
//! - every explicit **`return e`** — rewritten to assert before returning.
//!
//! Both are needed. ADR-0006 §5 condition 1 is "every return path
//! instrumented, not just the tail", and this is where the port is better
//! than upstream, whose Rust backend instruments only the implicit tail —
//! verified there: a function with `ensures result > 100` and an explicit
//! `return x` returned `7` with no diagnostic and no abort.
//!
//! The set of return points instrumented here is deliberately the same set
//! `rust_refine::checks` builds return-site obligations for, so the checker
//! and the enforcer agree by construction rather than by coincidence.

use mvl_rust_core::attrs::Predicate;
use proc_macro2::TokenStream;
use quote::quote;
use syn::visit_mut::{self, VisitMut};
use syn::{Block, Expr};

/// Lowers `predicate` to an expression that evaluates to `bool` at runtime.
///
/// A bounded quantifier becomes a loop. This is possible only because both
/// range endpoints are **literal integers** in the grammar (see
/// [`Predicate`]'s module docs) — there is nothing to evaluate, so the
/// bounds can be emitted directly.
///
/// That matters for ADR-0006 §5 condition 3 (the predicate must be
/// runtime-evaluable). Upstream's `is_runtime_checkable` returns false for
/// quantifiers and excludes them; #53 anticipated having to do the same and
/// then also exclude them from Γ, since an unenforceable predicate must not
/// be assumed. Lowering avoids that coupling altogether: no predicate in the
/// grammar is unenforceable, so nothing has to be excluded from Γ.
///
/// **The quantified variable's type is left to inference, deliberately not
/// pinned to `i64`.** `syn` carries no type information, so nothing here
/// knows what type the body actually needs. The compliant demo's
/// `require_dense_fleet` caught this concretely: its body is
/// `section_occupied(i)`, and `section_occupied` takes `i32` — pinning `i`
/// to `i64` made the real example crate fail to compile with a type
/// mismatch, a bug the hand-written fixtures never exercised because none of
/// them called a typed function from inside a quantifier body.
///
/// Getting this right takes two changes, not one — the closure parameter
/// being unannotated (below) is necessary but was not sufficient on its own,
/// caught by re-running the same real example after the first fix and
/// seeing the identical error. [`Predicate::Forall`]/[`Exists`] store `lo`
/// and `hi` as `i64` to accommodate the grammar's full literal range, and
/// `quote!(#lo)` on an `i64` value emits a **suffixed** literal (`1i64`),
/// confirmed empirically — so the range's type, and hence the closure
/// parameter's, was still being forced to `i64` by the bound literals
/// regardless of the parameter's own annotation. [`unsuffixed`] constructs
/// the literal without that suffix, so nothing pins the type and inference
/// is free to pick the constrained type where the body imposes one, falling
/// back to `i32` — its own default integer type — where nothing does (a body
/// with no function call at all, `i > 0`). A range literal too large for the
/// inferred type is then a compile error naming the literal, which is the
/// correct failure mode: fail loud rather than silently truncate. Not a new
/// limitation this injection introduces, either — L3's own bounded-quantifier
/// expansion (ADR-0006 §3) enumerates every value in the range, so a
/// multi-billion-entry range was already impractical to discharge statically
/// before any of this.
fn predicate_to_bool(predicate: &Predicate) -> TokenStream {
    match predicate {
        Predicate::Expr(expr) => quote!(#expr),
        // `all`/`any` over an inclusive range: the bounds are inclusive in
        // the source grammar (`[lo..hi]` means lo through hi), so `..=`.
        Predicate::Forall { var, lo, hi, body } => {
            let (lo, hi) = (unsuffixed(*lo), unsuffixed(*hi));
            let inner = predicate_to_bool(body);
            quote!((#lo..=#hi).all(|#var| #inner))
        }
        Predicate::Exists { var, lo, hi, body } => {
            let (lo, hi) = (unsuffixed(*lo), unsuffixed(*hi));
            let inner = predicate_to_bool(body);
            quote!((#lo..=#hi).any(|#var| #inner))
        }
    }
}

/// An integer literal with no type suffix, so it does not itself pin the
/// quantified variable's type — see [`predicate_to_bool`]'s doc comment.
/// Handles negative values directly; verified `i64_unsuffixed` renders one as
/// `- N` (unary negation of the unsuffixed magnitude) rather than rejecting
/// the sign, so the "nothing pins the type" property holds either way.
fn unsuffixed(value: i64) -> proc_macro2::Literal {
    proc_macro2::Literal::i64_unsuffixed(value)
}

/// The assertion for `predicate`, carrying `provenance` in its message so a
/// failure names the contract that was violated rather than just a line.
///
/// Wrapped in a block carrying `#[allow(clippy::all)]`. `quote!(#expr)`
/// re-emits the predicate's tokens with their **original spans** — the same
/// ones the author wrote in the attribute argument — so once the predicate
/// becomes live code, clippy lints it exactly as if the author had written it
/// inline in the body. Caught concretely: `#[mvl::requires(0 <= b && b <=
/// 255)]`, taken verbatim from the compliant demo's `mask_low_nibble`,
/// triggers `clippy::manual_range_contains` suggesting `(0..=255).contains(&b)`
/// — a refactor of the *predicate*, which is a contract specification in a
/// grammar that happens to reuse Rust's expression syntax for parsing
/// convenience, not a stylistic opinion about the author's Rust code. An
/// attribute is not written as an outer attribute on `assert!(...)` itself
/// (verified: rustc reports it unused when placed there, since the attribute
/// attaches to the macro *invocation* rather than anything it can carry
/// through expansion) but on the wrapping block, which the allow governs
/// regardless of where the tokens inside originated.
fn assertion(predicate: &Predicate, provenance: &str) -> TokenStream {
    let condition = predicate_to_bool(predicate);
    let rendered = predicate.render();
    quote! {
        #[allow(clippy::all)]
        {
            assert!(#condition, concat!(#provenance, " violated: ", #rendered));
        }
    }
}

/// Prepends `requires`'s assertion to `block`.
pub fn inject_requires(block: &mut Block, predicate: &Predicate) {
    let assertion = assertion(predicate, "`#[mvl::requires]`");
    let assertion: syn::Stmt = syn::parse2(assertion).expect("assertion is a valid statement");
    block.stmts.insert(0, assertion);
}

/// Instruments every point `block` produces its value with `ensures`'s
/// assertion, binding the produced value to `result`.
pub fn inject_ensures(block: &mut Block, predicate: &Predicate) {
    let assertion = assertion(predicate, "`#[mvl::ensures]`");

    // Explicit `return`s first: rewriting them does not disturb the tail,
    // whereas wrapping the tail first would put the new tail's `return`-free
    // body in scope of the rewriter for no reason.
    ReturnRewriter {
        assertion: &assertion,
    }
    .visit_block_mut(block);

    let tail = block.stmts.pop();
    let wrapped = match tail {
        // A trailing expression *is* the return value, whatever its shape.
        // No need to descend into `if`/`match` arms the way the checker does:
        // the checker splits them to get a precise per-branch obligation,
        // while the assert only needs the value that came out.
        Some(syn::Stmt::Expr(expr, None)) => quote! {
            { let result = #expr; #assertion result }
        },
        // No trailing expression. Either the function returns `()`, or the
        // body diverges — the only two ways a body can lack one. `ensures`
        // over `()` is unusual but legal, and silently skipping it would
        // leave a declared postcondition unenforced.
        other => {
            if let Some(stmt) = other {
                block.stmts.push(stmt);
            }
            quote! { { let result = (); #assertion result } }
        }
    };

    // `#[allow(unreachable_code)]`, scoped to the block this macro generates.
    //
    // Both arms above can land after a diverging body -- `fn f() -> i64 {
    // return x; }` leaves no tail, and `fn f() -> i64 { panic!() }` leaves a
    // diverging one. Rust accepts either (the unreachable tail coerces, so
    // this is a warning rather than a type error, verified) but warns, and
    // this workspace builds with `-D warnings`.
    //
    // The unreachability is an artifact of the injection, not of the author's
    // code: they wrote no unreachable expression. Suppressing it anywhere
    // wider would hide a real lint, so it goes on the generated block only.
    let mut wrapped: syn::ExprBlock =
        syn::parse2(wrapped).expect("wrapped tail is a valid block expression");
    wrapped
        .attrs
        .push(syn::parse_quote!(#[allow(unreachable_code)]));
    block
        .stmts
        .push(syn::Stmt::Expr(Expr::Block(wrapped), None));
}

/// Rewrites `return e` to assert before handing `e` back.
struct ReturnRewriter<'a> {
    assertion: &'a TokenStream,
}

impl VisitMut for ReturnRewriter<'_> {
    fn visit_expr_mut(&mut self, node: &mut Expr) {
        // A closure or `async` block owns its own return target, so a
        // `return` inside one is not a return from *this* function and must
        // be left alone. Same distinction `rust_refine::checks` draws with
        // its `returns_here` flag (#46), for the same reason: instrumenting
        // one would assert the enclosing function's postcondition against a
        // value that is not its result.
        if matches!(node, Expr::Closure(_) | Expr::Async(_)) {
            return;
        }

        if matches!(node, Expr::Return(_)) {
            // Recurse first: a `return` nested inside the returned
            // expression is itself a return point.
            visit_mut::visit_expr_mut(self, node);
            let Expr::Return(ret) = node else {
                unreachable!("node was a return and visiting cannot change that")
            };

            let assertion = self.assertion;
            let value = ret
                .expr
                .take()
                .map(|expr| quote!(#expr))
                .unwrap_or_else(|| quote!(()));
            ret.expr = Some(Box::new(
                syn::parse2(quote! {
                    { let result = #value; #assertion result }
                })
                .expect("wrapped return value is a valid expression"),
            ));
            return;
        }

        visit_mut::visit_expr_mut(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn predicate(src: &str) -> Predicate {
        syn::parse_str(src).expect("test predicate parses")
    }

    fn block(src: &str) -> Block {
        syn::parse_str(src).expect("test block parses")
    }

    fn rendered(block: &Block) -> String {
        quote!(#block).to_string()
    }

    #[test]
    fn requires_uses_assert_not_debug_assert() {
        // ADR-0006 §5 condition 2: a `debug_assert!` compiles out under
        // `--release`, which would make the check present in exactly the
        // build profile that ships least often. Upstream reached the same
        // conclusion (#672). This is the regression guard for that decision.
        let mut b = block("{ x }");
        inject_requires(&mut b, &predicate("x > 0"));
        let text = rendered(&b);
        assert!(text.contains("assert !"), "expected `assert!`, got: {text}");
        assert!(
            !text.contains("debug_assert"),
            "must never emit `debug_assert!`, got: {text}"
        );
    }

    #[test]
    fn ensures_uses_assert_not_debug_assert() {
        let mut b = block("{ x }");
        inject_ensures(&mut b, &predicate("result > 0"));
        let text = rendered(&b);
        assert!(text.contains("assert !"), "expected `assert!`, got: {text}");
        assert!(!text.contains("debug_assert"), "got: {text}");
    }

    #[test]
    fn requires_prepends_the_check_before_the_body() {
        let mut b = block("{ do_the_thing() }");
        inject_requires(&mut b, &predicate("x > 0"));
        // The assertion must run *before* the original body -- a precondition
        // checked after the body already executed doesn't protect the body.
        let text = rendered(&b);
        let assert_pos = text.find("assert !").unwrap();
        let body_pos = text.find("do_the_thing").unwrap();
        assert!(
            assert_pos < body_pos,
            "assert must precede the body: {text}"
        );
    }

    #[test]
    fn forall_lowers_to_all_over_an_inclusive_unsuffixed_range() {
        let mut b = block("{ 1 }");
        inject_ensures(&mut b, &predicate("forall i in [1..50] . i > 0"));
        let text = rendered(&b);
        assert!(text.contains(". all"), "expected `.all(...)`, got: {text}");
        // Inclusive: source `[1..50]` means 1 *through* 50.
        assert!(
            text.contains("1 ..= 50"),
            "expected an inclusive range, got: {text}"
        );
        // Unsuffixed -- no `1i64`/`50i64` pinning the quantified variable's
        // type. See `predicate_to_bool`'s doc comment for why this matters:
        // pinning to `i64` broke the real compliant demo, which quantifies
        // over a variable passed to an `i32`-taking function.
        assert!(
            !text.contains("i64"),
            "range bounds must not carry a type suffix, got: {text}"
        );
    }

    #[test]
    fn exists_lowers_to_any() {
        let mut b = block("{ 1 }");
        inject_ensures(&mut b, &predicate("exists i in [0..3] . i > 0"));
        let text = rendered(&b);
        assert!(text.contains(". any"), "expected `.any(...)`, got: {text}");
    }

    #[test]
    fn negative_bound_does_not_pin_a_type_either() {
        let mut b = block("{ 1 }");
        inject_ensures(&mut b, &predicate("forall i in [-5..5] . i > -10"));
        let text = rendered(&b);
        assert!(!text.contains("i64"), "got: {text}");
    }

    #[test]
    fn explicit_return_is_instrumented() {
        let mut b = block("{ if x > 0 { return x ; } x + 1 }");
        inject_ensures(&mut b, &predicate("result > 0"));
        let text = rendered(&b);
        // Two instrumented return points: the explicit `return x` and the
        // tail `x + 1`. Two occurrences of the assertion is the signal that
        // both were reached, not just the tail -- the exact gap #53 closes
        // relative to upstream's Rust backend, which instruments only the
        // implicit tail.
        assert_eq!(
            text.matches("assert !").count(),
            2,
            "both the explicit return and the tail must be instrumented: {text}"
        );
    }

    #[test]
    fn a_return_with_no_value_binds_unit() {
        let mut b = block("{ if x { return ; } }");
        inject_ensures(&mut b, &predicate("result == 0"));
        let text = rendered(&b);
        // `return;` with no expression must still bind `result` -- to `()`
        // -- rather than being skipped as if it carried no value.
        assert!(text.contains("let result = ()"), "got: {text}");
    }

    #[test]
    fn a_return_nested_in_a_closure_is_not_instrumented() {
        // A `return` inside a closure returns from the closure, not from the
        // enclosing function -- instrumenting it would assert this
        // function's postcondition against a value that is not its result.
        let mut b = block("{ let f = | | { return 1 ; } ; f ( ) }");
        inject_ensures(&mut b, &predicate("result > 0"));
        let text = rendered(&b);
        // Exactly one assertion: the tail `f()`. The closure's `return 1`
        // must be untouched.
        assert_eq!(
            text.matches("assert !").count(),
            1,
            "the closure's return must not be instrumented: {text}"
        );
    }

    #[test]
    fn a_diverging_tail_still_compiles_with_the_unreachable_allow() {
        // No trailing expression after the rewriter runs (the body's only
        // content was an explicit return) -- exercises the "no tail" arm and
        // confirms it carries the `#[allow(unreachable_code)]` needed to
        // build clean under `-D warnings`.
        let mut b = block("{ return x ; }");
        inject_ensures(&mut b, &predicate("result > 0"));
        let text = rendered(&b);
        assert!(text.contains("allow (unreachable_code)"), "got: {text}");
    }
}
