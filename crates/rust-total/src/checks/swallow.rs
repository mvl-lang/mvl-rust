//! Flags silent discarding of a fallible expression's result inside
//! `#[mvl::total]` functions: `let _ = <call>;`, `drop(<call>)` /
//! `mem::drop(<call>)`, and `.map(|_| ())` used to throw away a value.
//!
//! **Syntactic-only, same known limitation as [`super::panic_freedom`]**:
//! without type information, this cannot tell a `Result`/`Option`-returning
//! call from one that returns `()` or a plain value. It flags any call
//! discarded through these three shapes, which is deliberately
//! over-inclusive (a `let _ = log(...)` where `log` returns `()` is not a
//! real totality violation) rather than under-inclusive (missing a real
//! swallowed `Err`). `#[mvl::unchecked]` remains the escape hatch for a
//! function that trips a false positive here.
//!
//! `let _ = <ident>;` (a bare variable moved/dropped into `_`, not a call)
//! is **not** flagged — that's a no-op discard of an already-bound value,
//! not a hidden exit path; the totality risk is specifically in swallowing
//! a call's return value before anyone inspects it.

use mvl_rust_core::diagnostics::{Diagnostic, Level};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Expr, ExprCall, ExprClosure, ExprMethodCall, Local, Pat};

struct SwallowVisitor<'d> {
    diagnostics: &'d mut Vec<Diagnostic>,
}

fn is_call(expr: &Expr) -> bool {
    matches!(
        strip_parens(expr),
        Expr::Call(_) | Expr::MethodCall(_) | Expr::Try(_) | Expr::Await(_)
    )
}

fn strip_parens(mut expr: &Expr) -> &Expr {
    while let Expr::Paren(inner) = expr {
        expr = &inner.expr;
    }
    expr
}

/// `drop(<call>)` or `mem::drop(<call>)` / `std::mem::drop(<call>)`.
fn is_drop_call(call: &ExprCall) -> bool {
    let Expr::Path(path) = &*call.func else {
        return false;
    };
    path.path
        .segments
        .last()
        .map(|seg| seg.ident == "drop")
        .unwrap_or(false)
}

/// `|_| ()` -- a closure whose sole parameter is `_` and whose body is the
/// unit literal, the shape `.map(|_| ())` uses to discard an `Ok`/`Some`
/// payload while keeping the `Result`/`Option` wrapper.
fn is_discarding_closure(closure: &ExprClosure) -> bool {
    let all_wildcards = closure
        .inputs
        .iter()
        .all(|input| matches!(input, Pat::Wild(_)));
    let body_is_unit = matches!(&*closure.body, Expr::Tuple(t) if t.elems.is_empty());
    !closure.inputs.is_empty() && all_wildcards && body_is_unit
}

impl<'ast> Visit<'ast> for SwallowVisitor<'_> {
    fn visit_local(&mut self, node: &'ast Local) {
        if matches!(node.pat, Pat::Wild(_)) {
            if let Some(init) = &node.init {
                if is_call(&init.expr) {
                    self.diagnostics.push(
                        Diagnostic::new(
                            Level::Error,
                            "`let _ = <call>` silently discards this value, which is outside `#[mvl::total]`",
                            init.expr.span(),
                        )
                        .with_label("result silently discarded")
                        .with_suggestion(
                            "bind and handle the result explicitly (e.g. match, `?`, or `.expect_ok(..)`-style logging)",
                        ),
                    );
                }
            }
        }
        visit::visit_local(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if is_drop_call(node) && node.args.len() == 1 {
            self.diagnostics.push(
                Diagnostic::new(
                    Level::Error,
                    "`drop(..)` on a call result silently discards it, which is outside `#[mvl::total]`",
                    node.span(),
                )
                .with_label("result silently discarded"),
            );
        }
        visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if node.method == "map" && node.args.len() == 1 {
            if let Expr::Closure(closure) = &node.args[0] {
                if is_discarding_closure(closure) {
                    self.diagnostics.push(
                        Diagnostic::new(
                            Level::Error,
                            "`.map(|_| ())` discards the wrapped value, which is outside `#[mvl::total]`",
                            node.span(),
                        )
                        .with_label("value silently discarded"),
                    );
                }
            }
        }
        visit::visit_expr_method_call(self, node);
    }
}

pub fn check(item_fn: &syn::ItemFn, diagnostics: &mut Vec<Diagnostic>) {
    let mut visitor = SwallowVisitor { diagnostics };
    visitor.visit_block(&item_fn.block);
}
