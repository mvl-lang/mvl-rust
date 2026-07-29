//! Finds every refinement obligation in a source file and discharges it
//! through the native backend (`mvl_rust_core::solver::native`, ADR-0005).
//!
//! Obligations arise at three kinds of program point (#38, #42), asking
//! different questions of the solver:
//!
//! - **Declaration sites** — a `#[mvl::requires(p)]`/`#[mvl::ensures(p)]`
//!   on a function. Nothing is known about arguments here, so the question
//!   is whether `p` is internally coherent
//!   ([`discharge_predicate`]).
//! - **Call sites** — a call `g(args)` in `f`'s body, where `g` declares
//!   `#[mvl::requires(p)]`. The question is the one real MVL's own solver
//!   asks: does `f`'s hypothesis context Γ entail `p[params := args]`
//!   ([`discharge_entailment`])?
//! - **Return sites** — each point `f`'s body produces its value, where `f`
//!   declares `#[mvl::ensures(p)]`. Does Γ entail `p[result := e]` for the
//!   returned expression `e`? This is what makes the postcondition
//!   propagation below sound: a fact assumed at a call site has to be
//!   established somewhere, and this is where.
//!
//! Γ accumulates three kinds of fact, mirroring real MVL's own Γ:
//!
//! 1. `f`'s own `requires` clauses — the refinements of its parameters.
//! 2. Branch narrowing — inside `if c { … }` the condition `c` holds, and
//!    in the `else` arm its negation does. Same for a `while` body.
//! 3. Postcondition propagation — after `let y = g(x);`, `g`'s `ensures`
//!    holds with `result` bound to `y`. Assumed rather than re-derived,
//!    as in any modular verifier (and as in real MVL): `g`'s own
//!    obligation to establish it is the return-site obligation above.
//!
//! Scope, deliberately the same boundary `rust-effect` (#9) draws for the
//! same reason — `syn`-based scanning has no type information and no
//! cross-file resolution:
//!
//! - Call resolution is **same-file, free functions only**. A call to
//!   anything else is silently unresolvable and produces no obligation.
//! - `match`-arm patterns don't narrow Γ (an `if let`/`match` binding
//!   carries no refinement fact yet); only `if`/`else`/`while` conditions do.
//! - Calls inside a macro invocation (`println!("{}", g(x))`) are invisible:
//!   `syn` keeps a macro's body as an opaque token stream, so there is no
//!   call expression to find. Nothing is reported about them either way.
//! - A quantified `requires` (`forall i in [lo..hi]. …`) is a fine *goal*
//!   but isn't added to Γ as a hypothesis — Γ clauses are `&&`-flattened
//!   expressions, and a quantifier has no such form.
//! - **Return points are recognised structurally**, and only where a value
//!   provably flows outwards: a trailing expression, an explicit `return`,
//!   and through `if`/`else`, `match` arms, and plain or `unsafe` blocks in
//!   tail position. A construct not on that list yields no obligation rather
//!   than a guessed one — the pre-#42 silence is the safe direction, since a
//!   false return-site violation is an error that fails the build.
//! - **`?` is not a return point** here. It is an early return of `Err(…)`
//!   whose value isn't `result`-shaped under `syn`'s type-free view, so
//!   nothing is claimed about it either way.
//! - A postcondition over a term containing a **call** (`ensures(result ==
//!   g(a))`) falls to a runtime check: L1 reflexivity is gated to call-free
//!   terms because it is unsound for an impure term (#44). #45 tracks the
//!   purity signal that would lift this.
//!
//! Predicates are plain comparison/boolean expressions, or a bounded
//! quantifier (`forall`/`exists i in [lo..hi]. pred`) — see
//! `mvl_rust_core::attrs::Predicate` (#31) for the grammar.

use std::collections::HashMap;

use mvl_rust_core::attrs::{MvlAttr, Predicate};
use mvl_rust_core::diagnostics::{Diagnostic, Level};
use mvl_rust_core::solver::native::{discharge_entailment, discharge_predicate, substitute_exprs};
use mvl_rust_core::solver::{DischargeResult, Layer};
use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Block, Expr, ExprCall, ExprIf, ExprWhile, FnArg, Item, ItemFn, Local, Pat};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CheckError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse source as Rust: {0}")]
    Parse(#[source] syn::Error),
}

/// Which program point an obligation came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObligationKind {
    /// A `#[mvl::requires]` on the function itself — checked for coherence.
    Requires,
    /// A `#[mvl::ensures]` on the function itself — checked for coherence.
    Ensures,
    /// A call to `callee`, whose `requires` must be entailed by the
    /// caller's hypothesis context.
    CallSite { callee: String },
    /// A return point in the function's own body, whose returned expression
    /// must establish the function's `ensures` (#42). Distinct from
    /// [`ObligationKind::Ensures`], which only asks whether the
    /// postcondition is internally coherent.
    ReturnSite,
}

impl ObligationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ObligationKind::Requires => "requires",
            ObligationKind::Ensures => "ensures",
            ObligationKind::CallSite { .. } => "call-site requires",
            ObligationKind::ReturnSite => "return-site ensures",
        }
    }
}

/// One obligation found in a file, not yet bound to a path (the caller
/// supplies the origin when it needs a full
/// `mvl_rust_core::solver::Obligation`, mirroring how [`Diagnostic`]
/// carries a bare `Span` rather than a baked `file:line:col` string).
///
/// `predicate` is the goal to discharge — for a call site, already
/// substituted with that site's actual arguments. `hypotheses` is Γ, empty
/// for a declaration site.
#[derive(Debug, Clone)]
pub struct FoundObligation {
    pub fn_name: String,
    pub kind: ObligationKind,
    pub predicate: Predicate,
    pub hypotheses: Vec<Expr>,
    pub span: Span,
}

impl FoundObligation {
    pub fn id(&self) -> String {
        match &self.kind {
            ObligationKind::CallSite { callee } => {
                format!("{}::calls::{callee}::requires", self.fn_name)
            }
            ObligationKind::ReturnSite => format!("{}::returns::ensures", self.fn_name),
            kind => format!("{}::{}", self.fn_name, kind.as_str()),
        }
    }

    pub fn predicate_text(&self) -> String {
        self.predicate.render()
    }

    /// Renders Γ the way the predicate itself is rendered — used in
    /// diagnostics so a `Runtime` outcome says what was actually known at
    /// that point, which is usually the reason it couldn't be proven.
    pub fn hypotheses_text(&self) -> String {
        self.hypotheses
            .iter()
            .map(|h| quote::quote!(#h).to_string())
            .collect::<Vec<_>>()
            .join(" && ")
    }

    /// Matched exhaustively on purpose: a `_` arm here would route a new
    /// obligation kind to `discharge_predicate`, i.e. ask whether the
    /// predicate is *coherent* rather than whether it *holds*. That is
    /// exactly the bug #42 fixed, so the compiler is made to force the
    /// choice for whatever kind comes next.
    pub fn discharge(&self) -> DischargeResult {
        match self.kind {
            ObligationKind::CallSite { .. } | ObligationKind::ReturnSite => {
                discharge_entailment(&self.hypotheses, &self.predicate)
            }
            ObligationKind::Requires | ObligationKind::Ensures => {
                discharge_predicate(&self.predicate)
            }
        }
    }
}

/// What a same-file callee declares that its call sites need: parameter
/// names in order (to substitute arguments positionally), and its
/// contract clauses.
#[derive(Debug, Clone, Default)]
struct FnFacts {
    params: Vec<String>,
    requires: Vec<Predicate>,
    ensures: Vec<Predicate>,
}

impl FnFacts {
    fn of(item_fn: &ItemFn) -> Self {
        let mut facts = FnFacts {
            params: item_fn.sig.inputs.iter().filter_map(param_name).collect(),
            ..Default::default()
        };
        for attr in &item_fn.attrs {
            match MvlAttr::try_from_attribute(attr) {
                Some(Ok(MvlAttr::Requires(requires))) => facts.requires.push(requires.predicate),
                Some(Ok(MvlAttr::Ensures(ensures))) => facts.ensures.push(ensures.predicate),
                _ => {}
            }
        }
        facts
    }

    /// The clauses this function's own `requires` contribute to Γ inside
    /// its body. Quantified preconditions are skipped — see the module
    /// doc's scope note.
    fn hypotheses(&self) -> Vec<Expr> {
        self.requires
            .iter()
            .filter_map(|pred| match pred {
                Predicate::Expr(expr) => Some(expr.clone()),
                _ => None,
            })
            .collect()
    }
}

/// A single named parameter (`x: i32`). A `self` receiver or a pattern
/// parameter (`(a, b): (i32, i32)`) has no single name to substitute
/// positionally, so it's skipped — which makes the whole function's
/// parameter list shorter than its argument list and suppresses
/// substitution for it (see [`CallSiteScan::obligations_for_call`]).
fn param_name(arg: &FnArg) -> Option<String> {
    match arg {
        FnArg::Typed(typed) => match &*typed.pat {
            Pat::Ident(ident) => Some(ident.ident.to_string()),
            _ => None,
        },
        FnArg::Receiver(_) => None,
    }
}

struct DeclarationFinder<'o> {
    found: &'o mut Vec<FoundObligation>,
}

impl<'ast> Visit<'ast> for DeclarationFinder<'_> {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let fn_name = node.sig.ident.to_string();
        for attr in &node.attrs {
            let (kind, predicate) = match MvlAttr::try_from_attribute(attr) {
                Some(Ok(MvlAttr::Requires(requires))) => {
                    (ObligationKind::Requires, requires.predicate)
                }
                Some(Ok(MvlAttr::Ensures(ensures))) => (ObligationKind::Ensures, ensures.predicate),
                _ => continue,
            };
            self.found.push(FoundObligation {
                fn_name: fn_name.clone(),
                kind,
                predicate,
                hypotheses: Vec::new(),
                span: attr.span(),
            });
        }
        visit::visit_item_fn(self, node);
    }
}

/// Walks one function's body, carrying Γ, and records an obligation at
/// every resolvable call to a callee that declares a precondition.
///
/// Γ is a stack: entering a narrowed region pushes clauses, leaving it
/// truncates back. Only the nodes where Γ actually changes are overridden;
/// `syn`'s own traversal handles the rest.
struct CallSiteScan<'a> {
    caller: &'a str,
    functions: &'a HashMap<String, FnFacts>,
    gamma: Vec<Expr>,
    /// Names bound by a `let` in scope. A call through one of these is a
    /// local (closure, function pointer), not the same-file free function
    /// that happens to share its name.
    locals: Vec<String>,
    /// This function's own `ensures` clauses -- the goals every return point
    /// has to establish (#42). Empty for a function without a postcondition,
    /// which makes the whole return-point walk a no-op.
    ensures: &'a [Predicate],
    /// Whether the node being visited is in tail position of *this*
    /// function, i.e. whether its value becomes the return value.
    ///
    /// Defaults to cleared and is only forwarded by nodes known to pass
    /// their value outwards. That asymmetry is deliberate: a construct this
    /// scan doesn't understand then yields *no* return obligation, which is
    /// the pre-#42 behaviour (a missing check), rather than a *false* one.
    /// A spurious return-site violation is `Level::Error` and fails the
    /// build, so it is the louder mistake of the two.
    in_tail: bool,
    /// Whether an explicit `return` at this point returns from *this*
    /// function. Cleared for closure and `async` bodies, which own their own
    /// return target.
    ///
    /// Necessarily separate from [`Self::in_tail`]: the two answer different
    /// questions and disagree in both directions. A `return` inside a `while`
    /// body is not in tail position but does return from this function; a
    /// closure's trailing expression is in tail position *of the closure* but
    /// returns from neither. Reusing `in_tail` for this is what let a
    /// closure's `return -1` be reported as a violating return of its
    /// enclosing function.
    returns_here: bool,
    found: &'a mut Vec<FoundObligation>,
}

impl CallSiteScan<'_> {
    /// Drops everything Γ knew about `name`: it has just been rebound,
    /// assigned, or mutably borrowed, so those facts describe a previous
    /// value. Keeping them is how a stale hypothesis proves a false goal.
    ///
    /// Clauses are replaced by `true` rather than removed because Γ's
    /// scoping is index-based — leaving a narrowed region truncates back
    /// to a saved depth, so positions have to stay stable.
    fn invalidate(&mut self, name: &str) {
        for clause in &mut self.gamma {
            if mentions_ident(clause, name) {
                *clause = true_expr();
            }
        }
    }

    /// Every name a construct rebinds or mutates, invalidated together.
    fn invalidate_all(&mut self, names: &[String]) {
        for name in names {
            self.invalidate(name);
        }
    }

    /// Visits `expr` with the tail flag forced to `tail`, restoring it
    /// afterwards. Every override that recurses by hand instead of going
    /// through `visit::visit_expr_*` has to route through this, or the flag
    /// leaks into a position whose value is not the function's return value.
    fn visit_with_tail(&mut self, expr: &Expr, tail: bool) {
        let saved = std::mem::replace(&mut self.in_tail, tail);
        self.visit_expr(expr);
        self.in_tail = saved;
    }

    /// Same, for a block (`if`/`while` bodies arrive as `Block`, not `Expr`).
    fn visit_block_with_tail(&mut self, block: &Block, tail: bool) {
        let saved = std::mem::replace(&mut self.in_tail, tail);
        self.visit_block(block);
        self.in_tail = saved;
    }

    /// Routes an expression sitting in tail position. Control flow forwards
    /// the flag inwards and reports from whichever leaf actually produces the
    /// value; anything else *is* that value and takes the obligation here.
    ///
    /// Without the split, `fn f() -> i64 { if c { 1 } else { 2 } }` would
    /// report three times: once per branch, plus once for the whole `if` with
    /// `result` bound to the `if` expression itself.
    fn visit_tail_expr(&mut self, expr: &Expr) {
        if forwards_tail(expr) {
            self.visit_with_tail(expr, true);
        } else {
            self.obligations_for_return(Some(expr), expr.span());
            self.visit_with_tail(expr, false);
        }
    }

    /// One obligation per `ensures` clause, with `result` bound to the
    /// expression actually being returned, discharged against Γ as it stands
    /// at this point in the body (#42).
    ///
    /// `returned` is `None` for a bare `return;`, which yields nothing: there
    /// is no value to substitute, and a function with a postcondition worth
    /// checking returns something.
    fn obligations_for_return(&mut self, returned: Option<&Expr>, span: Span) {
        if self.ensures.is_empty() {
            return;
        }
        let Some(returned) = returned else { return };
        if is_diverging(returned) {
            return; // Produces no `result` -- see `is_diverging`.
        }

        let bindings: HashMap<String, Expr> =
            HashMap::from([("result".to_string(), returned.clone())]);

        for ensures in self.ensures {
            self.found.push(FoundObligation {
                fn_name: self.caller.to_string(),
                kind: ObligationKind::ReturnSite,
                predicate: substitute_exprs(ensures, &bindings),
                hypotheses: self.gamma.clone(),
                span,
            });
        }
    }

    fn obligations_for_call(&mut self, node: &ExprCall) {
        let Some(callee) = called_fn_name(&node.func) else {
            return;
        };
        if self.locals.contains(&callee) {
            return; // Shadowed by a local binding — not the free function.
        }
        let Some(facts) = self.functions.get(&callee) else {
            return; // Unresolvable (other file, other crate, a method, ...).
        };
        if facts.requires.is_empty() {
            return;
        }

        // Positional substitution needs a name for every argument. An
        // arity mismatch (or a parameter with no single name) means the
        // call can't be substituted faithfully, so nothing is claimed
        // about it rather than something wrong.
        if facts.params.len() != node.args.len() {
            return;
        }
        let bindings: HashMap<String, Expr> = facts
            .params
            .iter()
            .cloned()
            .zip(node.args.iter().cloned())
            .collect();

        for requires in &facts.requires {
            self.found.push(FoundObligation {
                fn_name: self.caller.to_string(),
                kind: ObligationKind::CallSite {
                    callee: callee.clone(),
                },
                predicate: substitute_exprs(requires, &bindings),
                hypotheses: self.gamma.clone(),
                span: node.span(),
            });
        }
    }

    /// After `let y = g(x);`, `g`'s postcondition holds with `result`
    /// bound to `y` and `g`'s parameters bound to this call's arguments.
    fn propagate_postcondition(&mut self, local: &Local) {
        let Some(init) = &local.init else { return };
        let Some(binding) = binding_name(&local.pat) else {
            return;
        };
        let Expr::Call(call) = strip_groups(&init.expr) else {
            return;
        };
        let Some(callee) = called_fn_name(&call.func) else {
            return;
        };
        if self.locals.contains(&callee) {
            return; // Shadowed by a local binding — not the free function.
        }
        let Some(facts) = self.functions.get(&callee) else {
            return;
        };

        let mut bindings: HashMap<String, Expr> = HashMap::new();
        if facts.params.len() == call.args.len() {
            bindings.extend(facts.params.iter().cloned().zip(call.args.iter().cloned()));
        }
        bindings.insert("result".to_string(), ident_expr(&binding));

        for ensures in &facts.ensures {
            if let Predicate::Expr(expr) = substitute_exprs(ensures, &bindings) {
                self.gamma.push(expr);
            }
        }
    }
}

impl<'ast> Visit<'ast> for CallSiteScan<'_> {
    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        self.obligations_for_call(node);
        visit::visit_expr_call(self, node);
    }

    /// A block scopes Γ: facts learned by `let`s inside it don't outlive it,
    /// and neither do the bindings that shadow a free function's name.
    ///
    /// It also decides tail position: only the final statement can carry the
    /// block's value outwards, and only when it is a trailing expression with
    /// no semicolon (`Stmt::Expr(e, None)`). Everything earlier is visited
    /// with the flag cleared.
    fn visit_block(&mut self, node: &'ast Block) {
        let depth = self.gamma.len();
        let locals_depth = self.locals.len();
        let block_is_tail = self.in_tail;
        let last = node.stmts.len().saturating_sub(1);

        for (i, stmt) in node.stmts.iter().enumerate() {
            match stmt {
                syn::Stmt::Expr(expr, None) if block_is_tail && i == last => {
                    self.visit_tail_expr(expr)
                }
                _ => {
                    let saved = std::mem::replace(&mut self.in_tail, false);
                    self.visit_stmt(stmt);
                    self.in_tail = saved;
                }
            }
        }
        self.gamma.truncate(depth);
        self.locals.truncate(locals_depth);
    }

    /// An explicit `return e` is a return point wherever it appears in *this*
    /// function's body, tail position or not. Γ is already correct here --
    /// branch narrowing, propagated postconditions and #40's invalidation have
    /// all been applied on the way down.
    ///
    /// A `return` inside a closure or `async` block returns from that, not
    /// from the enclosing function, so `returns_here` gates the obligation.
    fn visit_expr_return(&mut self, node: &'ast syn::ExprReturn) {
        if self.returns_here {
            self.obligations_for_return(node.expr.as_deref(), node.span());
        }
        // The returned expression may itself contain calls, which still owe
        // their own call-site obligations. It is not in tail position for
        // *this* purpose -- the obligation above already covers it.
        let saved = std::mem::replace(&mut self.in_tail, false);
        visit::visit_expr_return(self, node);
        self.in_tail = saved;
    }

    /// A closure's trailing expression is the *closure's* return value, not
    /// the enclosing function's, and so is an explicit `return` inside it.
    /// Both flags have to be cleared: `in_tail` for the trailing expression,
    /// `returns_here` for the `return`. Clearing only the first reported a
    /// closure's `return -1` as a violating return point of the enclosing
    /// function -- a build-failing error on correct code.
    fn visit_expr_closure(&mut self, node: &'ast syn::ExprClosure) {
        let saved = std::mem::replace(&mut self.returns_here, false);
        self.visit_with_tail(&node.body, false);
        self.returns_here = saved;
    }

    /// An `async` block evaluates to a future, so neither its tail nor a
    /// `return` inside it is the function's return value.
    fn visit_expr_async(&mut self, node: &'ast syn::ExprAsync) {
        let saved = std::mem::replace(&mut self.returns_here, false);
        self.visit_block_with_tail(&node.block, false);
        self.returns_here = saved;
    }

    /// A plain or `unsafe` block in tail position does pass its value
    /// outwards, so the flag is forwarded rather than cleared.
    fn visit_expr_block(&mut self, node: &'ast syn::ExprBlock) {
        self.visit_block(&node.block);
    }

    fn visit_expr_unsafe(&mut self, node: &'ast syn::ExprUnsafe) {
        self.visit_block(&node.block);
    }

    /// Each arm of a `match` in tail position is a return point. Arm patterns
    /// do not narrow Γ (see the module doc's scope note), so each arm is
    /// discharged against the enclosing Γ -- imprecise, never unsound, since
    /// a missing hypothesis can only fail to prove a goal.
    fn visit_expr_match(&mut self, node: &'ast syn::ExprMatch) {
        let arms_are_tail = self.in_tail;
        self.visit_with_tail(&node.expr, false);
        for arm in &node.arms {
            if let Some((_, guard)) = &arm.guard {
                self.visit_with_tail(guard, false);
            }
            if arms_are_tail {
                self.visit_tail_expr(&arm.body);
            } else {
                self.visit_with_tail(&arm.body, false);
            }
        }
    }

    /// The initializer is evaluated before the binding takes effect, so it
    /// sees the old Γ. Afterwards the bound names are invalidated (a `let`
    /// may shadow a variable Γ has facts about) and only then does this
    /// call's own postcondition enter Γ.
    fn visit_local(&mut self, node: &'ast Local) {
        if let Some(init) = &node.init {
            self.visit_expr(&init.expr);
            if let Some(diverge) = &init.diverge {
                self.visit_expr(&diverge.1);
            }
        }
        let bound = pattern_idents(&node.pat);
        self.invalidate_all(&bound);
        self.locals.extend(bound);
        self.propagate_postcondition(node);
    }

    /// `x = …` replaces `x`'s value, so Γ's facts about it no longer hold.
    fn visit_expr_assign(&mut self, node: &'ast syn::ExprAssign) {
        visit::visit_expr_assign(self, node);
        self.invalidate_all(&assigned_idents(&node.left));
    }

    /// Compound assignment (`x += 1`) is a `Binary` node in `syn`, not an
    /// `Assign` one, but mutates `x` just the same.
    fn visit_expr_binary(&mut self, node: &'ast syn::ExprBinary) {
        visit::visit_expr_binary(self, node);
        if is_assign_op(&node.op) {
            self.invalidate_all(&assigned_idents(&node.left));
        }
    }

    /// `&mut x` hands out the power to change `x`, and this backend can't
    /// see whether the callee uses it. Conservatively assume it does.
    fn visit_expr_reference(&mut self, node: &'ast syn::ExprReference) {
        visit::visit_expr_reference(self, node);
        if node.mutability.is_some() {
            self.invalidate_all(&assigned_idents(&node.expr));
        }
    }

    /// Branch narrowing: the condition holds in the `then` arm, its
    /// negation in the `else` arm. An `else if` chain nests through the
    /// same path, so each arm accumulates the negations of every condition
    /// before it.
    /// Both arms inherit tail position from the `if` itself, so a return
    /// point inside one is discharged against that branch's narrowed Γ. The
    /// condition never does -- its value is not returned.
    fn visit_expr_if(&mut self, node: &'ast ExprIf) {
        let arms_are_tail = self.in_tail;
        self.visit_with_tail(&node.cond, false);

        let depth = self.gamma.len();
        self.gamma.push((*node.cond).clone());
        self.visit_block_with_tail(&node.then_branch, arms_are_tail);
        self.gamma.truncate(depth);

        if let Some((_, else_branch)) = &node.else_branch {
            self.gamma.push(negate_condition(&node.cond));
            self.visit_with_tail(else_branch, arms_are_tail);
            self.gamma.truncate(depth);
        }
    }

    /// A `while` body only runs when the condition holds — the same
    /// narrowing as an `if`. Its body is never in tail position: a `while`
    /// evaluates to `()`, so nothing in it becomes the return value except
    /// through an explicit `return`, which `visit_expr_return` catches on its
    /// own and with this same narrowed Γ.
    fn visit_expr_while(&mut self, node: &'ast ExprWhile) {
        self.visit_with_tail(&node.cond, false);
        let depth = self.gamma.len();
        self.gamma.push((*node.cond).clone());
        self.visit_block_with_tail(&node.body, false);
        self.gamma.truncate(depth);
    }

    /// A nested `fn` item has its own parameters, so it gets its own Γ
    /// rather than inheriting the enclosing function's -- the two functions'
    /// identically-named parameters are different variables. Its body is
    /// still scanned, under that fresh Γ.
    ///
    /// Calls *to* it stay unresolvable: `functions` holds file-level items
    /// only, the same boundary `rust-effect` draws.
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let caller = node.sig.ident.to_string();
        // Its own contract, not the enclosing function's -- a nested `fn`'s
        // return points establish *its* `ensures`.
        let facts = FnFacts::of(node);
        let mut nested = CallSiteScan {
            caller: &caller,
            functions: self.functions,
            gamma: facts.hypotheses(),
            locals: Vec::new(),
            ensures: &facts.ensures,
            in_tail: true,
            // A nested `fn` is its own return target, even when the `fn` sits
            // inside a closure body where the enclosing scan had it cleared.
            returns_here: true,
            found: &mut *self.found,
        };
        nested.visit_block(&node.block);
    }
}

/// Whether `expr` hands its value outwards to sub-expressions rather than
/// being that value. These forward tail position and let the leaf that
/// actually produces the value carry the obligation; everything else is a
/// leaf for this purpose. `Return` is included because
/// [`CallSiteScan::visit_expr_return`] owns that case.
fn forwards_tail(expr: &Expr) -> bool {
    matches!(
        strip_groups(expr),
        Expr::Block(_) | Expr::Unsafe(_) | Expr::If(_) | Expr::Match(_) | Expr::Return(_)
    )
}

/// Macros that never produce a value, so a body ending in one has no return
/// point to check -- `fn f() -> i64 { panic!() }` establishes its
/// postcondition vacuously, the same conclusion the solver reaches for an
/// unreachable program point (ADR-0005).
///
/// `rust-total`'s `PANICKING_MACROS` covers the first three for panic-freedom;
/// `unreachable` is added here because divergence, not panicking, is what
/// matters for this question.
const DIVERGING_MACROS: &[&str] = &["panic", "todo", "unimplemented", "unreachable"];

fn is_diverging(expr: &Expr) -> bool {
    match strip_groups(expr) {
        Expr::Macro(mac) => mac
            .mac
            .path
            .segments
            .last()
            .is_some_and(|seg| DIVERGING_MACROS.contains(&seg.ident.to_string().as_str())),
        _ => false,
    }
}

/// Whether `expr` mentions `name` anywhere — the test for which Γ clauses
/// a rebinding invalidates.
fn mentions_ident(expr: &Expr, name: &str) -> bool {
    struct Search<'a> {
        name: &'a str,
        found: bool,
    }
    impl<'ast> Visit<'ast> for Search<'_> {
        fn visit_ident(&mut self, ident: &'ast proc_macro2::Ident) {
            if ident == self.name {
                self.found = true;
            }
        }
    }
    let mut search = Search { name, found: false };
    search.visit_expr(expr);
    search.found
}

fn true_expr() -> Expr {
    syn::parse_str::<Expr>("true").expect("`true` parses")
}

/// Every name a pattern binds (`let (a, b) = …` binds both).
fn pattern_idents(pat: &Pat) -> Vec<String> {
    struct Collect {
        names: Vec<String>,
    }
    impl<'ast> Visit<'ast> for Collect {
        fn visit_pat_ident(&mut self, node: &'ast syn::PatIdent) {
            self.names.push(node.ident.to_string());
            visit::visit_pat_ident(self, node);
        }
    }
    let mut collect = Collect { names: Vec::new() };
    collect.visit_pat(pat);
    collect.names
}

/// The names an assignment target touches. A plain `x` is exact; for
/// `xs[i] = …` or `s.f = …` every identifier in the target is invalidated,
/// which is imprecise but never wrong in the unsafe direction.
fn assigned_idents(target: &Expr) -> Vec<String> {
    struct Collect {
        names: Vec<String>,
    }
    impl<'ast> Visit<'ast> for Collect {
        fn visit_ident(&mut self, ident: &'ast proc_macro2::Ident) {
            self.names.push(ident.to_string());
        }
    }
    let mut collect = Collect { names: Vec::new() };
    collect.visit_expr(target);
    collect.names
}

fn is_assign_op(op: &syn::BinOp) -> bool {
    use syn::BinOp::*;
    matches!(
        op,
        AddAssign(_)
            | SubAssign(_)
            | MulAssign(_)
            | DivAssign(_)
            | RemAssign(_)
            | BitXorAssign(_)
            | BitAndAssign(_)
            | BitOrAssign(_)
            | ShlAssign(_)
            | ShrAssign(_)
    )
}

/// A bare function name in call position (`g(…)`). Anything else — a
/// method call, a path with multiple segments, a closure variable — isn't
/// resolvable by name against this file's free functions.
fn called_fn_name(func: &Expr) -> Option<String> {
    match func {
        Expr::Path(path) if path.qself.is_none() => path.path.get_ident().map(ToString::to_string),
        Expr::Paren(paren) => called_fn_name(&paren.expr),
        Expr::Group(group) => called_fn_name(&group.expr),
        _ => None,
    }
}

/// The single name a `let` binds (`let y = …`, with or without a type
/// annotation). Destructuring patterns bind no single name for `result`.
fn binding_name(pat: &Pat) -> Option<String> {
    match pat {
        Pat::Ident(ident) => Some(ident.ident.to_string()),
        Pat::Type(typed) => binding_name(&typed.pat),
        _ => None,
    }
}

fn strip_groups(expr: &Expr) -> &Expr {
    match expr {
        Expr::Paren(paren) => strip_groups(&paren.expr),
        Expr::Group(group) => strip_groups(&group.expr),
        other => other,
    }
}

fn ident_expr(name: &str) -> Expr {
    Expr::Path(syn::ExprPath {
        attrs: vec![],
        qself: None,
        path: syn::Ident::new(name, Span::call_site()).into(),
    })
}

/// `!cond`, kept in a form the solver can still read where possible: a
/// comparison flips its operator (`x > 0` → `x <= 0`), a double negation
/// cancels, and anything else becomes a plain `!(…)`.
///
/// That fallback is an *undecidable* hypothesis, which the solver drops.
/// Dropping a hypothesis only costs precision in the `else` arm — it can
/// never make an obligation wrongly provable (see the entailment section
/// of `mvl_rust_core::solver::native`).
fn negate_condition(cond: &Expr) -> Expr {
    let negated = match strip_groups(cond) {
        Expr::Binary(bin) => negated_op(&bin.op).map(|op| {
            let (left, right) = (&bin.left, &bin.right);
            format!("{} {op} {}", quote::quote!(#left), quote::quote!(#right))
        }),
        Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Not(_)) => {
            let inner = &unary.expr;
            Some(quote::quote!(#inner).to_string())
        }
        other => Some(format!("!({})", quote::quote!(#other))),
    };

    negated
        .and_then(|text| syn::parse_str::<Expr>(&text).ok())
        // Unreachable in practice (every branch above re-renders valid
        // Rust). `true` is the safe answer if it ever isn't: a hypothesis
        // that adds no information, rather than one that adds a wrong one.
        .unwrap_or_else(|| syn::parse_str::<Expr>("true").expect("`true` parses"))
}

fn negated_op(op: &syn::BinOp) -> Option<&'static str> {
    match op {
        syn::BinOp::Lt(_) => Some(">="),
        syn::BinOp::Le(_) => Some(">"),
        syn::BinOp::Gt(_) => Some("<="),
        syn::BinOp::Ge(_) => Some("<"),
        syn::BinOp::Eq(_) => Some("!="),
        syn::BinOp::Ne(_) => Some("=="),
        _ => None,
    }
}

/// Finds every obligation in `source` — declaration sites and call sites
/// both — without discharging or rendering them yet.
pub fn find_obligations(source: &str) -> Result<Vec<FoundObligation>, CheckError> {
    let file: syn::File = syn::parse_str(source).map_err(CheckError::Parse)?;

    let mut found = Vec::new();
    DeclarationFinder { found: &mut found }.visit_file(&file);

    // Same collect-then-walk shape as `rust-effect`: every function's own
    // declared facts first, so a call can be resolved against a callee
    // defined later in the file.
    let mut functions: HashMap<String, FnFacts> = HashMap::new();
    for item in &file.items {
        if let Item::Fn(item_fn) = item {
            functions.insert(item_fn.sig.ident.to_string(), FnFacts::of(item_fn));
        }
    }

    for item in &file.items {
        if let Item::Fn(item_fn) = item {
            let caller = item_fn.sig.ident.to_string();
            let facts = functions.get(&caller);
            let gamma = facts.map(FnFacts::hypotheses).unwrap_or_default();
            let ensures: &[Predicate] = facts.map(|f| f.ensures.as_slice()).unwrap_or(&[]);
            let mut scan = CallSiteScan {
                caller: &caller,
                functions: &functions,
                gamma,
                locals: Vec::new(),
                ensures,
                // The function body's own trailing expression is its return
                // value, so the walk starts in tail position -- and a `return`
                // in it returns from this function.
                in_tail: true,
                returns_here: true,
                found: &mut found,
            };
            scan.visit_block(&item_fn.block);
        }
    }

    Ok(found)
}

/// Renders one obligation's discharge outcome as a Gate-mode diagnostic.
/// Every obligation produces one, regardless of outcome, per spec
/// Requirement 3's "report which layer discharged it" UX -- `Proven`/
/// `Runtime` are informational (`Level::Note`, doesn't fail the build);
/// only `Violated` is `Level::Error`.
pub fn to_diagnostic(found: &FoundObligation, result: &DischargeResult) -> Diagnostic {
    match &found.kind {
        ObligationKind::CallSite { callee } => call_site_diagnostic(found, callee, result),
        ObligationKind::ReturnSite => return_site_diagnostic(found, result),
        ObligationKind::Requires | ObligationKind::Ensures => declaration_diagnostic(found, result),
    }
}

/// A return site's diagnostic quotes the *substituted* postcondition, so the
/// returned expression appears in place of `result` and the reader sees the
/// claim that actually failed. Mirrors [`call_site_diagnostic`], including
/// naming what was known at that point on a `Runtime` outcome.
fn return_site_diagnostic(found: &FoundObligation, result: &DischargeResult) -> Diagnostic {
    let name = &found.fn_name;
    match result {
        DischargeResult::Proven { layer } => Diagnostic::new(
            Level::Note,
            format!(
                "`{name}` postcondition `{}` established at {} by this return",
                found.predicate_text(),
                layer_str(*layer)
            ),
            found.span,
        )
        .with_label("proven"),
        DischargeResult::Runtime => {
            let known = found.hypotheses_text();
            let known = if known.is_empty() {
                "nothing is known here".to_string()
            } else {
                format!("known here: {known}")
            };
            Diagnostic::new(
                Level::Note,
                format!(
                    "`{name}` postcondition `{}` is not established by this return ({known}), \
                     inserting a runtime check",
                    found.predicate_text()
                ),
                found.span,
            )
            .with_label("runtime fallback")
        }
        DischargeResult::Violated { counterexample } => Diagnostic::new(
            Level::Error,
            format!(
                "`{name}` postcondition `{}` cannot hold for this return value: {counterexample}",
                found.predicate_text()
            ),
            found.span,
        )
        .with_label("postcondition violated"),
    }
}

fn declaration_diagnostic(found: &FoundObligation, result: &DischargeResult) -> Diagnostic {
    match result {
        DischargeResult::Proven { layer } => Diagnostic::new(
            Level::Note,
            format!(
                "`{}` {} discharged at {}",
                found.fn_name,
                found.kind.as_str(),
                layer_str(*layer)
            ),
            found.span,
        )
        .with_label("proven"),
        DischargeResult::Runtime => Diagnostic::new(
            Level::Note,
            format!(
                "`{}` {} could not be discharged by L1-L2, inserting a runtime check",
                found.fn_name,
                found.kind.as_str()
            ),
            found.span,
        )
        .with_label("runtime fallback"),
        DischargeResult::Violated { counterexample } => Diagnostic::new(
            Level::Error,
            format!(
                "`{}` {} is violated: {counterexample}",
                found.fn_name,
                found.kind.as_str()
            ),
            found.span,
        )
        .with_label("unsatisfiable"),
    }
}

/// A call site's diagnostic names both ends of the call and, when the
/// obligation couldn't be closed, what was actually known at that point --
/// an empty Γ is by far the most common reason a call site falls to a
/// runtime check, and saying so is more useful than naming the layers that
/// failed.
fn call_site_diagnostic(
    found: &FoundObligation,
    callee: &str,
    result: &DischargeResult,
) -> Diagnostic {
    let caller = &found.fn_name;
    match result {
        DischargeResult::Proven { layer } => Diagnostic::new(
            Level::Note,
            format!(
                "`{callee}` precondition `{}` proven at {} for this call",
                found.predicate_text(),
                layer_str(*layer)
            ),
            found.span,
        )
        .with_label("proven"),
        DischargeResult::Runtime => {
            let known = found.hypotheses_text();
            let known = if known.is_empty() {
                "nothing is known about the arguments here".to_string()
            } else {
                format!("known here: {known}")
            };
            Diagnostic::new(
                Level::Note,
                format!(
                    "`{callee}` precondition `{}` is not entailed in `{caller}` ({known}), \
                     inserting a runtime check",
                    found.predicate_text()
                ),
                found.span,
            )
            .with_label("runtime fallback")
        }
        DischargeResult::Violated { counterexample } => Diagnostic::new(
            Level::Error,
            format!(
                "`{callee}` precondition `{}` can never hold for this call from `{caller}`: \
                 {counterexample}",
                found.predicate_text()
            ),
            found.span,
        )
        .with_label("precondition violated"),
    }
}

fn layer_str(layer: Layer) -> &'static str {
    match layer {
        Layer::L1 => "L1",
        Layer::L2 => "L2",
        Layer::L3 => "L3",
        Layer::L4 => "L4",
        Layer::L5 => "L5",
        Layer::Runtime => "runtime",
    }
}

/// Gate-mode entry point: finds every obligation and reports its
/// discharge outcome as a [`Diagnostic`]. Matches `rust-limit`/
/// `rust-total`'s `check_source(source: &str) -> Result<Vec<Diagnostic>,
/// _>` shape so `cargo-mvl` can dispatch to it identically.
pub fn check_source(source: &str) -> Result<Vec<Diagnostic>, CheckError> {
    let found = find_obligations(source)?;
    Ok(found
        .iter()
        .map(|f| to_diagnostic(f, &f.discharge()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_requires_and_ensures_on_a_function() {
        let found = find_obligations(
            "#[mvl::total]\n\
             #[mvl::requires(0 <= b && b <= 255)]\n\
             #[mvl::ensures(0 <= result && result <= 15)]\n\
             fn mask_low_nibble(b: i32) -> i32 { b & 15 }",
        )
        .unwrap();

        // Three since #42: the two declaration-site coherence checks, plus
        // the return-site obligation for the body's `b & 15`. Declaration
        // sites come first -- `DeclarationFinder` runs over the whole file
        // before any body is scanned.
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].kind, ObligationKind::Requires);
        assert_eq!(found[0].fn_name, "mask_low_nibble");
        assert_eq!(found[1].kind, ObligationKind::Ensures);
        assert_eq!(found[2].kind, ObligationKind::ReturnSite);
        assert_eq!(found[2].fn_name, "mask_low_nibble");
    }

    #[test]
    fn function_with_no_refinement_attrs_finds_nothing() {
        let found = find_obligations("fn f(x: i32) -> i32 { x }").unwrap();
        assert!(found.is_empty());
    }

    #[test]
    fn satisfiable_requires_is_a_note_not_an_error() {
        let diagnostics =
            check_source("#[mvl::requires(x >= 0 && x < 100)]\nfn f(x: i32) {}").unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].level, Level::Note);
    }

    #[test]
    fn contradictory_requires_is_an_error() {
        let diagnostics =
            check_source("#[mvl::requires(x >= 10 && x < 5)]\nfn f(x: i32) {}").unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].level, Level::Error);
    }

    #[test]
    fn bounded_quantifier_predicate_is_found_and_discharged_at_l3() {
        let diagnostics =
            check_source("#[mvl::requires(forall i in [0..9] . i < 10)]\nfn f(sections: i32) {}")
                .unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].level, Level::Note);
        assert!(diagnostics[0].message.contains("L3"));
    }

    #[test]
    fn bounded_quantifier_over_an_opaque_call_falls_to_runtime() {
        // Matches the `require_dense_fleet` shape: L3 unrolling doesn't
        // spuriously "prove" what the inner backend can't decide.
        let diagnostics = check_source(
            "#[mvl::requires(forall i in [1..50] . sections_get(i) != 0)]\nfn f(sections: i32) {}",
        )
        .unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].level, Level::Note);
        assert!(diagnostics[0].message.contains("runtime check"));
    }

    #[test]
    fn genuinely_unsatisfiable_quantifier_fails_the_build() {
        let diagnostics =
            check_source("#[mvl::requires(forall i in [0..9] . i < 5)]\nfn f() {}").unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].level, Level::Error);
        assert!(diagnostics[0].message.contains("i = 5"));
    }
}
