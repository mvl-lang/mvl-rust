//! Flags panic-risk constructs inside `#[mvl::total]` functions: `.unwrap()`,
//! `.expect(...)`, `panic!(...)`/`todo!()`/`unimplemented!()`, raw indexing,
//! and division/modulo (divide-by-zero risk).
//!
//! Deliberately does **not** flag general arithmetic (`+`/`-`/`*`) for
//! overflow — without type information, that would flag nearly all numeric
//! code (every `+`/`-`/`*` anywhere), making the tool useless. A known,
//! documented v1 gap, not an oversight. Division/modulo has the same
//! syntactic-only limitation (a float divisor can't overflow/panic, but
//! this can't tell floats from integers) — kept in scope anyway since
//! `/`/`%` are far less common in ordinary code than `+`/`-`/`*`, so the
//! false-positive rate is much lower.
//!
//! # `#[mvl::requires]`/`#[mvl::ensures]` are deliberately not flagged (#53)
//!
//! Since #53 those attributes expand to a real `assert!`, which panics on
//! failure — a `#[mvl::total]` function carrying one is not literally
//! panic-free, and this checker never sees the assert to begin with: it scans
//! the author's *source*, not the macro-expanded body, so there is no
//! `assert!` token here to flag even in principle (the same boundary that
//! makes any macro invocation invisible to this family of tools, ADR-0002 #4).
//!
//! The resolution is **not** to add `assert` to [`PANICKING_MACROS`] were it
//! ever visible — it is to read `#[mvl::total]`'s panic-freedom claim as
//! scoped to *accidental* crash sources (ADR-0003 §2): `.unwrap()`, raw
//! indexing, a bare `panic!`. A contract assert is not one of those. It is an
//! intentional, documented check whose failure reports a broken promise
//! rather than an unhandled case, and `total` was never claiming to guard
//! against that category.
//!
//! That reasoning covers `requires` cleanly — a firing precondition means the
//! *caller* stepped outside the domain the function was told to expect. It is
//! less clean for `ensures`: a firing postcondition means the function's *own
//! body* failed to establish what it declared, which is the function's bug,
//! not its caller's. Exempting it anyway is still correct, because the
//! relevant distinction for `total` was never "whose fault" — it is
//! "accidental crash source" versus "documented contract check". An `ensures`
//! assert is squarely the latter regardless of who is responsible for it
//! firing, so it stays exempt on the same grounds `requires` does.
//!
//! So this checker takes no action on `requires`/`ensures` at all, and that
//! silence is the decision, not an oversight this module failed to implement.
//! `#[mvl::unchecked]` (`mvl-macros`) is the escape hatch for a function whose
//! author wants `total`'s original, unconditional reading instead.

use mvl_rust_core::diagnostics::{Diagnostic, Level};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{BinOp, ExprBinary, ExprIndex, ExprMethodCall, ItemFn, Macro};

const PANICKING_MACROS: &[&str] = &["panic", "todo", "unimplemented"];

struct PanicFreedomVisitor<'d> {
    diagnostics: &'d mut Vec<Diagnostic>,
}

impl<'ast> Visit<'ast> for PanicFreedomVisitor<'_> {
    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        let method = node.method.to_string();
        if method == "unwrap" || method == "expect" {
            self.diagnostics.push(
                Diagnostic::new(
                    Level::Error,
                    format!("`.{method}()` can panic, which is outside `#[mvl::total]`"),
                    node.method.span(),
                )
                .with_label("panics on failure")
                .with_suggestion(
                    "handle the error case explicitly (e.g. match, `?`, or a default)",
                ),
            );
        }
        visit::visit_expr_method_call(self, node);
    }

    // Overriding the shared `Macro` node (rather than `visit_expr_macro`)
    // catches a panicking macro used as a standalone statement too (e.g.
    // `panic!("oops");`), which syn parses as `Stmt::Macro`, not
    // `Expr::Macro` -- same lesson learned building rust-limit's macro
    // check.
    fn visit_macro(&mut self, node: &'ast Macro) {
        if let Some(name) = node.path.segments.last().map(|s| s.ident.to_string()) {
            if PANICKING_MACROS.contains(&name.as_str()) {
                self.diagnostics.push(
                    Diagnostic::new(
                        Level::Error,
                        format!("`{name}!` panics, which is outside `#[mvl::total]`"),
                        node.span(),
                    )
                    .with_label("always panics"),
                );
            }
        }
        visit::visit_macro(self, node);
    }

    fn visit_expr_index(&mut self, node: &'ast ExprIndex) {
        self.diagnostics.push(
            Diagnostic::new(
                Level::Error,
                "indexing can panic on out-of-bounds access, which is outside `#[mvl::total]`",
                node.span(),
            )
            .with_label("may panic")
            .with_suggestion("use `.get(i)` and handle `None` explicitly"),
        );
        visit::visit_expr_index(self, node);
    }

    fn visit_expr_binary(&mut self, node: &'ast ExprBinary) {
        if matches!(node.op, BinOp::Div(_) | BinOp::Rem(_)) {
            self.diagnostics.push(
                Diagnostic::new(
                    Level::Error,
                    "division/modulo can panic on a zero divisor, which is outside `#[mvl::total]`",
                    node.span(),
                )
                .with_label("may panic"),
            );
        }
        visit::visit_expr_binary(self, node);
    }
}

pub fn check(item_fn: &ItemFn, diagnostics: &mut Vec<Diagnostic>) {
    let mut visitor = PanicFreedomVisitor { diagnostics };
    visitor.visit_item_fn(item_fn);
}
