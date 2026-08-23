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
//! **Impl methods are checked too** (declaration- and return-site
//! obligations only — closing ADR-0001's "largest practical coverage gap":
//! methods were previously invisible to every annotation-consuming check
//! end to end). A method's obligation id is qualified `Type::method` from
//! its enclosing `impl` block's `Self` type ([`impl_self_type_name`]), so
//! it can't collide with a free function or another impl's identically
//! named method. This does *not* extend call resolution, per the next
//! bullet — see it for what's still unreached.
//!
//! Scope, otherwise deliberately the same boundary `rust-effect` (#9) draws
//! for the same reason — `syn`-based scanning has no type information and
//! no cross-file resolution:
//!
//! - Call resolution is **same-file, free functions only**. A call to
//!   anything else — including a method call (`self.foo()`, `x.method()`,
//!   or `Type::method(x)`), even now that the method itself is checked
//!   above — is silently unresolvable and produces no obligation.
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
//!   tail position. A construct not on that list is **substituted whole** —
//!   `loop { break -5; }` becomes the goal `(loop { break - 5 ; }) > 0` —
//!   which the solver cannot decide, so it falls to a runtime outcome (#48).
//!
//!   This is not the same as yielding nothing, and the difference now matters.
//!   Since #47 a function's postcondition propagates only when *every* return
//!   site closed, and that test is an `all()` — so a function with **zero**
//!   return-site obligations is treated as closed. Skipping unmodelled
//!   constructs, which earlier versions of this doc claimed happened, would
//!   therefore mark `fn f() -> i64 { loop { break -5; } }` closed and
//!   propagate `result > 0` from a body returning `-5`. The undecidable
//!   obligation is what keeps that honest.
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
use mvl_rust_core::impl_methods::{flatten_items, impl_methods};
use mvl_rust_core::solver::native::{discharge_entailment, discharge_predicate, substitute_exprs};
use mvl_rust_core::solver::{DischargeResult, Layer, ObligationClass, Warrant};
use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Block, Expr, ExprCall, ExprIf, ExprWhile, FnArg, Item, ItemFn, Local, Pat, Type};
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
    /// Parallel to `hypotheses`, same length, same index (#69): `Some(callee)`
    /// when that hypothesis is `callee`'s postcondition, propagated via the
    /// *enforced* (not statically proven) half of the relaxed `closed` gate
    /// — see `return_site_closure`. `None` for every other kind of
    /// hypothesis (the function's own `requires`, branch narrowing, a
    /// propagated postcondition from a *proven*-closed callee) — these are
    /// genuinely established facts, not resting on a runtime check.
    pub hypothesis_provenance: Vec<Option<String>>,
    /// Whether this obligation's own target function -- the callee for a
    /// call site, the function itself for a return site -- carries the
    /// relevant contract attribute and is not `#[mvl::unchecked]` (#69).
    /// Unused (always `false`) for a declaration-site obligation, which has
    /// no per-site enforcement concept to rest on.
    pub enforced: bool,
    pub span: Span,
    /// Which occurrence this is among the obligations in `fn_name` sharing
    /// its `id_stem` — 0-based, in visit order. Assigned by
    /// `number_occurrences` after the walk, not at the push sites; see
    /// there for why.
    pub occurrence: usize,
}

impl FoundObligation {
    /// This obligation's address within the report (#51).
    ///
    /// The occurrence suffix is what makes it an address rather than a
    /// description. Without it two `requires` clauses on one function, two
    /// calls to the same callee, or two return points all collide, and a
    /// certification reviewer following [`AssuranceLeaf::obligation_id`]
    /// back from a leaf cannot tell which obligation it meant.
    ///
    /// [`AssuranceLeaf::obligation_id`]: mvl_rust_core::assurance::schema::AssuranceLeaf::obligation_id
    ///
    /// The suffix is present **uniformly**, including on `#0`. Omitting it
    /// for the single-occurrence case would read as a distinction that
    /// isn't there: `caller::requires` beside `caller::requires#1` gives no
    /// way to tell "the only one" from "the first of several".
    pub fn id(&self) -> String {
        format!("{}#{}", self.id_stem(), self.occurrence)
    }

    /// This obligation's wire-facing classification (#56) — which question
    /// the solver was asked, so the report can stop presenting a coherence
    /// check and an entailment proof identically.
    ///
    /// Both declaration kinds collapse to
    /// [`ObligationClass::Declaration`]: `requires` and `ensures` differ in
    /// *which* predicate is checked, not in the question asked of it, and
    /// which one it was is already in the id.
    pub fn class(&self) -> ObligationClass {
        match &self.kind {
            ObligationKind::Requires | ObligationKind::Ensures => ObligationClass::Declaration,
            ObligationKind::CallSite { .. } => ObligationClass::CallSite,
            ObligationKind::ReturnSite => ObligationClass::ReturnSite,
        }
    }

    /// The id without its occurrence suffix — the part shared by colliding
    /// obligations, and so the key `number_occurrences` groups on.
    fn id_stem(&self) -> String {
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

    /// What actually backs this obligation's outcome (#69, spec 007
    /// Requirement 6) — see [`Warrant`]'s own doc comment for why this axis
    /// exists and why upstream never needed it.
    ///
    /// A declaration-site obligation has no Γ and no per-site enforcement
    /// concept: `Proof` if `discharge()` is `Proven`, `None` otherwise,
    /// never `Enforcement`.
    ///
    /// For an entailment obligation (`CallSite`/`ReturnSite`):
    /// - `Violated` is always `None` — a demonstrated counterexample is a
    ///   real defect the enforcement backstop doesn't excuse; propagation
    ///   soundness (ADR-0006 §5) is about executions that *return*
    ///   normally, and a violated obligation's runtime consequence is an
    ///   abort, not a silent bad value.
    /// - `Runtime` becomes `Enforcement { premises: [target] }` exactly
    ///   when `self.enforced` — the direct case: no static proof at this
    ///   site, but a real `assert!` exists for it regardless (the callee's
    ///   `requires`, or this function's own `ensures`).
    /// - `Proven` is re-checked in `warrant_for_proof`, which is
    ///   where the exactness guarantee (and its one documented limit) live.
    pub fn warrant(&self) -> Warrant {
        if !self.class().is_entailment() {
            return match self.discharge() {
                DischargeResult::Proven { .. } => Warrant::Proof,
                _ => Warrant::None,
            };
        }
        match self.discharge() {
            DischargeResult::Violated { .. } => Warrant::None,
            DischargeResult::Runtime => {
                if self.enforced {
                    Warrant::Enforcement {
                        premises: vec![self.enforcement_target()],
                    }
                } else {
                    Warrant::None
                }
            }
            DischargeResult::Proven { .. } => self.warrant_for_proof(),
        }
    }

    /// The function this obligation's own enforcement is about — the
    /// callee for a call site (its `requires` fires there), this function
    /// itself for a return site (its `ensures` fires on its own return).
    fn enforcement_target(&self) -> String {
        match &self.kind {
            ObligationKind::CallSite { callee } => callee.clone(),
            ObligationKind::ReturnSite => self.fn_name.clone(),
            ObligationKind::Requires | ObligationKind::Ensures => {
                unreachable!("declaration kinds return from `warrant` before reaching this")
            }
        }
    }

    /// Determines whether a statically `Proven` outcome rests on any
    /// enforced-not-proven Γ hypothesis, and if so, names exactly which
    /// functions it depends on.
    ///
    /// **The yes/no question is exact.** The untainted (non-enforced)
    /// hypotheses are re-discharged alone first: if that subset already
    /// proves the goal, every tainted hypothesis actually present was a red
    /// herring — this is a real `Proof`, full stop, regardless of what else
    /// happened to be in Γ. This is what makes a proof that merely
    /// *coexists* with an unrelated enforced fact indistinguishable from
    /// one derived without #69 ever having relaxed the gate.
    ///
    /// **Naming *which* premises is exact whenever they are individually
    /// necessary**, found by leave-one-out against the *full* hypothesis
    /// set: removing a genuinely necessary premise alone must break the
    /// proof, by monotonicity of interval/Fourier–Motzkin reasoning (adding
    /// a hypothesis can only prove more, never less) — so this check cannot
    /// under- or over-report a premise that is individually load-bearing.
    ///
    /// **One documented limitation**: two enforced premises that are only
    /// *jointly* sufficient as alternatives (either alone would suffice, so
    /// neither is individually necessary) are not disentangled into "the"
    /// minimal explanation — native discharge has no notion of one. The
    /// remaining candidates are added back in scan order until the result
    /// is sufficient, giving a real, sufficient witness set — just not
    /// guaranteed to be the globally smallest one in that specific case.
    fn warrant_for_proof(&self) -> Warrant {
        let tainted: Vec<usize> = self
            .hypothesis_provenance
            .iter()
            .enumerate()
            .filter_map(|(i, p)| p.is_some().then_some(i))
            .collect();
        if tainted.is_empty() {
            return Warrant::Proof;
        }

        let subset_of = |indices: &[usize]| -> Vec<Expr> {
            self.hypotheses
                .iter()
                .enumerate()
                .filter(|(i, _)| indices.contains(i))
                .map(|(_, e)| e.clone())
                .collect()
        };
        let is_proven = |hyps: &[Expr]| {
            matches!(
                discharge_entailment(hyps, &self.predicate),
                DischargeResult::Proven { .. }
            )
        };

        let clean: Vec<usize> = (0..self.hypotheses.len())
            .filter(|i| !tainted.contains(i))
            .collect();
        if is_proven(&subset_of(&clean)) {
            // Every tainted hypothesis was a red herring: not needed.
            return Warrant::Proof;
        }

        // At least one enforced premise is genuinely necessary. Find every
        // one that is *individually* necessary first (exact).
        let mut chosen: Vec<usize> = tainted
            .iter()
            .copied()
            .filter(|&i| {
                let without_i: Vec<usize> = clean
                    .iter()
                    .copied()
                    .chain(tainted.iter().copied().filter(|&j| j != i))
                    .collect();
                !is_proven(&subset_of(&without_i))
            })
            .collect();

        // The individually-necessary set might not yet be sufficient on its
        // own (independently-sufficient alternatives) -- add the rest back
        // in scan order until it is. Guaranteed to terminate sufficient:
        // once every tainted index is included, `chosen ∪ clean` is the
        // full hypothesis set, which `self.discharge()` already proved.
        for &i in &tainted {
            let full_subset: Vec<usize> = clean
                .iter()
                .copied()
                .chain(chosen.iter().copied())
                .collect();
            if is_proven(&subset_of(&full_subset)) {
                break;
            }
            if !chosen.contains(&i) {
                chosen.push(i);
            }
        }

        let mut premises: Vec<String> = Vec::new();
        for &i in &chosen {
            if let Some(name) = &self.hypothesis_provenance[i] {
                if !premises.contains(name) {
                    premises.push(name.clone());
                }
            }
        }
        Warrant::Enforcement { premises }
    }
}

/// What a same-file callee declares that its call sites need: parameter
/// names in order (to substitute arguments positionally), and its
/// contract clauses.
#[derive(Debug, Clone, Default)]
struct FnFacts {
    params: Vec<String>,
    /// `params` entries whose declared type is an unsigned integer
    /// (`u8`/`u16`/`u32`/`u64`/`u128`/`usize`), mapped to that type's bit
    /// width — used by [`FnFacts::hypotheses`] to inject the implicit
    /// `param >= 0` bound the type carries for free (#94), and by
    /// [`strip_safe_widening_casts`] (#113) to know which `expr as T`
    /// casts are safe to see through (`T`'s width `>=` the parameter's).
    /// `self`'s own fields are explicitly out of scope here (tracked
    /// separately, #95).
    unsigned_param_widths: HashMap<String, u32>,
    requires: Vec<Predicate>,
    ensures: Vec<Predicate>,
    /// Whether this function carries `#[mvl::unchecked]` (#69) — `requires`/
    /// `ensures` on it inject nothing (`mvl-macros`, #53), so its contract
    /// is declared but not actually backed by a runtime check.
    unchecked: bool,
}

impl FnFacts {
    fn of(item_fn: &ItemFn) -> Self {
        Self::from_attrs_and_sig(&item_fn.attrs, &item_fn.sig)
    }

    /// Same extraction, reached through an `impl` method (`syn::ImplItemFn`)
    /// instead of a free function -- the two share the same `attrs`/`sig`
    /// shape, so this is `of`'s only real difference.
    fn of_method(method: &syn::ImplItemFn) -> Self {
        Self::from_attrs_and_sig(&method.attrs, &method.sig)
    }

    fn from_attrs_and_sig(attrs: &[syn::Attribute], sig: &syn::Signature) -> Self {
        let mut facts = FnFacts {
            params: sig.inputs.iter().filter_map(param_name).collect(),
            unsigned_param_widths: sig
                .inputs
                .iter()
                .filter_map(unsigned_param_name_and_width)
                .collect(),
            ..Default::default()
        };
        for attr in attrs {
            match MvlAttr::try_from_attribute(attr) {
                Some(Ok(MvlAttr::Requires(requires))) => facts.requires.push(requires.predicate),
                Some(Ok(MvlAttr::Ensures(ensures))) => facts.ensures.push(ensures.predicate),
                Some(Ok(MvlAttr::Unchecked(_))) => facts.unchecked = true,
                _ => {}
            }
        }
        facts
    }

    /// Whether this function's `ensures` is actually backed by a runtime
    /// check — present *and* not opted out (#69). Only meaningful when
    /// `ensures` is non-empty; a function with no postcondition has nothing
    /// to be enforced.
    fn ensures_enforced(&self) -> bool {
        !self.ensures.is_empty() && !self.unchecked
    }

    /// The clauses this function's own `requires` contribute to Γ inside
    /// its body, plus the implicit `param >= 0` bound each unsigned
    /// parameter carries for free (#94). Quantified preconditions are
    /// skipped — see the module doc's scope note.
    fn hypotheses(&self) -> Vec<Expr> {
        self.requires
            .iter()
            .filter_map(|pred| match pred {
                Predicate::Expr(expr) => Some(expr.clone()),
                _ => None,
            })
            .chain(self.unsigned_param_widths.keys().map(|name| {
                syn::parse_str::<Expr>(&format!("{name} >= 0"))
                    .expect("ident >= 0 literal always parses")
            }))
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

/// The bit width of a recognized unsigned integer type name, matched
/// conservatively on the last path segment (no type inference — a
/// qualified path like `std::primitive::u32` still matches, a type
/// alias for one does not). `usize` is treated as 64-bit -- this
/// backend has no target-pointer-width concept, and assuming the wider,
/// common case is the conservative direction for a *source* width (it
/// only makes fewer casts look "safe", never more) even though it's the
/// unsafe direction for a *target* width; `usize` is excluded as a cast
/// target for that reason, see [`strip_safe_widening_casts`].
fn unsigned_type_width(ty: &Type) -> Option<u32> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    match type_path.path.segments.last()?.ident.to_string().as_str() {
        "u8" => Some(8),
        "u16" => Some(16),
        "u32" => Some(32),
        "u64" | "usize" => Some(64),
        "u128" => Some(128),
        _ => None,
    }
}

/// The name and bit width of `arg` if it is a plain-identifier parameter
/// declared with an unsigned integer type. #94's implicit non-negative
/// bound, and #113's safe-cast stripping, both apply only to bare
/// parameters; `self` is out of scope (#95).
fn unsigned_param_name_and_width(arg: &FnArg) -> Option<(String, u32)> {
    let FnArg::Typed(typed) = arg else {
        return None;
    };
    let Pat::Ident(ident) = &*typed.pat else {
        return None;
    };
    let width = unsigned_type_width(&typed.ty)?;
    Some((ident.ident.to_string(), width))
}

/// Rewrites `expr`, recursing only through arithmetic-shaped nodes
/// (`+`/`-`/`*`, unary negation, parens/groups, and casts themselves --
/// the same shape [`mvl_rust_core::solver::native`]'s linear-term
/// extraction recurses through), dropping any `expr as T` that wraps a
/// known unsigned parameter whose declared width is `<=` `T`'s.
///
/// Sound because the value of an unsigned integer is unchanged by
/// widening it — `page_size as u64` and `page_size` denote the same
/// number when `page_size: u32`, so treating them as the same solver
/// variable (#95's `variable_key`) is exactly as safe as not casting at
/// all. A *narrowing* cast is not stripped (can truncate, changing the
/// value) — this only ever widens, never removes information the
/// solver needs, and never treats a target of `usize` as a safe widening
/// (its own width is target-dependent, unlike every fixed-width type
/// here) even though it treats `usize` as a 64-bit *source*.
///
/// Anything outside this arithmetic shape (a method call, a struct
/// field's own cast, a function call's argument) is left exactly as
/// written — conservative, matching this backend's bail-on-anything-else
/// convention elsewhere; see #113's follow-up note for field projections.
fn strip_safe_widening_casts(expr: &Expr, unsigned_param_widths: &HashMap<String, u32>) -> Expr {
    match expr {
        Expr::Cast(cast) => {
            let inner = strip_safe_widening_casts(&cast.expr, unsigned_param_widths);
            let target_is_safe_widening_target = unsigned_type_width(&cast.ty)
                .filter(|_| !matches!(&*cast.ty, Type::Path(p) if p.path.is_ident("usize")));
            if let (Expr::Path(path), Some(target_width)) = (&inner, target_is_safe_widening_target)
            {
                if let Some(ident) = path.path.get_ident() {
                    if let Some(&source_width) = unsigned_param_widths.get(&ident.to_string()) {
                        if source_width <= target_width {
                            return inner;
                        }
                    }
                }
            }
            let mut cast = cast.clone();
            cast.expr = Box::new(inner);
            Expr::Cast(cast)
        }
        Expr::Binary(bin) => {
            let mut bin = bin.clone();
            bin.left = Box::new(strip_safe_widening_casts(&bin.left, unsigned_param_widths));
            bin.right = Box::new(strip_safe_widening_casts(&bin.right, unsigned_param_widths));
            Expr::Binary(bin)
        }
        Expr::Unary(unary) => {
            let mut unary = unary.clone();
            unary.expr = Box::new(strip_safe_widening_casts(
                &unary.expr,
                unsigned_param_widths,
            ));
            Expr::Unary(unary)
        }
        Expr::Paren(paren) => {
            let mut paren = paren.clone();
            paren.expr = Box::new(strip_safe_widening_casts(
                &paren.expr,
                unsigned_param_widths,
            ));
            Expr::Paren(paren)
        }
        Expr::Group(group) => {
            let mut group = group.clone();
            group.expr = Box::new(strip_safe_widening_casts(
                &group.expr,
                unsigned_param_widths,
            ));
            Expr::Group(group)
        }
        other => other.clone(),
    }
}

/// [`strip_safe_widening_casts`], lifted over a whole [`Predicate`] --
/// applied *after* [`substitute_exprs`], so it reaches a cast sitting in
/// the `ensures` text itself (`result <= page_size as u64`) exactly the
/// same way it reaches one in the substituted return expression.
fn strip_safe_widening_casts_predicate(
    pred: Predicate,
    unsigned_param_widths: &HashMap<String, u32>,
) -> Predicate {
    match pred {
        Predicate::Expr(expr) => {
            Predicate::Expr(strip_safe_widening_casts(&expr, unsigned_param_widths))
        }
        Predicate::Forall { var, lo, hi, body } => Predicate::Forall {
            var,
            lo,
            hi,
            body: Box::new(strip_safe_widening_casts_predicate(
                *body,
                unsigned_param_widths,
            )),
        },
        Predicate::Exists { var, lo, hi, body } => Predicate::Exists {
            var,
            lo,
            hi,
            body: Box::new(strip_safe_widening_casts_predicate(
                *body,
                unsigned_param_widths,
            )),
        },
    }
}

/// The declaration-site obligations (`#[mvl::requires]`/`#[mvl::ensures]`
/// coherence) for every impl method in the file -- [`DeclarationFinder`]'s
/// `syn::visit::Visit` walk never reaches these (it overrides
/// `visit_item_fn`, not `visit_impl_item_fn`), so they're collected
/// separately rather than by adding impl-tracking state to that visitor.
fn find_method_declarations(file: &syn::File, found: &mut Vec<FoundObligation>) {
    for (name, method) in impl_methods(file) {
        for attr in &method.attrs {
            let (kind, predicate) = match MvlAttr::try_from_attribute(attr) {
                Some(Ok(MvlAttr::Requires(requires))) => {
                    (ObligationKind::Requires, requires.predicate)
                }
                Some(Ok(MvlAttr::Ensures(ensures))) => (ObligationKind::Ensures, ensures.predicate),
                _ => continue,
            };
            found.push(FoundObligation {
                fn_name: name.clone(),
                kind,
                predicate,
                hypotheses: Vec::new(),
                hypothesis_provenance: Vec::new(),
                enforced: false,
                span: attr.span(),
                occurrence: 0,
            });
        }
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
                hypothesis_provenance: Vec::new(),
                // A declaration-site coherence check has no Γ and so
                // nothing to be enforced against -- see `warrant`'s doc
                // comment (#69). Unused for this kind, but always `false`
                // rather than left ambiguous.
                enforced: false,
                span: attr.span(),
                // Assigned by `number_occurrences` once the walk is over.
                occurrence: 0,
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
    /// Parallel to `gamma`, same length, same index (#69): `Some(callee)`
    /// wherever that Γ clause is `callee`'s postcondition, pushed via the
    /// *enforced* (not statically proven) half of the relaxed closure gate.
    /// `None` for every other clause. Threaded through every push/truncate
    /// site `gamma` itself goes through, so a [`FoundObligation`] snapshot
    /// of one always has a same-shaped snapshot of the other.
    gamma_provenance: Vec<Option<String>>,
    /// Names bound by a `let` in scope. A call through one of these is a
    /// local (closure, function pointer), not the same-file free function
    /// that happens to share its name.
    locals: Vec<String>,
    /// This function's own `ensures` clauses -- the goals every return point
    /// has to establish (#42). Empty for a function without a postcondition,
    /// which makes the whole return-point walk a no-op.
    ensures: &'a [Predicate],
    /// This function's own unsigned parameters and their widths -- used by
    /// [`CallSiteScan::obligations_for_return`] to strip a safe widening
    /// cast from the returned expression before substituting it for
    /// `result` (#113), the same source [`FnFacts::hypotheses`] already
    /// reads for #94's implicit `>= 0` bound. `None` when there are no
    /// known facts for this function at all (matches `ensures`'s own
    /// `unwrap_or(&[])` fallback elsewhere).
    unsigned_param_widths: Option<&'a HashMap<String, u32>>,
    /// Whether *this* function (the one being scanned) carries
    /// `#[mvl::unchecked]` (#69) -- used to compute `enforced` on a
    /// return-site [`FoundObligation`], which asks whether an unproven
    /// postcondition is at least backed by a real runtime check.
    self_unchecked: bool,
    /// Whether the node being visited is in tail position of *this*
    /// function, i.e. whether its value becomes the return value.
    ///
    /// Defaults to cleared and is only forwarded by nodes known to pass their
    /// value outwards. That asymmetry is deliberate, but note what it does and
    /// does not buy (#48): clearing the flag stops the scan descending into a
    /// position whose value is not returned. It does **not** mean an unmodelled
    /// tail expression is skipped — [`Self::visit_tail_expr`] substitutes such
    /// an expression whole, producing an obligation the solver cannot decide.
    ///
    /// That is the safe direction for two reasons. A spurious return-site
    /// *violation* is `Level::Error` and fails the build, so guessing is the
    /// louder mistake; and an undecidable obligation still prevents the
    /// function being credited as closed, which is what stops its postcondition
    /// propagating unearned (#47).
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
    /// Which callees have established their own postconditions, and how,
    /// keyed by name (#47, relaxed by #69). `Some(map)` is the real walk: a
    /// callee's `ensures` may enter Γ when `map[callee]` is
    /// [`ClosureKind::Proven`] (cleanly) or [`ClosureKind::Enforced`]
    /// (tainted -- see [`Self::propagate_postcondition`]). `None` marks the
    /// pre-pass that builds that map, where nothing propagates at all --
    /// see `return_site_closure`.
    closed: Option<&'a HashMap<String, ClosureKind>>,
    found: &'a mut Vec<FoundObligation>,
}

/// Whether a function's postcondition may propagate into a caller's Γ, and
/// whether doing so taints the result (#69).
///
/// Before #69 this was a bare `bool`: propagation required every return
/// site to be statically [`DischargeResult::Proven`]. That is still
/// [`ClosureKind::Proven`], unchanged. [`ClosureKind::Enforced`] is the
/// relaxation ADR-0006 §5 condition 5 anticipated: the function carries
/// `#[mvl::ensures]`, is not `#[mvl::unchecked]`, and so is backed by a
/// real runtime check regardless of what the static solver concluded about
/// any individual return site -- an `assert!` at every return point makes
/// "either the postcondition holds, or the process aborted before
/// returning" true unconditionally, which is exactly the soundness
/// argument the static-only case relies on, just resting on enforcement
/// instead of proof. [`ClosureKind::Open`] is everything else (no
/// `ensures`, `unchecked`, or a returns-list this scan somehow left
/// unresolved) -- not safe to propagate at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClosureKind {
    Proven,
    Enforced,
    Open,
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
        for (i, clause) in self.gamma.iter_mut().enumerate() {
            if mentions_ident(clause, name) {
                *clause = true_expr();
                // The clause is now a vacuous `true`, not `callee`'s
                // postcondition any more -- nothing to taint (#69).
                self.gamma_provenance[i] = None;
            }
        }
    }

    /// Every name a construct rebinds or mutates, invalidated together.
    fn invalidate_all(&mut self, names: &[String]) {
        for name in names {
            self.invalidate(name);
        }
    }

    /// Runs `f` with `bound` shadowed: Γ's facts about those names are retired
    /// for the duration, then restored (#50).
    ///
    /// Restoring matters. `for x in -5..0 { … }` rebinds `x` only inside the
    /// loop, so a blanket invalidation would also lose a fact that is still
    /// true afterwards — sound, but it would quietly disable call-site
    /// checking for any function that ever shadows a parameter name.
    ///
    /// Γ is cloned rather than truncated because [`Self::invalidate`] rewrites
    /// clauses in place (positions must stay stable for the index-based block
    /// scoping), so there is nothing to pop.
    fn with_shadowed<F>(&mut self, bound: &[String], f: F)
    where
        F: FnOnce(&mut Self),
    {
        if bound.is_empty() {
            f(self);
            return;
        }
        let saved_gamma = self.gamma.clone();
        let saved_gamma_provenance = self.gamma_provenance.clone();
        let saved_locals = self.locals.len();
        self.invalidate_all(bound);
        self.locals.extend(bound.iter().cloned());
        f(self);
        self.gamma = saved_gamma;
        self.gamma_provenance = saved_gamma_provenance;
        self.locals.truncate(saved_locals);
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
            // Substitute first, then strip safe widening casts from the
            // *whole* resulting predicate -- the cast can equally sit in
            // the `ensures` text itself (`result <= page_size as u64`) as
            // in the substituted return expression, and stripping only one
            // side leaves the other unrecognized by the solver (#113).
            let predicate = substitute_exprs(ensures, &bindings);
            let predicate = match self.unsigned_param_widths {
                Some(widths) => strip_safe_widening_casts_predicate(predicate, widths),
                None => predicate,
            };
            self.found.push(FoundObligation {
                fn_name: self.caller.to_string(),
                kind: ObligationKind::ReturnSite,
                predicate,
                hypotheses: self.gamma.clone(),
                hypothesis_provenance: self.gamma_provenance.clone(),
                // `self.ensures` is non-empty (checked above), so this
                // function's postcondition is enforced exactly when it
                // isn't `#[mvl::unchecked]` (#69).
                enforced: !self.self_unchecked,
                span,
                // Assigned by `number_occurrences` once the walk is over.
                occurrence: 0,
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

        // `facts.requires` is non-empty (checked above), so the callee's
        // precondition is enforced exactly when it isn't `#[mvl::unchecked]`
        // (#69).
        let enforced = !facts.unchecked;
        for requires in &facts.requires {
            self.found.push(FoundObligation {
                fn_name: self.caller.to_string(),
                kind: ObligationKind::CallSite {
                    callee: callee.clone(),
                },
                predicate: substitute_exprs(requires, &bindings),
                hypotheses: self.gamma.clone(),
                hypothesis_provenance: self.gamma_provenance.clone(),
                enforced,
                span: node.span(),
                // Assigned by `number_occurrences` once the walk is over.
                occurrence: 0,
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

        // Γ's soundness invariant (ADR-0006 §5): a fact enters Γ only if it
        // has been established, or is an obligation some other program point
        // is required to discharge, or (#69) is backed by a real runtime
        // check regardless of what the static solver concluded. A
        // postcondition that is neither proven nor enforced -- `unchecked`,
        // or no `ensures` at all -- must not enter Γ: propagating it unearned
        // is how `needs_big(y)` came to be reported "proven at L2" from a
        // premise false for every input (#47).
        //
        // `None` is the pre-pass building the map: it propagates nothing, so
        // closure is computed without assuming any other function's claim.
        // `Some(ClosureKind::Open)` and an absent entry both mean "do not
        // propagate" -- an unresolved or genuinely open closure carries no
        // claim of any kind.
        let taint: Option<String> = match self.closed.and_then(|map| map.get(&callee)) {
            Some(ClosureKind::Proven) => None,
            Some(ClosureKind::Enforced) => Some(callee.clone()),
            _ => return,
        };

        // On an arity mismatch, propagate NOTHING (#50). `FnFacts::params`
        // skips receivers and pattern parameters, so a legal compiling function
        // reaches this branch -- and binding only `result` left the callee's
        // parameter names free to capture same-named variables in the
        // *caller's* scope. `ensures(result > n)` over a callee whose `n` was
        // unbound picked up the caller's own `n > 100`, proving a call whose
        // argument was negative.
        //
        // `obligations_for_call` already bails on the same condition; this path
        // is the one that did not.
        if facts.params.len() != call.args.len() {
            return;
        }

        let mut bindings: HashMap<String, Expr> = HashMap::new();
        bindings.extend(facts.params.iter().cloned().zip(call.args.iter().cloned()));
        bindings.insert("result".to_string(), ident_expr(&binding));

        for ensures in &facts.ensures {
            if let Predicate::Expr(expr) = substitute_exprs(ensures, &bindings) {
                self.gamma.push(expr);
                self.gamma_provenance.push(taint.clone());
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
        self.gamma_provenance.truncate(depth);
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
    ///
    /// Its parameters also shadow Γ for the body's duration (#50): a closure
    /// `|x: i64| …` rebinds `x`, so a fact about the enclosing `x` says nothing
    /// about the one in scope inside.
    fn visit_expr_closure(&mut self, node: &'ast syn::ExprClosure) {
        let saved = std::mem::replace(&mut self.returns_here, false);
        let bound: Vec<String> = node.inputs.iter().flat_map(pattern_idents).collect();
        self.with_shadowed(&bound, |this| this.visit_with_tail(&node.body, false));
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
    /// do not *narrow* Γ (see the module doc's scope note), so each arm is
    /// discharged against the enclosing Γ -- imprecise, never unsound, since
    /// a missing hypothesis can only fail to prove a goal.
    ///
    /// They do, however, **shadow** it (#50). `match o { Some(x) => … }` binds
    /// a new `x`, and a fact about the outer `x` is not a fact about it. The
    /// guard is shadowed too, since it can see the arm's bindings.
    fn visit_expr_match(&mut self, node: &'ast syn::ExprMatch) {
        let arms_are_tail = self.in_tail;
        self.visit_with_tail(&node.expr, false);
        for arm in &node.arms {
            let bound = pattern_idents(&arm.pat);
            self.with_shadowed(&bound, |this| {
                if let Some((_, guard)) = &arm.guard {
                    this.visit_with_tail(guard, false);
                }
                if arms_are_tail {
                    this.visit_tail_expr(&arm.body);
                } else {
                    this.visit_with_tail(&arm.body, false);
                }
            });
        }
    }

    /// `if let Some(x) = o { … }` and `while let` bind through a pattern in
    /// condition position. `syn` models that as an `Expr::Let`, which the
    /// default walk descends without noticing the binding -- so the enclosing
    /// `x`'s facts survived into a scope where `x` is a different value (#50).
    ///
    /// The bindings are scoped to the enclosing `if`/`while` body rather than
    /// to this node, so the shadowing is applied by
    /// [`Self::visit_expr_if`]/[`Self::visit_expr_while`]; this override only
    /// makes sure the scrutinee itself is still visited.
    fn visit_expr_let(&mut self, node: &'ast syn::ExprLet) {
        self.visit_with_tail(&node.expr, false);
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
    ///
    /// An `if let` condition binds through a pattern, and those bindings are in
    /// scope for the `then` arm only — so they shadow Γ there and not in the
    /// `else` arm (#50).
    fn visit_expr_if(&mut self, node: &'ast ExprIf) {
        let arms_are_tail = self.in_tail;
        self.visit_with_tail(&node.cond, false);
        let pattern_bound = condition_pattern_idents(&node.cond);

        let depth = self.gamma.len();
        // A branch-narrowing condition is a genuinely established fact, not
        // an enforced-not-proven premise -- `None` provenance (#69).
        self.gamma.push((*node.cond).clone());
        self.gamma_provenance.push(None);
        self.with_shadowed(&pattern_bound, |this| {
            this.visit_block_with_tail(&node.then_branch, arms_are_tail)
        });
        self.gamma.truncate(depth);
        self.gamma_provenance.truncate(depth);

        if let Some((_, else_branch)) = &node.else_branch {
            self.gamma.push(negate_condition(&node.cond));
            self.gamma_provenance.push(None);
            self.visit_with_tail(else_branch, arms_are_tail);
            self.gamma.truncate(depth);
            self.gamma_provenance.truncate(depth);
        }
    }

    /// A `while` body only runs when the condition holds — the same
    /// narrowing as an `if`. Its body is never in tail position: a `while`
    /// evaluates to `()`, so nothing in it becomes the return value except
    /// through an explicit `return`, which `visit_expr_return` catches on its
    /// own and with this same narrowed Γ.
    ///
    /// The body's own assignments are retired on entry (#50), because the walk
    /// is a single in-order pass: without that, `while c { need_pos(x); x = -1; }`
    /// proves `x > 0` from a fact false on every iteration but the first. A
    /// `while let` pattern shadows for the body's duration, as in `if let`.
    fn visit_expr_while(&mut self, node: &'ast ExprWhile) {
        self.visit_with_tail(&node.cond, false);
        let depth = self.gamma.len();
        self.gamma.push((*node.cond).clone());
        self.gamma_provenance.push(None);
        let mut shadowed = condition_pattern_idents(&node.cond);
        shadowed.extend(assigned_in_block(&node.body));
        self.with_shadowed(&shadowed, |this| {
            this.visit_block_with_tail(&node.body, false)
        });
        self.gamma.truncate(depth);
        self.gamma_provenance.truncate(depth);
    }

    /// A `loop` body carries no condition to narrow with, but the same
    /// single-pass problem as `while`: retire anything the body assigns before
    /// walking it (#50).
    fn visit_expr_loop(&mut self, node: &'ast syn::ExprLoop) {
        let shadowed = assigned_in_block(&node.body);
        self.with_shadowed(&shadowed, |this| {
            this.visit_block_with_tail(&node.body, false)
        });
    }

    /// `for pat in iter { … }` rebinds through `pat` on every iteration, and
    /// the body may assign as well (#50). Both shadow Γ for the body only; the
    /// iterator expression still sees the outer Γ, since it is evaluated before
    /// the binding takes effect.
    fn visit_expr_for_loop(&mut self, node: &'ast syn::ExprForLoop) {
        self.visit_with_tail(&node.expr, false);
        let mut shadowed = pattern_idents(&node.pat);
        shadowed.extend(assigned_in_block(&node.body));
        self.with_shadowed(&shadowed, |this| {
            this.visit_block_with_tail(&node.body, false)
        });
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
        let hypotheses = facts.hypotheses();
        let mut nested = CallSiteScan {
            caller: &caller,
            functions: self.functions,
            // Its own `requires` -- genuinely established, not enforced-not-
            // proven premises, so `None` provenance throughout (#69).
            gamma_provenance: vec![None; hypotheses.len()],
            gamma: hypotheses,
            locals: Vec::new(),
            ensures: &facts.ensures,
            unsigned_param_widths: Some(&facts.unsigned_param_widths),
            self_unchecked: facts.unchecked,
            in_tail: true,
            // A nested `fn` is its own return target, even when the `fn` sits
            // inside a closure body where the enclosing scan had it cleared.
            returns_here: true,
            closed: self.closed,
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

/// Names bound by an `if let`/`while let` pattern in condition position (#50).
///
/// `syn` models `if let Some(x) = o` as an `Expr::Let` in the condition, and a
/// `&&`-chain of them (let-chains) as nested `Binary`. Both shapes bind for the
/// body's duration, so both are collected.
fn condition_pattern_idents(cond: &Expr) -> Vec<String> {
    match strip_groups(cond) {
        Expr::Let(let_expr) => pattern_idents(&let_expr.pat),
        Expr::Binary(bin) if matches!(bin.op, syn::BinOp::And(_)) => {
            let mut names = condition_pattern_idents(&bin.left);
            names.extend(condition_pattern_idents(&bin.right));
            names
        }
        _ => Vec::new(),
    }
}

/// Every name assigned anywhere inside `block` — `x = …`, `x += …`, or a
/// `&mut x` borrow (#50).
///
/// A loop body is walked once, in order, so a mutation *after* a call never
/// retires the hypothesis that call used: `loop { need_pos(x); x = -1; }`
/// proved `x > 0` from a fact false on every iteration but the first.
///
/// Retiring these on entry to the body is the sound-and-cheap alternative to a
/// real fixpoint. It costs precision on a name assigned only *after* its last
/// use in the body, and it never admits a stale fact — the required direction
/// (ADR-0001 §5).
fn assigned_in_block(block: &Block) -> Vec<String> {
    struct Collect {
        names: Vec<String>,
    }
    impl<'ast> Visit<'ast> for Collect {
        fn visit_expr_assign(&mut self, node: &'ast syn::ExprAssign) {
            self.names.extend(assigned_idents(&node.left));
            visit::visit_expr_assign(self, node);
        }
        fn visit_expr_binary(&mut self, node: &'ast syn::ExprBinary) {
            if is_assign_op(&node.op) {
                self.names.extend(assigned_idents(&node.left));
            }
            visit::visit_expr_binary(self, node);
        }
        fn visit_expr_reference(&mut self, node: &'ast syn::ExprReference) {
            if node.mutability.is_some() {
                self.names.extend(assigned_idents(&node.expr));
            }
            visit::visit_expr_reference(self, node);
        }
    }
    let mut collect = Collect { names: Vec::new() };
    collect.visit_block(block);
    collect.names.sort();
    collect.names.dedup();
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
/// Which functions establish their own postconditions at every return point.
///
/// Γ may only assume a callee's `ensures` once the callee has been shown to
/// deliver it (ADR-0006 §5, spec 007 Requirement 2). Outcomes are not known
/// during the walk -- `discharge` runs later -- so they are computed here in a
/// pre-pass and consulted by [`CallSiteScan::propagate_postcondition`].
///
/// **Deliberately conservative: this pass propagates nothing.** A return site
/// that would only close using a fact propagated from elsewhere is therefore
/// not credited, and that function's own postcondition does not propagate in
/// turn. That under-credits rather than over-credits, which is the required
/// direction (ADR-0001 §5: imprecise is acceptable, unsound is not) -- and it
/// sidesteps a real circularity, since closure would otherwise depend on the
/// very map being built. A fixpoint iteration would recover the precision;
/// nothing needs it yet.
///
/// **Zero return-site obligations maps to `true`**, because `all()` over an
/// empty set is `true`. Two ways that arises, and only one is safe (#48):
///
/// - **No `ensures` at all** — vacuous and harmless. There is no postcondition
///   to propagate either way.
/// - **A diverging body** (`panic!`/`todo!`/`unimplemented!`/`unreachable!`)
///   produces no `result`, so no return point. The postcondition *does* then
///   propagate — verified — and that is sound only because the function never
///   returns, making the caller's continuation unreachable. Proving things
///   about unreachable code is vacuous, the same conclusion the solver reaches
///   for a contradictory Γ.
///
/// So the empty case is currently correct **for a specific reason**, not by
/// construction. Any future change that makes a function have zero return-site
/// obligations *while still returning* would silently mark it closed and
/// propagate an unestablished postcondition. Skipping unmodelled tail
/// expressions is exactly such a change — see [`CallSiteScan::visit_tail_expr`]
/// and the module doc. If a second non-divergent source ever appears, this
/// should become `!found.is_empty() && found.all(...)` and divergence handled
/// explicitly.
///
/// **Relaxed by #69**: a function that isn't fully `Proven`-closed may still
/// be [`ClosureKind::Enforced`]-closed — carrying `#[mvl::ensures]`, not
/// `#[mvl::unchecked]` — regardless of what any individual return site's
/// static discharge concluded. This is deliberately *not* conditioned on
/// each return site's own outcome: ADR-0006 §5's soundness argument for
/// enforcement is unconditional (an `assert!` at every return point means
/// "either the postcondition holds, or the process aborted", full stop),
/// so even a return site the solver reports `Runtime` or `Violated` for is
/// still safely covered by the same backstop as one it reports `Proven`
/// for. Only `#[mvl::unchecked]` — no runtime check at all — forfeits this.
fn return_site_closure_for(
    name: &str,
    block: &Block,
    functions: &HashMap<String, FnFacts>,
) -> ClosureKind {
    let facts = functions.get(name);
    let hypotheses = facts.map(FnFacts::hypotheses).unwrap_or_default();
    let mut found = Vec::new();
    let mut scan = CallSiteScan {
        caller: name,
        functions,
        gamma_provenance: vec![None; hypotheses.len()],
        gamma: hypotheses,
        locals: Vec::new(),
        ensures: facts.map(|f| f.ensures.as_slice()).unwrap_or(&[]),
        unsigned_param_widths: facts.map(|f| &f.unsigned_param_widths),
        self_unchecked: facts.is_some_and(|f| f.unchecked),
        in_tail: true,
        returns_here: true,
        closed: None,
        found: &mut found,
    };
    scan.visit_block(block);
    let all_proven = found
        .iter()
        .filter(|f| f.kind == ObligationKind::ReturnSite)
        .all(|f| matches!(f.discharge(), DischargeResult::Proven { .. }));
    if all_proven {
        ClosureKind::Proven
    } else if facts.is_some_and(FnFacts::ensures_enforced) {
        ClosureKind::Enforced
    } else {
        ClosureKind::Open
    }
}

fn return_site_closure(
    file: &syn::File,
    functions: &HashMap<String, FnFacts>,
) -> HashMap<String, ClosureKind> {
    let mut closed = HashMap::new();
    for item in flatten_items(file) {
        let Item::Fn(item_fn) = item else { continue };
        let name = item_fn.sig.ident.to_string();
        let kind = return_site_closure_for(&name, &item_fn.block, functions);
        closed.insert(name, kind);
    }
    for (name, method) in impl_methods(file) {
        let kind = return_site_closure_for(&name, &method.block, functions);
        closed.insert(name, kind);
    }
    closed
}

/// Assigns each obligation its occurrence index within its function (#51).
///
/// Done as a pass over the finished vector rather than at the three push
/// sites, for two reasons. Visit order is only definitively known once the
/// walk is over — the declaration finder and the call-site scan contribute
/// to the same vector in separate passes, so a counter held by either one
/// would number only its own half. And a single site cannot be got wrong in
/// three places.
///
/// Grouping is on the id stem, so the counters are per-`(function, stem)`:
/// a function's two `requires` clauses number 0 and 1 independently of its
/// two calls to the same callee, which also number 0 and 1.
fn number_occurrences(found: &mut [FoundObligation]) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for obligation in found.iter_mut() {
        let counter = seen.entry(obligation.id_stem()).or_insert(0);
        obligation.occurrence = *counter;
        *counter += 1;
    }
}

pub fn find_obligations(source: &str) -> Result<Vec<FoundObligation>, CheckError> {
    let file: syn::File = syn::parse_str(source).map_err(CheckError::Parse)?;

    let mut found = Vec::new();
    DeclarationFinder { found: &mut found }.visit_file(&file);
    // `DeclarationFinder`'s `Visit` walk overrides `visit_item_fn`, not
    // `visit_impl_item_fn` -- impl methods' own `requires`/`ensures` need a
    // separate pass (ADR-0001's largest practical coverage gap: methods
    // were invisible to every annotation-consuming check end to end).
    find_method_declarations(&file, &mut found);

    // Same collect-then-walk shape as `rust-effect`: every function's own
    // declared facts first, so a call can be resolved against a callee
    // defined later in the file.
    let mut functions: HashMap<String, FnFacts> = HashMap::new();
    for item in flatten_items(&file) {
        if let Item::Fn(item_fn) = item {
            functions.insert(item_fn.sig.ident.to_string(), FnFacts::of(item_fn));
        }
    }
    for (name, method) in impl_methods(&file) {
        functions.insert(name, FnFacts::of_method(method));
    }

    let closed = return_site_closure(&file, &functions);

    for item in flatten_items(&file) {
        if let Item::Fn(item_fn) = item {
            let caller = item_fn.sig.ident.to_string();
            scan_function_body(&caller, &item_fn.block, &functions, &closed, &mut found);
        }
    }
    for (name, method) in impl_methods(&file) {
        scan_function_body(&name, &method.block, &functions, &closed, &mut found);
    }

    number_occurrences(&mut found);
    Ok(found)
}

/// Runs [`CallSiteScan`] over one function-or-method body, starting in
/// tail position with its own `requires` as Γ and its own `ensures` as the
/// return-site goal. Shared by [`find_obligations`]'s free-function and
/// impl-method loops -- the scan itself doesn't care which kind of item
/// `name`/`block` came from.
fn scan_function_body(
    name: &str,
    block: &Block,
    functions: &HashMap<String, FnFacts>,
    closed: &HashMap<String, ClosureKind>,
    found: &mut Vec<FoundObligation>,
) {
    let facts = functions.get(name);
    let gamma = facts.map(FnFacts::hypotheses).unwrap_or_default();
    let ensures: &[Predicate] = facts.map(|f| f.ensures.as_slice()).unwrap_or(&[]);
    let mut scan = CallSiteScan {
        caller: name,
        functions,
        gamma_provenance: vec![None; gamma.len()],
        gamma,
        locals: Vec::new(),
        ensures,
        unsigned_param_widths: facts.map(|f| &f.unsigned_param_widths),
        self_unchecked: facts.is_some_and(|f| f.unchecked),
        // The body's own trailing expression is its return value, so the
        // walk starts in tail position -- and a `return` in it returns
        // from this function/method.
        in_tail: true,
        returns_here: true,
        closed: Some(closed),
        found,
    };
    scan.visit_block(block);
}

/// Renders one obligation's discharge outcome as a Gate-mode diagnostic.
/// Every obligation produces one, regardless of outcome, per spec
/// Requirement 3's "report which layer discharged it" UX -- `Proven`/
/// `Runtime` are informational (`Level::Note`, doesn't fail the build);
/// only `Violated` is `Level::Error`. `warrant` (#69) is what actually backs
/// a `Proven`/`Runtime` outcome for an entailment obligation -- see
/// [`Warrant`]'s own doc comment.
pub fn to_diagnostic(
    found: &FoundObligation,
    result: &DischargeResult,
    warrant: &Warrant,
) -> Diagnostic {
    match &found.kind {
        ObligationKind::CallSite { callee } => call_site_diagnostic(found, callee, result, warrant),
        ObligationKind::ReturnSite => return_site_diagnostic(found, result, warrant),
        ObligationKind::Requires | ObligationKind::Ensures => declaration_diagnostic(found, result),
    }
}

/// A return site's diagnostic quotes the *substituted* postcondition, so the
/// returned expression appears in place of `result` and the reader sees the
/// claim that actually failed. Mirrors [`call_site_diagnostic`], including
/// naming what was known at that point on a `Runtime` outcome, and (#69)
/// naming what's enforced rather than proven when `warrant` says so.
fn return_site_diagnostic(
    found: &FoundObligation,
    result: &DischargeResult,
    warrant: &Warrant,
) -> Diagnostic {
    let name = &found.fn_name;
    match (result, warrant) {
        (DischargeResult::Proven { layer }, Warrant::Enforcement { premises }) => Diagnostic::new(
            Level::Note,
            format!(
                "`{name}` postcondition `{}` is entailed at {} by this return, but the proof \
                 rests on {}'s runtime enforcement rather than a static guarantee alone",
                found.predicate_text(),
                layer_str(*layer),
                premises.join(", "),
            ),
            found.span,
        )
        .with_label("enforced, not proven"),
        (DischargeResult::Proven { layer }, _) => Diagnostic::new(
            Level::Note,
            format!(
                "`{name}` postcondition `{}` established at {} by this return",
                found.predicate_text(),
                layer_str(*layer)
            ),
            found.span,
        )
        .with_label("proven"),
        (DischargeResult::Runtime, Warrant::Enforcement { premises }) => Diagnostic::new(
            Level::Note,
            format!(
                "`{name}` postcondition `{}` is not established statically by this return, but \
                 {} enforces it at runtime -- treated as an enforced premise, not a proof, \
                 wherever it is relied upon",
                found.predicate_text(),
                premises.join(", "),
            ),
            found.span,
        )
        .with_label("enforced, not proven"),
        (DischargeResult::Runtime, _) => {
            let known = found.hypotheses_text();
            let known = if known.is_empty() {
                "nothing is known here".to_string()
            } else {
                format!("known here: {known}")
            };
            Diagnostic::new(
                Level::Note,
                format!(
                    "`{name}` postcondition `{}` is not established by this return \
                     ({known}) -- no runtime check is inserted, so it is unverified",
                    found.predicate_text()
                ),
                found.span,
            )
            .with_label("unverified")
        }
        (DischargeResult::Violated { counterexample }, _) => Diagnostic::new(
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
                "`{}` {} could not be discharged by any layer \
                 -- no runtime check is inserted, so it is unverified",
                found.fn_name,
                found.kind.as_str()
            ),
            found.span,
        )
        .with_label("unverified"),
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
/// failed. (#69) `warrant` names what's enforced rather than proven, when
/// it applies.
fn call_site_diagnostic(
    found: &FoundObligation,
    callee: &str,
    result: &DischargeResult,
    warrant: &Warrant,
) -> Diagnostic {
    let caller = &found.fn_name;
    match (result, warrant) {
        (DischargeResult::Proven { layer }, Warrant::Enforcement { premises }) => Diagnostic::new(
            Level::Note,
            format!(
                "`{callee}` precondition `{}` is entailed at {} for this call, but the proof \
                 rests on {}'s runtime enforcement rather than a static guarantee alone",
                found.predicate_text(),
                layer_str(*layer),
                premises.join(", "),
            ),
            found.span,
        )
        .with_label("enforced, not proven"),
        (DischargeResult::Proven { layer }, _) => Diagnostic::new(
            Level::Note,
            format!(
                "`{callee}` precondition `{}` proven at {} for this call",
                found.predicate_text(),
                layer_str(*layer)
            ),
            found.span,
        )
        .with_label("proven"),
        (DischargeResult::Runtime, Warrant::Enforcement { premises }) => Diagnostic::new(
            Level::Note,
            format!(
                "`{callee}` precondition `{}` is not entailed in `{caller}`, but {} enforces it \
                 at runtime -- treated as an enforced premise, not a proof, wherever it is \
                 relied upon",
                found.predicate_text(),
                premises.join(", "),
            ),
            found.span,
        )
        .with_label("enforced, not proven"),
        (DischargeResult::Runtime, _) => {
            let known = found.hypotheses_text();
            let known = if known.is_empty() {
                "nothing is known about the arguments here".to_string()
            } else {
                format!("known here: {known}")
            };
            Diagnostic::new(
                Level::Note,
                format!(
                    "`{callee}` precondition `{}` is not entailed in `{caller}` \
                     ({known}) -- no runtime check is inserted, so it is unverified",
                    found.predicate_text()
                ),
                found.span,
            )
            .with_label("unverified")
        }
        (DischargeResult::Violated { counterexample }, _) => Diagnostic::new(
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
        .map(|f| to_diagnostic(f, &f.discharge(), &f.warrant()))
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
    fn a_free_function_nested_in_a_mod_gets_its_return_site_obligation_too() {
        // #115: previously only the declaration-site obligations (via
        // DeclarationFinder's Visit walk, which already recurses through
        // modules by default) were found through a `mod` -- the
        // return-site obligation, generated by a flat `for item in
        // &file.items` loop, was silently dropped.
        let found = find_obligations(
            "mod foo {\n\
                 #[mvl::ensures(0 <= result && result <= 15)]\n\
                 fn mask_low_nibble(b: i32) -> i32 { b & 15 }\n\
             }",
        )
        .unwrap();

        assert_eq!(found.len(), 2, "found: {found:?}");
        assert_eq!(found[0].kind, ObligationKind::Ensures);
        assert_eq!(found[1].kind, ObligationKind::ReturnSite);
        assert_eq!(found[1].fn_name, "mask_low_nibble");
    }

    #[test]
    fn an_impl_method_nested_in_a_mod_is_found_at_all() {
        // #115: an `impl` block inside a `mod` was previously invisible
        // entirely -- not even its declaration-site obligation.
        let found = find_obligations(
            "mod foo {\n\
                 struct T;\n\
                 impl T {\n\
                     #[mvl::ensures(result > 0)]\n\
                     fn f(x: i32) -> i32 { x }\n\
                 }\n\
             }",
        )
        .unwrap();

        assert_eq!(found.len(), 2, "found: {found:?}");
        assert_eq!(found[0].kind, ObligationKind::Ensures);
        assert_eq!(found[0].fn_name, "T::f");
        assert_eq!(found[1].kind, ObligationKind::ReturnSite);
        assert_eq!(found[1].fn_name, "T::f");
    }

    #[test]
    fn finds_requires_and_ensures_on_an_impl_method() {
        let found = find_obligations(
            "struct DatabaseHeader;\n\
             impl DatabaseHeader {\n\
                 #[mvl::requires(0 <= b && b <= 255)]\n\
                 #[mvl::ensures(0 <= result && result <= 15)]\n\
                 fn mask_low_nibble(b: i32) -> i32 { b & 15 }\n\
             }",
        )
        .unwrap();

        // Same three obligations as the free-function case, qualified by
        // the impl's Self type -- ADR-0001's "methods are largely invisible"
        // gap, closed for declaration-site and return-site checking.
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].kind, ObligationKind::Requires);
        assert_eq!(found[0].fn_name, "DatabaseHeader::mask_low_nibble");
        assert_eq!(found[1].kind, ObligationKind::Ensures);
        assert_eq!(found[2].kind, ObligationKind::ReturnSite);
        assert_eq!(found[2].fn_name, "DatabaseHeader::mask_low_nibble");
    }

    #[test]
    fn a_method_and_a_free_function_with_the_same_name_are_independent() {
        let found = find_obligations(
            "struct T;\n\
             impl T {\n\
                 #[mvl::ensures(result > 0)]\n\
                 fn f(x: i32) -> i32 { x }\n\
             }\n\
             #[mvl::ensures(result < 0)]\n\
             fn f(x: i32) -> i32 { x }",
        )
        .unwrap();

        let method_ensures = found
            .iter()
            .find(|o| o.fn_name == "T::f" && o.kind == ObligationKind::Ensures)
            .expect("method's own ensures, not the free function's");
        let free_ensures = found
            .iter()
            .find(|o| o.fn_name == "f" && o.kind == ObligationKind::Ensures)
            .expect("free function's own ensures, not the method's");
        assert_ne!(
            format!("{:?}", method_ensures.predicate),
            format!("{:?}", free_ensures.predicate)
        );
    }

    #[test]
    fn method_with_no_refinement_attrs_finds_nothing() {
        let found =
            find_obligations("struct T;\nimpl T {\n    fn f(x: i32) -> i32 { x }\n}").unwrap();
        assert!(found.is_empty());
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
        // Assert on "unverified", not on the substring "runtime check": the
        // reworded diagnostic (#47) says "no runtime check is inserted", so a
        // `contains("runtime check")` check would keep passing while asserting
        // the opposite of what it reads as.
        assert!(
            diagnostics[0].message.contains("unverified"),
            "expected an unverified-obligation note, got: {}",
            diagnostics[0].message
        );
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
