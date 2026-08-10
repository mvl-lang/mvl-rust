//! Requires `#[mvl::decreases(measure)]` on any `#[mvl::total]` function
//! that directly calls itself, and proves the measure strictly decreases
//! at every recursive call (ADR-0009, superseding spec 003 Requirement 3's
//! original presence-only wording).
//!
//! `measure` MUST be a bare identifier naming one of the function's own
//! parameters. At every direct recursive call, this builds the obligation
//! `<argument> < <measure>` and discharges it through
//! `mvl_rust_core::solver::native` — the same native linear-arithmetic
//! backend `rust-refine` uses for `requires`/`ensures` (already a
//! dependency of this crate; ADR-0001 §3 names the solver as deliberately
//! *shared* infrastructure — "no shared analysis state" is about tools not
//! exchanging per-run results, not about each tool needing its own
//! reasoning engine). The function's own `#[mvl::requires(...)]` clauses
//! (if any) are supplied as hypotheses, so a symbolic decrement can be
//! proved given a positivity precondition — e.g. `#[mvl::decreases(fuel)]`
//! with a recursive call passing `fuel - k`, given
//! `#[mvl::requires(k > 0)]` — not just a literal constant.
//!
//! This reaches exactly as far as the native solver's linear-arithmetic
//! fragment does, no further. Subtraction by a positive amount is provable
//! (a literal proves unconditionally; a variable amount needs a
//! `requires`-supplied lower bound). Division/modulo is outside the
//! solver's linear system entirely and is **never** provable this way —
//! `discharge_entailment` returns `Runtime` regardless of what hypotheses
//! are supplied, confirmed empirically: `(n / 2) < n` is `Runtime` both
//! with no hypotheses and with `n > 0` supplied. Since `#[mvl::decreases]`
//! has no runtime-enforcement fallback (unlike `requires`/`ensures`,
//! ADR-0006), an unproven call is rejected rather than silently trusted.
//! Only direct self-recursion is detected; mutual recursion between two
//! functions is out of scope for v1.
//!
//! `measure` MUST also not be rebound anywhere in the function body (a
//! `let`, a closure parameter, a match arm, ...). With no name resolution,
//! the check cannot tell a load-bearing shadow from a harmless one --
//! `fn f(n: u64) { let n = n + 100; if n == 0 {0} else {f(n - 1)} }` builds
//! the same goal `(n - 1) < n` this check would prove for the honest case,
//! but the `n` in the recursive call means the *shadowed local*, and the
//! function actually never terminates. Confirmed empirically before adding
//! this guard: it was accepted with zero diagnostics. Rejecting any
//! shadow of `measure` is conservative (a harmless, unrelated reuse of the
//! same name is rejected too) but sound, per ADR-0001 §5.

use mvl_rust_core::attrs::Predicate;
use mvl_rust_core::diagnostics::{Diagnostic, Level};
use mvl_rust_core::solver::native::discharge_entailment;
use mvl_rust_core::solver::DischargeResult;
use quote::quote;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Block, Expr, ExprCall, FnArg, Ident, ItemFn, Pat, PatIdent};

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

struct ShadowDetector<'a> {
    measure: &'a Ident,
    shadowed: bool,
}

impl<'ast> Visit<'ast> for ShadowDetector<'_> {
    fn visit_pat_ident(&mut self, node: &'ast PatIdent) {
        if node.ident == *self.measure {
            self.shadowed = true;
        }
        visit::visit_pat_ident(self, node);
    }
}

/// Whether `measure` is rebound anywhere in `block` -- a `let`, a closure
/// parameter, a match arm, a for-loop pattern, anything that introduces a
/// new binding with the same name. `rust-total` has no name resolution
/// (ADR-0001), so it cannot tell a load-bearing shadow (a reference to
/// `measure` downstream now means the *shadowed* local, not the binding an
/// entailment goal is supposed to be about) from a harmless, unrelated
/// reuse of the same identifier. Flagging both is the "false rejection is
/// the safe direction for a gate" call (ADR-0001 §5): the alternative is a
/// real false *acceptance* -- confirmed empirically for the recursive case,
/// `fn f(n: u64) { let n = n + 100; if n == 0 {0} else {f(n - 1)} }` never
/// terminates (each call's `n` is strictly larger than the last) and was
/// accepted with zero diagnostics before this check existed. Shared with
/// `loop_termination.rs`, which has the identical risk for a loop's
/// measure.
pub(super) fn measure_is_shadowed(block: &Block, measure: &Ident) -> bool {
    let mut detector = ShadowDetector {
        measure,
        shadowed: false,
    };
    detector.visit_block(block);
    detector.shadowed
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

pub fn check(
    item_fn: &ItemFn,
    decreases: Option<&Expr>,
    hypotheses: &[Expr],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let fn_name = &item_fn.sig.ident;
    let recursive_calls = find_recursive_calls(item_fn, fn_name);
    if recursive_calls.is_empty() {
        return;
    }

    let Some(measure) = decreases else {
        diagnostics.push(missing_decreases_diagnostic(fn_name));
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

    if measure_is_shadowed(&item_fn.block, measure_ident) {
        diagnostics.push(shadowed_measure_diagnostic(fn_name, measure_ident));
        return;
    }

    for call in recursive_calls {
        let Some(arg) = call.args.iter().nth(param_index) else {
            diagnostics.push(unproven_diagnostic(fn_name, measure_ident, call, None));
            continue;
        };

        // `arg` and `measure_ident` are both already-parsed syn nodes from
        // the same source file, so re-quoting them into `(#arg) < (#measure)`
        // and reparsing always yields a valid comparison expression.
        let goal_expr: Expr = syn::parse2(quote! { (#arg) < (#measure_ident) })
            .expect("a call argument and a parameter identifier form a valid comparison");

        match discharge_entailment(hypotheses, &Predicate::Expr(goal_expr)) {
            DischargeResult::Proven { .. } => {}
            DischargeResult::Violated { counterexample } => {
                diagnostics.push(unproven_diagnostic(
                    fn_name,
                    measure_ident,
                    call,
                    Some(counterexample),
                ));
            }
            DischargeResult::Runtime => {
                diagnostics.push(unproven_diagnostic(fn_name, measure_ident, call, None));
            }
        }
    }
}

fn missing_decreases_diagnostic(fn_name: &Ident) -> Diagnostic {
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
    )
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

fn shadowed_measure_diagnostic(fn_name: &Ident, measure_ident: &Ident) -> Diagnostic {
    Diagnostic::new(
        Level::Error,
        format!(
            "`#[mvl::decreases({measure_ident})]` on `{fn_name}` cannot be verified: `{measure_ident}` is rebound somewhere in the function body"
        ),
        fn_name.span(),
    )
    .with_label("measure identifier shadowed in the body")
    .with_suggestion(format!(
        "rename the shadowing binding so `{measure_ident}` unambiguously refers to the parameter at every recursive call"
    ))
}

fn unproven_diagnostic(
    fn_name: &Ident,
    measure_ident: &Ident,
    call: &ExprCall,
    counterexample: Option<String>,
) -> Diagnostic {
    let diagnostic = Diagnostic::new(
        Level::Error,
        format!(
            "recursive call to `{fn_name}` does not provably decrease `#[mvl::decreases({measure_ident})]`"
        ),
        call.span(),
    );
    let diagnostic = match counterexample {
        Some(counterexample) => diagnostic.with_label(counterexample),
        None => diagnostic.with_label(
            "the solver could not show this call's argument is strictly less than the measure",
        ),
    };
    diagnostic.with_suggestion(format!(
        "pass an argument provably less than `{measure_ident}` (e.g. `{measure_ident} - 1`), or add a `#[mvl::requires(...)]` bound the solver can use as a hypothesis"
    ))
}
