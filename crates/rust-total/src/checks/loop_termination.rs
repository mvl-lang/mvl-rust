//! Requires `mvl::loop_decreases!(measure)` as the first statement of any
//! `while`/`loop` inside a `#[mvl::total]` function body, and proves the
//! measure strictly decreases each iteration (spec 003 Requirement 6,
//! ADR-0010).
//!
//! `termination.rs` only ever looked at direct recursive function calls --
//! confirmed empirically before this module existed, `#[mvl::total] fn f()
//! { loop { n += 1; } }` was accepted with zero diagnostics. An unconditional
//! `loop`/`while` is at least as common a way real Rust diverges as direct
//! self-recursion, so "terminates" was a bigger claim than what was checked.
//!
//! MVL's own `decreases` attaches directly to the loop (`while cond
//! decreases measure { ... }`). `#[mvl::decreases(measure)]` can't do the
//! same here -- confirmed by actually compiling it: a real
//! `#[proc_macro_attribute]` cannot legally attach to a `while`/`loop`
//! expression in statement position on stable Rust (rust-lang/rust#54727;
//! needs the unstable `stmt_expr_attributes` feature). `loop_decreases!` is
//! instead a **function-like** macro, which has no such restriction, placed
//! as the loop body's first statement.
//!
//! Once the measure is named, the shape of the check mirrors
//! `termination.rs` closely: find the (at most one) top-level mutation of
//! `measure` in the loop body, build it as an entailment goal
//! `<value after mutation> < <measure>`, and discharge it through the same
//! native solver `termination.rs` and `rust-refine` use, with the
//! function's own `#[mvl::requires(...)]` clauses as hypotheses. No
//! operator is special-cased or excluded up front -- `n -= 1`, `n += 1`,
//! `n *= 2`, `n &= mask`, and `n = <anything>` are all handed to the solver
//! identically, and it accepts only what it can actually prove decreases
//! (subtraction of a provably-positive amount; nothing else in this
//! solver's linear-arithmetic fragment, same as `termination.rs`).
//!
//! # Why exactly one, unconditional, top-level mutation
//!
//! A mutation buried inside an `if`/`match`/nested loop only sometimes
//! runs, so it isn't a sound per-iteration decrease even when the shape
//! itself is fine. A second mutation anywhere in the body composes with the
//! first in a way this checker has no way to reason about. Both cases are
//! rejected by the same mechanism: count every assignment to `measure`
//! anywhere in the body (any depth, any nesting -- a plain recursive walk
//! catches all of it for free) and require the total to be exactly one,
//! *and* for that one to also show up in a flat, top-level scan of the
//! body's own statement list. If both scans agree on exactly one, it's
//! unconditional; if the recursive count is 1 but the top-level scan finds
//! none, the one mutation that exists is conditional -- rejected either
//! way, per ADR-0001 §5 (false rejection over false acceptance).
//!
//! `measure` MUST also not be rebound anywhere in the loop body, for the
//! identical reason `termination.rs` rejects a shadowed recursive measure
//! (see [`super::termination::measure_is_shadowed`], shared with this
//! module).
//!
//! Nested loops are each checked independently and need their own
//! `loop_decreases!`. Loops inside `impl` methods are out of scope, same as
//! every other check in this tool (ADR-0001: only `ItemFn` is visited).

use mvl_rust_core::attrs::Predicate;
use mvl_rust_core::diagnostics::{Diagnostic, Level};
use mvl_rust_core::solver::native::discharge_entailment;
use mvl_rust_core::solver::DischargeResult;
use proc_macro2::Span;
use quote::quote;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{BinOp, Block, Expr, ExprLoop, ExprWhile, Ident, ItemFn, Stmt};

use super::termination::measure_is_shadowed;

struct LoopSite<'ast> {
    body: &'ast Block,
    span: Span,
}

struct LoopCollector<'ast> {
    sites: Vec<LoopSite<'ast>>,
}

impl<'ast> Visit<'ast> for LoopCollector<'ast> {
    fn visit_expr_while(&mut self, node: &'ast ExprWhile) {
        self.sites.push(LoopSite {
            body: &node.body,
            span: node.while_token.span(),
        });
        visit::visit_expr_while(self, node);
    }

    fn visit_expr_loop(&mut self, node: &'ast ExprLoop) {
        self.sites.push(LoopSite {
            body: &node.body,
            span: node.loop_token.span(),
        });
        visit::visit_expr_loop(self, node);
    }
}

fn find_loop_sites(item_fn: &ItemFn) -> Vec<LoopSite<'_>> {
    let mut collector = LoopCollector { sites: Vec::new() };
    collector.visit_block(&item_fn.block);
    collector.sites
}

fn expr_is_ident(expr: &Expr, ident: &Ident) -> bool {
    matches!(expr, Expr::Path(p) if p.path.get_ident().is_some_and(|i| i == ident))
}

fn is_compound_assign_op(op: &BinOp) -> bool {
    matches!(
        op,
        BinOp::AddAssign(_)
            | BinOp::SubAssign(_)
            | BinOp::MulAssign(_)
            | BinOp::DivAssign(_)
            | BinOp::RemAssign(_)
            | BinOp::BitXorAssign(_)
            | BinOp::BitAndAssign(_)
            | BinOp::BitOrAssign(_)
            | BinOp::ShlAssign(_)
            | BinOp::ShrAssign(_)
    )
}

/// Is `expr` an assignment (compound or plain) to `measure`? Deliberately
/// uniform across every operator -- see the module doc for why nothing is
/// filtered out here.
fn is_measure_assignment(expr: &Expr, measure: &Ident) -> bool {
    match expr {
        Expr::Assign(a) => expr_is_ident(&a.left, measure),
        Expr::Binary(b) => is_compound_assign_op(&b.op) && expr_is_ident(&b.left, measure),
        _ => false,
    }
}

struct MutationCounter<'a> {
    measure: &'a Ident,
    count: usize,
}

impl<'ast> Visit<'ast> for MutationCounter<'_> {
    fn visit_expr(&mut self, node: &'ast Expr) {
        if is_measure_assignment(node, self.measure) {
            self.count += 1;
        }
        visit::visit_expr(self, node);
    }
}

/// Every assignment to `measure` anywhere in `body`, at any nesting depth.
/// The recursive default walk means an `if`/`match`/nested-loop-buried
/// mutation is caught for free, with no per-construct handling needed.
fn count_mutations_anywhere(body: &Block, measure: &Ident) -> usize {
    let mut counter = MutationCounter { measure, count: 0 };
    counter.visit_block(body);
    counter.count
}

/// The assignment to `measure` that is a direct, top-level statement of
/// `body` (not nested in any conditional or inner block) -- `None` if
/// there isn't exactly one.
fn top_level_mutation<'ast>(body: &'ast Block, measure: &Ident) -> Option<&'ast Expr> {
    let mut found = None;
    for stmt in &body.stmts {
        if let Stmt::Expr(expr, _) = stmt {
            if is_measure_assignment(expr, measure) {
                if found.is_some() {
                    return None; // more than one at the top level
                }
                found = Some(expr);
            }
        }
    }
    found
}

/// The expression for `measure`'s value after `mutation` executes --
/// `a - b` for `a -= b`, the assignment's right-hand side as-is for
/// `a = <expr>`. Handed to the solver as-is; see the module doc for why no
/// operator is special-cased here.
fn value_after(mutation: &Expr) -> Expr {
    match mutation {
        Expr::Assign(a) => (*a.right).clone(),
        Expr::Binary(b) => {
            let left = &b.left;
            let right = &b.right;
            let tokens = match &b.op {
                BinOp::AddAssign(_) => quote! { (#left) + (#right) },
                BinOp::SubAssign(_) => quote! { (#left) - (#right) },
                BinOp::MulAssign(_) => quote! { (#left) * (#right) },
                BinOp::DivAssign(_) => quote! { (#left) / (#right) },
                BinOp::RemAssign(_) => quote! { (#left) % (#right) },
                BinOp::BitXorAssign(_) => quote! { (#left) ^ (#right) },
                BinOp::BitAndAssign(_) => quote! { (#left) & (#right) },
                BinOp::BitOrAssign(_) => quote! { (#left) | (#right) },
                BinOp::ShlAssign(_) => quote! { (#left) << (#right) },
                BinOp::ShrAssign(_) => quote! { (#left) >> (#right) },
                _ => unreachable!("guarded by is_compound_assign_op"),
            };
            syn::parse2(tokens).expect("a compound assignment's operands form a valid expression")
        }
        _ => unreachable!("guarded by is_measure_assignment"),
    }
}

fn find_marker(body: &Block) -> Option<&syn::Macro> {
    let Stmt::Macro(stmt_macro) = body.stmts.first()? else {
        return None;
    };
    stmt_macro
        .mac
        .path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "loop_decreases")
        .then_some(&stmt_macro.mac)
}

pub fn check(item_fn: &ItemFn, hypotheses: &[Expr], diagnostics: &mut Vec<Diagnostic>) {
    for site in find_loop_sites(item_fn) {
        check_site(&site, hypotheses, diagnostics);
    }
}

fn check_site(site: &LoopSite<'_>, hypotheses: &[Expr], diagnostics: &mut Vec<Diagnostic>) {
    let Some(marker) = find_marker(site.body) else {
        diagnostics.push(missing_marker_diagnostic(site.span));
        return;
    };

    let measure_expr: Expr = match syn::parse2(marker.tokens.clone()) {
        Ok(expr) => expr,
        Err(_) => {
            diagnostics.push(non_parameter_measure_diagnostic(marker.span()));
            return;
        }
    };
    let Expr::Path(measure_path) = &measure_expr else {
        diagnostics.push(non_parameter_measure_diagnostic(measure_expr.span()));
        return;
    };
    let Some(measure_ident) = measure_path.path.get_ident() else {
        diagnostics.push(non_parameter_measure_diagnostic(measure_expr.span()));
        return;
    };

    if measure_is_shadowed(site.body, measure_ident) {
        diagnostics.push(shadowed_measure_diagnostic(site.span, measure_ident));
        return;
    }

    let total = count_mutations_anywhere(site.body, measure_ident);
    if total == 0 {
        diagnostics.push(no_mutation_diagnostic(site.span, measure_ident));
        return;
    }
    if total > 1 {
        diagnostics.push(multiple_mutations_diagnostic(site.span, measure_ident));
        return;
    }
    let Some(mutation) = top_level_mutation(site.body, measure_ident) else {
        diagnostics.push(conditional_mutation_diagnostic(site.span, measure_ident));
        return;
    };

    let new_value = value_after(mutation);
    let goal_expr: Expr = syn::parse2(quote! { (#new_value) < (#measure_ident) })
        .expect("a loop mutation's new value and the measure form a valid comparison");

    match discharge_entailment(hypotheses, &Predicate::Expr(goal_expr)) {
        DischargeResult::Proven { .. } => {}
        DischargeResult::Violated { counterexample } => {
            diagnostics.push(unproven_diagnostic(
                site.span,
                measure_ident,
                Some(counterexample),
            ));
        }
        DischargeResult::Runtime => {
            diagnostics.push(unproven_diagnostic(site.span, measure_ident, None));
        }
    }
}

fn missing_marker_diagnostic(span: Span) -> Diagnostic {
    Diagnostic::new(
        Level::Error,
        "`while`/`loop` in a `#[mvl::total]` function requires `mvl::loop_decreases!(measure)` as its first statement".to_string(),
        span,
    )
    .with_label("loop found, no termination measure")
    .with_suggestion(
        "add `mvl::loop_decreases!(measure);` as the loop body's first statement, naming a variable that strictly decreases each iteration",
    )
}

fn non_parameter_measure_diagnostic(span: Span) -> Diagnostic {
    Diagnostic::new(
        Level::Error,
        "`mvl::loop_decreases!(...)` must name a bare local variable; computed measures aren't analyzable yet".to_string(),
        span,
    )
    .with_label("not a bare identifier")
    .with_suggestion("use a single variable name as the measure, e.g. mvl::loop_decreases!(n)")
}

fn shadowed_measure_diagnostic(span: Span, measure: &Ident) -> Diagnostic {
    Diagnostic::new(
        Level::Error,
        format!("`mvl::loop_decreases!({measure})` cannot be verified: `{measure}` is rebound somewhere in the loop body"),
        span,
    )
    .with_label("measure identifier shadowed in the loop body")
    .with_suggestion(format!(
        "rename the shadowing binding so `{measure}` unambiguously refers to the same variable throughout the loop"
    ))
}

fn no_mutation_diagnostic(span: Span, measure: &Ident) -> Diagnostic {
    Diagnostic::new(
        Level::Error,
        format!("`mvl::loop_decreases!({measure})`: `{measure}` is never assigned in the loop body, so it cannot decrease"),
        span,
    )
    .with_label("no assignment to the measure found")
    .with_suggestion(format!("assign `{measure}` a provably smaller value once per iteration, e.g. `{measure} -= 1;`"))
}

fn multiple_mutations_diagnostic(span: Span, measure: &Ident) -> Diagnostic {
    Diagnostic::new(
        Level::Error,
        format!("`mvl::loop_decreases!({measure})`: `{measure}` is assigned more than once in the loop body"),
        span,
    )
    .with_label("more than one assignment to the measure")
    .with_suggestion(format!("keep exactly one, unconditional assignment to `{measure}` per iteration"))
}

fn conditional_mutation_diagnostic(span: Span, measure: &Ident) -> Diagnostic {
    Diagnostic::new(
        Level::Error,
        format!("`mvl::loop_decreases!({measure})`: the only assignment to `{measure}` is conditional (nested in an `if`/`match`/inner loop)"),
        span,
    )
    .with_label("assignment to the measure is not unconditional")
    .with_suggestion(format!("assign `{measure}` unconditionally, as a direct top-level statement of the loop body"))
}

fn unproven_diagnostic(span: Span, measure: &Ident, counterexample: Option<String>) -> Diagnostic {
    let diagnostic = Diagnostic::new(
        Level::Error,
        format!("loop does not provably decrease `mvl::loop_decreases!({measure})`"),
        span,
    );
    let diagnostic = match counterexample {
        Some(counterexample) => diagnostic.with_label(counterexample),
        None => diagnostic.with_label(
            "the solver could not show the measure's new value is strictly less than its old value",
        ),
    };
    diagnostic.with_suggestion(format!(
        "assign `{measure}` a value provably less than `{measure}` (e.g. `{measure} -= 1`), or add a `#[mvl::requires(...)]` bound the solver can use as a hypothesis"
    ))
}
