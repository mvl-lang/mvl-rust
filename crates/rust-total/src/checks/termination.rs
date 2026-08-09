//! Requires `#[mvl::decreases(measure)]` on any `#[mvl::total]` function
//! that directly calls itself, and checks that the measure provably
//! decreases (ADR-0009, superseding spec 003 Requirement 3's original
//! presence-only wording).
//!
//! `measure` MUST be a bare identifier naming one of the function's own
//! parameters. At every direct recursive call, the argument passed in that
//! parameter's position MUST match one of a small recognized
//! strictly-decreasing shape set: `param - <positive integer literal>` or
//! `param / <integer literal >= 2>`. Anything else — a computed measure, an
//! unrelated argument, the same value passed unchanged — is rejected, not
//! silently accepted. Only direct self-recursion is detected; mutual
//! recursion between two functions is out of scope for v1.

use mvl_rust_core::diagnostics::{Diagnostic, Level};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{BinOp, Expr, ExprCall, FnArg, Ident, ItemFn, Lit, Pat};

struct RecursiveCallCollector<'a> {
    fn_name: &'a Ident,
    calls: Vec<&'a ExprCall>,
}

impl<'ast> Visit<'ast> for RecursiveCallCollector<'ast> {
    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Expr::Path(path_expr) = &*node.func {
            if path_expr
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == *self.fn_name)
            {
                self.calls.push(node);
            }
        }
        visit::visit_expr_call(self, node);
    }
}

fn find_recursive_calls<'ast>(item_fn: &'ast ItemFn, fn_name: &'ast Ident) -> Vec<&'ast ExprCall> {
    let mut collector = RecursiveCallCollector {
        fn_name,
        calls: Vec::new(),
    };
    collector.visit_block(&item_fn.block);
    collector.calls
}

/// Parameter names in declaration order. `None` in place of any parameter
/// this v1 doesn't model (`self`, or a pattern more complex than a bare
/// binding) -- such a parameter simply can't be a measure, not a reason to
/// error here.
fn param_names(item_fn: &ItemFn) -> Vec<Option<&Ident>> {
    item_fn
        .sig
        .inputs
        .iter()
        .map(|arg| match arg {
            FnArg::Typed(pat_type) => match &*pat_type.pat {
                Pat::Ident(pat_ident) => Some(&pat_ident.ident),
                _ => None,
            },
            FnArg::Receiver(_) => None,
        })
        .collect()
}

fn expr_is_ident(expr: &Expr, ident: &Ident) -> bool {
    matches!(expr, Expr::Path(p) if p.path.get_ident().is_some_and(|i| i == ident))
}

fn int_literal_value(expr: &Expr) -> Option<i128> {
    match expr {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Int(n) => n.base10_parse::<i128>().ok(),
            _ => None,
        },
        _ => None,
    }
}

/// Does `arg` provably decrease `measure` (a bare parameter identifier)?
/// Recognizes exactly two shapes (ADR-0009 §2): `measure - <positive
/// literal>` and `measure / <literal >= 2>`. Anything else -- including the
/// measure passed unchanged, a computed expression, or an unrelated
/// argument -- is not recognized.
fn is_recognized_decrease(arg: &Expr, measure: &Ident) -> bool {
    let Expr::Binary(bin) = arg else {
        return false;
    };
    if !expr_is_ident(&bin.left, measure) {
        return false;
    }
    let Some(n) = int_literal_value(&bin.right) else {
        return false;
    };
    match bin.op {
        BinOp::Sub(_) => n > 0,
        BinOp::Div(_) => n >= 2,
        _ => false,
    }
}

pub fn check(item_fn: &ItemFn, decreases: Option<&Expr>, diagnostics: &mut Vec<Diagnostic>) {
    let fn_name = &item_fn.sig.ident;
    let recursive_calls = find_recursive_calls(item_fn, fn_name);
    if recursive_calls.is_empty() {
        return;
    }

    let Some(measure) = decreases else {
        diagnostics.push(
            Diagnostic::new(
                Level::Error,
                format!(
                    "recursive `#[mvl::total]` function `{fn_name}` requires `#[mvl::decreases(measure)]`"
                ),
                fn_name.span(),
            )
            .with_label("recursive call found, no termination measure")
            .with_suggestion(
                "add #[mvl::decreases(measure)] with a measure that strictly decreases on each recursive call",
            ),
        );
        return;
    };

    let Expr::Path(measure_path) = measure else {
        diagnostics.push(non_parameter_measure_diagnostic(fn_name, measure));
        return;
    };
    let Some(measure_ident) = measure_path.path.get_ident() else {
        diagnostics.push(non_parameter_measure_diagnostic(fn_name, measure));
        return;
    };

    let names = param_names(item_fn);
    let Some(param_index) = names
        .iter()
        .position(|name| name.is_some_and(|n| n == measure_ident))
    else {
        diagnostics.push(non_parameter_measure_diagnostic(fn_name, measure));
        return;
    };

    for call in recursive_calls {
        let arg = call.args.iter().nth(param_index);
        let recognized = arg.is_some_and(|arg| is_recognized_decrease(arg, measure_ident));
        if !recognized {
            diagnostics.push(
                Diagnostic::new(
                    Level::Error,
                    format!(
                        "recursive call to `{fn_name}` does not provably decrease `#[mvl::decreases({measure_ident})]`"
                    ),
                    call.span(),
                )
                .with_label("measure not shown to strictly decrease here")
                .with_suggestion(format!(
                    "pass `{measure_ident} - <positive literal>` or `{measure_ident} / <literal >= 2>` at this call site"
                )),
            );
        }
    }
}

fn non_parameter_measure_diagnostic(fn_name: &Ident, measure: &Expr) -> Diagnostic {
    Diagnostic::new(
        Level::Error,
        format!(
            "`#[mvl::decreases(...)]` on `{fn_name}` must be a bare parameter identifier; computed measures aren't analyzable yet"
        ),
        measure.span(),
    )
    .with_label("not a bare parameter identifier")
    .with_suggestion("use a single parameter name as the measure, e.g. #[mvl::decreases(n)]")
}
