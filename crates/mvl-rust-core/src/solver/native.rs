//! The native `L1` (trivial) + `L2` (integer interval arithmetic) + `L3`
//! (bounded quantifier expansion) backend (ADR-0005 v0.1 scope + #31's
//! quantifier extension).
//!
//! Two questions, two entry points:
//!
//! - [`discharge_predicate`] — **coherence**, at a declaration site. Is
//!   the predicate internally consistent: a tautology, or a conjunction of
//!   integer bounds whose intersection is non-empty, as opposed to
//!   self-contradictory (`x >= 10 && x < 5`, which no value of `x` can ever
//!   satisfy)? A declaration site has no arguments to reason about, so this
//!   is the only question available there. It covers all three of spec
//!   Requirement 3's original acceptance scenarios: a satisfiable interval
//!   bound discharges at `L2`, a self-contradictory one is a genuine
//!   violation, and anything this analysis can't decompose (function calls,
//!   disjunction, `!=`, non-integer types) falls through to a runtime check
//!   rather than blocking the build.
//! - [`discharge_entailment`] — **entailment**, at a call site (#38). Does
//!   the caller's hypothesis context Γ entail the callee's precondition
//!   with the actual arguments substituted in? This is the question real
//!   MVL's own solver asks, and needs the call graph `rust-refine` now
//!   builds; see that function's own section for the framing.
//!
//! Because tool crates parse plain source text with no type information,
//! call resolution is same-file only and free-functions only — the same
//! boundary `rust-effect` documents for the same reason.
//!
//! `L3` (bounded quantifiers, `forall`/`exists i in [lo..hi]. body`) is
//! discharged by expansion, matching `mvl-lang/mvl`'s own real, accepted
//! design (ADR-0056, confirmed against its actual solver source and test
//! fixtures — see `crate::attrs::Predicate`'s doc comment): substitute the
//! bound variable with each concrete integer in range, dispatch each
//! instance recursively through this same backend, and aggregate. Every
//! expanded instance is attributed to `Layer::L3` regardless of which
//! inner layer (`L1` or `L2`) actually discharged it — the expansion
//! itself *is* the `L3` activity, matching ADR-0056's own framing.
//!
//! `L4` (linear arithmetic beyond plain interval containment, e.g.
//! `a > c && b > 0 && a + b <= c`) is Fourier-Motzkin elimination plus a
//! divisibility check for single-variable equalities -- ported from real
//! MVL's actual `src/mvl/checker/solver/layer4.rs` (#35), not the fuller
//! "Cooper's algorithm" its own doc comments (inaccurately) call it. See
//! the `L4` section below for the adaptation to this backend's
//! satisfiability framing.

use std::collections::{HashMap, HashSet};

use syn::visit_mut::{self, VisitMut};
use syn::{BinOp, Expr, ExprLit, ExprUnary, Ident, Lit, UnOp};

use crate::attrs::Predicate;

use super::{smt, DischargeResult, Layer, Obligation, SolverBackend};

/// A range wider than this many elements (`hi - lo + 1`) isn't expanded
/// and falls straight to `Runtime` instead — same constant and rationale
/// as `mvl-lang/mvl`'s own `MAX_BOUNDED_EXPANSION` (`src/mvl/checker/
/// refinements.rs`): prevents pathological blow-up on wide ranges, no
/// L5/SMT involvement (quantifiers are L3-only in the real design, per
/// ADR-0056).
const MAX_BOUNDED_EXPANSION: i64 = 1000;

/// Total instances a predicate expands to — the product of its quantifier
/// widths, `1` for a quantifier-free one, `None` on overflow.
///
/// The per-quantifier check inside [`quantify_forall`] is not enough on its
/// own: nesting two legal 1000-wide ranges passes it twice and still expands
/// to a million instances. Checking the product once, up front, is the bound
/// that constant was always meant to express.
fn expansion_size(pred: &Predicate) -> Option<i64> {
    match pred {
        Predicate::Expr(_) => Some(1),
        Predicate::Forall { lo, hi, body, .. } | Predicate::Exists { lo, hi, body, .. } => {
            let width = hi.checked_sub(*lo)?.checked_add(1)?.max(0);
            width.checked_mul(expansion_size(body)?)
        }
    }
}

/// Whether expanding `pred` is affordable. An empty range (`width` 0) is
/// decided without expanding anything, so it is always affordable.
fn expansion_is_affordable(pred: &Predicate) -> bool {
    match expansion_size(pred) {
        Some(size) => size <= MAX_BOUNDED_EXPANSION,
        None => false,
    }
}

/// Native `L1`+`L2`+`L3` obligation dispatcher. Holds no state — every
/// obligation is analyzed independently from its own predicate text.
#[derive(Debug, Clone, Copy, Default)]
pub struct NativeBackend;

impl SolverBackend for NativeBackend {
    fn discharge(&self, obligation: &Obligation) -> DischargeResult {
        match syn::parse_str::<Predicate>(&obligation.predicate) {
            Ok(pred) => discharge_predicate(&pred),
            // The predicate string came from a real `Predicate` in the
            // first place (see `rust-refine`'s obligation extraction), so
            // this only fires if that invariant is ever broken elsewhere.
            Err(_) => DischargeResult::Runtime,
        }
    }
}

/// Discharges a predicate — a plain expression via `L1`/`L2`, or a
/// bounded quantifier via `L3` expansion (recursing back into this same
/// function for each expanded instance, so nested quantifiers and a
/// quantifier composed with ordinary comparisons both work without any
/// special-casing).
///
/// This is the **declaration-site** question: is the predicate internally
/// coherent? For the **call-site** question (does a caller's hypothesis
/// context entail a callee's precondition), see
/// [`discharge_entailment`] — a different question, deliberately kept as
/// a separate entry point rather than folded in here (#38).
pub fn discharge_predicate(pred: &Predicate) -> DischargeResult {
    if !expansion_is_affordable(pred) {
        return DischargeResult::Runtime;
    }
    match pred {
        Predicate::Expr(expr) => discharge_expr(expr),
        Predicate::Forall { var, lo, hi, body } => {
            quantify_forall(var, *lo, *hi, body, &discharge_predicate)
        }
        Predicate::Exists { var, lo, hi, body } => {
            quantify_exists(var, *lo, *hi, body, &discharge_predicate)
        }
    }
}

/// `forall x in [lo..hi]. body`: any expanded instance `Violated` ⇒ the
/// whole quantifier `Violated` (short-circuit, counterexample names the
/// failing `x`); all instances `Proven` ⇒ `Proven{L3}`; otherwise (some
/// `Runtime`, none `Violated`) ⇒ `Runtime`. `hi < lo` (an empty range) is
/// vacuously true.
///
/// `dispatch` decides each expanded instance, so the same expansion drives
/// both the coherence question ([`discharge_predicate`]) and the
/// entailment one ([`discharge_entailment`], which closes over Γ) — the
/// `L3` expansion itself is identical either way.
fn quantify_forall(
    var: &Ident,
    lo: i64,
    hi: i64,
    body: &Predicate,
    dispatch: &dyn Fn(&Predicate) -> DischargeResult,
) -> DischargeResult {
    if hi < lo {
        return DischargeResult::Proven { layer: Layer::L3 };
    }
    if hi - lo + 1 > MAX_BOUNDED_EXPANSION {
        return DischargeResult::Runtime;
    }

    let mut any_runtime = false;
    for k in lo..=hi {
        match dispatch(&substitute(body, var, k)) {
            DischargeResult::Violated { counterexample } => {
                return DischargeResult::Violated {
                    counterexample: format!(
                        "forall {var} in [{lo}..{hi}]: fails at {var} = {k}: {counterexample}"
                    ),
                };
            }
            DischargeResult::Runtime => any_runtime = true,
            DischargeResult::Proven { .. } => {}
        }
    }

    if any_runtime {
        DischargeResult::Runtime
    } else {
        DischargeResult::Proven { layer: Layer::L3 }
    }
}

/// `exists x in [lo..hi]. body`: dual of `forall` — any instance `Proven`
/// ⇒ `Proven{L3}` (short-circuit); all instances `Violated` (no witness
/// found) ⇒ `Violated`; otherwise ⇒ `Runtime`. `hi < lo` (an empty range)
/// has no possible witness, so it's `Violated`. `dispatch` as in
/// [`quantify_forall`].
fn quantify_exists(
    var: &Ident,
    lo: i64,
    hi: i64,
    body: &Predicate,
    dispatch: &dyn Fn(&Predicate) -> DischargeResult,
) -> DischargeResult {
    if hi < lo {
        return DischargeResult::Violated {
            counterexample: format!("the range [{lo}..{hi}] for `{var}` is empty; no witness"),
        };
    }
    if hi - lo + 1 > MAX_BOUNDED_EXPANSION {
        return DischargeResult::Runtime;
    }

    let mut any_runtime = false;
    for k in lo..=hi {
        match dispatch(&substitute(body, var, k)) {
            DischargeResult::Proven { .. } => {
                return DischargeResult::Proven { layer: Layer::L3 };
            }
            DischargeResult::Runtime => any_runtime = true,
            DischargeResult::Violated { .. } => {}
        }
    }

    if any_runtime {
        DischargeResult::Runtime
    } else {
        DischargeResult::Violated {
            counterexample: format!(
                "no value of `{var}` in [{lo}..{hi}] satisfies the existential"
            ),
        }
    }
}

/// Substitutes every occurrence of `var` with the integer literal `value`
/// in a cloned copy of `pred`. A nested quantifier that reuses `var` as
/// its own bound variable shadows the outer one — its body is left
/// untouched, matching ordinary lexical scoping.
fn substitute(pred: &Predicate, var: &Ident, value: i64) -> Predicate {
    match pred {
        Predicate::Expr(expr) => {
            let mut cloned = expr.clone();
            SubstituteVar { var, value }.visit_expr_mut(&mut cloned);
            Predicate::Expr(cloned)
        }
        Predicate::Forall {
            var: bound,
            lo,
            hi,
            body,
        } => {
            if bound == var {
                pred.clone()
            } else {
                Predicate::Forall {
                    var: bound.clone(),
                    lo: *lo,
                    hi: *hi,
                    body: Box::new(substitute(body, var, value)),
                }
            }
        }
        Predicate::Exists {
            var: bound,
            lo,
            hi,
            body,
        } => {
            if bound == var {
                pred.clone()
            } else {
                Predicate::Exists {
                    var: bound.clone(),
                    lo: *lo,
                    hi: *hi,
                    body: Box::new(substitute(body, var, value)),
                }
            }
        }
    }
}

struct SubstituteVar<'a> {
    var: &'a Ident,
    value: i64,
}

impl VisitMut for SubstituteVar<'_> {
    fn visit_expr_mut(&mut self, expr: &mut Expr) {
        let is_target = matches!(
            expr,
            Expr::Path(path) if path.qself.is_none()
                && path.path.get_ident().is_some_and(|ident| ident == self.var)
        );
        if is_target {
            *expr = int_literal_expr(self.value);
            return;
        }
        visit_mut::visit_expr_mut(self, expr);
    }
}

fn int_literal_expr(value: i64) -> Expr {
    if value < 0 {
        Expr::Unary(ExprUnary {
            attrs: vec![],
            op: UnOp::Neg(Default::default()),
            expr: Box::new(int_literal_expr(-value)),
        })
    } else {
        Expr::Lit(ExprLit {
            attrs: vec![],
            lit: Lit::Int(syn::LitInt::new(
                &value.to_string(),
                proc_macro2::Span::call_site(),
            )),
        })
    }
}

/// Closed interval `[lo, hi]` over `i128` (wide enough for any Rust
/// integer literal). `lo > hi` represents an empty (unsatisfiable)
/// interval — the signal for a genuine contradiction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Interval {
    lo: i128,
    hi: i128,
}

impl Interval {
    fn point(v: i128) -> Self {
        Interval { lo: v, hi: v }
    }

    fn at_least(v: i128) -> Self {
        Interval {
            lo: v,
            hi: i128::MAX,
        }
    }

    fn at_most(v: i128) -> Self {
        Interval {
            lo: i128::MIN,
            hi: v,
        }
    }

    fn intersect(self, other: Interval) -> Interval {
        Interval {
            lo: self.lo.max(other.lo),
            hi: self.hi.min(other.hi),
        }
    }

    fn is_empty(self) -> bool {
        self.lo > self.hi
    }

    /// Whether every value in `other` is also in `self` (`other ⊆ self`) —
    /// the entailment test at `L2`: a goal bound is entailed when what the
    /// hypotheses already establish for that variable fits inside it.
    fn contains(self, other: Interval) -> bool {
        self.lo <= other.lo && other.hi <= self.hi
    }
}

/// What one AND-clause of the predicate reduced to.
enum Clause {
    /// A constant-foldable leaf (no free variables): `true`, `5 >= 0`, ...
    Constant(bool),
    /// `var` must lie in `interval` for this clause alone to hold.
    Bound { var: String, interval: Interval },
    /// Not decomposable by this backend (quantifiers, calls, `||`, `!=`,
    /// non-literal comparisons, ...).
    Unknown,
}

/// Discharges a plain `L1`/`L2` expression (no quantifiers).
fn discharge_expr(expr: &Expr) -> DischargeResult {
    let mut clauses = Vec::new();
    flatten_and(expr, &mut clauses);

    let mut bounds: HashMap<String, Interval> = HashMap::new();
    let mut saw_bound = false;
    let mut undecidable = false;

    for clause_expr in &clauses {
        match classify_clause(clause_expr) {
            Clause::Constant(false) => {
                return DischargeResult::Violated {
                    counterexample: format!("`{}` is always false", quote::quote!(#clause_expr)),
                };
            }
            Clause::Constant(true) => {}
            Clause::Bound { var, interval } => {
                saw_bound = true;
                bounds
                    .entry(var)
                    .and_modify(|existing| *existing = existing.intersect(interval))
                    .or_insert(interval);
            }
            Clause::Unknown => undecidable = true,
        }
    }

    for (var, interval) in &bounds {
        if interval.is_empty() {
            return DischargeResult::Violated {
                counterexample: format!(
                    "no value of `{var}` satisfies the combined bounds ({}..={} is empty)",
                    interval.lo, interval.hi
                ),
            };
        }
    }

    if !undecidable {
        return DischargeResult::Proven {
            layer: if saw_bound { Layer::L2 } else { Layer::L1 },
        };
    }

    // L1/L2 couldn't fully decide every clause (e.g. a cross-variable
    // relation like `a + b <= c`, which isn't a `var OP literal` bound) --
    // try L4 before giving up.
    match discharge_l4(&clauses) {
        Some(result) => result,
        None => DischargeResult::Runtime,
    }
}

// ── Entailment: `Γ ⊢ goal`, the call-site framing (#38) ───────────────────
//
// Everything above answers "is this predicate internally coherent?" — the
// right question at a *declaration* site, where there are no arguments to
// reason about. At a *call* site the question is real MVL's own: does the
// caller's hypothesis context Γ entail the callee's precondition, with the
// actual arguments substituted in? (`try_z3(pred, arg, var_refs, _)` in
// `mvl-lang/mvl`'s `src/mvl/checker/solver/layer5.rs` takes exactly those
// three things; its query is `Γ ∧ ¬pred(arg)`, and `unsat` ⇒ proven.)
//
// Both questions are wanted, at their own program points, so this is a
// separate entry point rather than a change to `discharge_predicate`. The
// one thing the two share is `classify_clause`, so a rule added there
// reaches both — #43's reflexivity is the first that does, which is why
// `a != a` is a declaration-site violation now too.
//
// Three outcomes, mirroring real MVL's own literal-vs-symbolic split in
// `impl_z3`:
//
// | Query                  | Meaning                          | Result     |
// |------------------------|----------------------------------|------------|
// | `Γ ∧ ¬goal` UNSAT      | goal holds for every value Γ allows | `Proven` |
// | `Γ ∧ goal` UNSAT       | goal fails for every value Γ allows | `Violated` |
// | neither                | may hold, may not               | `Runtime`  |
//
// Negation stays inside the `Le`/`Eq` constraint fragment: `¬(c₁ ∧ … ∧ cₙ)`
// is checked one disjunct at a time, and over the integers `¬(t ≤ 0)` is
// `t ≥ 1`. An equality clause's negation is itself a disjunction
// (`¬(t = 0)` is `t ≤ -1 ∨ t ≥ 1`), handled the same way one level down —
// both halves must be independently unsat (#43). No disjunction is ever
// represented inside the solver either time; it becomes one query per
// disjunct.
//
// Hypotheses this backend can't decompose are **dropped**, not bailed on.
// That's sound in both directions: fewer facts make `Γ ∧ ¬goal` easier to
// satisfy (harder to prove) and `Γ ∧ goal` easier to satisfy (harder to
// call violated). Goal clauses are never dropped — every one must be
// decided for `Proven`.

/// Discharges `hypotheses ⊢ goal`: whether the goal predicate follows from
/// everything the hypothesis context establishes. `hypotheses` are
/// `&&`-flattened independently, so a caller can pass its own `requires`
/// clauses, narrowed branch conditions, and propagated postconditions as
/// separate expressions.
pub fn discharge_entailment(hypotheses: &[Expr], goal: &Predicate) -> DischargeResult {
    if !expansion_is_affordable(goal) {
        return DischargeResult::Runtime;
    }
    match goal {
        Predicate::Expr(expr) => entail_expr(hypotheses, expr),
        Predicate::Forall { var, lo, hi, body } => {
            quantify_forall(var, *lo, *hi, body, &|instance| {
                discharge_entailment(hypotheses, instance)
            })
        }
        Predicate::Exists { var, lo, hi, body } => {
            quantify_exists(var, *lo, *hi, body, &|instance| {
                discharge_entailment(hypotheses, instance)
            })
        }
    }
}

fn entail_expr(hypotheses: &[Expr], goal: &Expr) -> DischargeResult {
    let mut hyp_clauses: Vec<&Expr> = Vec::new();
    for hypothesis in hypotheses {
        flatten_and(hypothesis, &mut hyp_clauses);
    }
    let mut goal_clauses: Vec<&Expr> = Vec::new();
    flatten_and(goal, &mut goal_clauses);

    // What Γ establishes as a per-variable interval (L2's view of it).
    let mut hyp_bounds: HashMap<String, Interval> = HashMap::new();
    for clause in &hyp_clauses {
        if let Clause::Bound { var, interval } = classify_clause(clause) {
            hyp_bounds
                .entry(var)
                .and_modify(|existing| *existing = existing.intersect(interval))
                .or_insert(interval);
        }
    }

    // Contradictory Γ means this program point is unreachable, so anything
    // is entailed here — `Γ ∧ ¬goal` is unsat for want of a satisfiable Γ.
    // Real MVL reaches the same conclusion the same way (its Z3 query goes
    // unsat on the hypotheses alone); reporting it as proven rather than as
    // its own outcome keeps this backend's three-way result shape.
    if hyp_bounds.values().any(|interval| interval.is_empty()) {
        return DischargeResult::Proven { layer: Layer::L2 };
    }

    let hyp_constraints = system_constraints(&hyp_clauses);

    // L1/L2: a goal clause is entailed when it's a tautology, or when what
    // Γ knows about its variable fits inside the interval it demands.
    let mut unresolved: Vec<&Expr> = Vec::new();
    for clause in &goal_clauses {
        match classify_clause(clause) {
            Clause::Constant(true) => {}
            Clause::Constant(false) => {
                // A clause false on its face still doesn't make the call a
                // violation if Γ permits no values at all -- an unreachable
                // program point entails anything, this clause included. The
                // interval check above only catches Γ contradicting itself
                // one variable at a time, so ask the linear system too
                // before erroring. Same reasoning as the check below the L4
                // attempt, which this return would otherwise pre-empt.
                if matches!(
                    check_satisfiability(hyp_constraints.clone()),
                    SatOutcome::Contradiction
                ) {
                    return DischargeResult::Proven { layer: Layer::L4 };
                }
                return DischargeResult::Violated {
                    counterexample: format!("`{}` is always false", quote::quote!(#clause)),
                };
            }
            Clause::Bound { var, interval } => match hyp_bounds.get(&var) {
                Some(known) if interval.contains(*known) => {}
                _ => unresolved.push(clause),
            },
            Clause::Unknown => unresolved.push(clause),
        }
    }

    if unresolved.is_empty() {
        return DischargeResult::Proven {
            layer: if hyp_bounds.is_empty() {
                Layer::L1
            } else {
                Layer::L2
            },
        };
    }

    // L4: `Γ ∧ ¬clause` must be UNSAT for every clause L1/L2 left open.
    let all_entailed = unresolved
        .iter()
        .all(|clause| refutes_negation(&hyp_constraints, clause));
    if all_entailed {
        return DischargeResult::Proven { layer: Layer::L4 };
    }

    // L5 (#37): whatever L4 couldn't refute, retried through Z3. Evaluated
    // without short-circuiting (`all_entailed`'s `.all()` above stops at the
    // first `false`, which is fine for a bare bool but not for deciding
    // exactly what still needs trying) and handed the *raw* hypotheses --
    // not `hyp_constraints`, which already dropped anything outside the
    // linear fragment, and the whole reason L5 exists is the fragment L4
    // cannot represent (genuine nonlinearity) or gave up on (its own
    // complexity guards). A no-op returning `false` when the `z3` feature
    // is off, so this changes nothing about the default build or its
    // outcomes.
    let still_unresolved: Vec<&Expr> = unresolved
        .iter()
        .copied()
        .filter(|clause| !refutes_negation(&hyp_constraints, clause))
        .collect();
    if smt::try_entail_all(&hyp_clauses, &still_unresolved) {
        return DischargeResult::Proven { layer: Layer::L5 };
    }

    // The interval check above only sees Γ contradict itself one variable
    // at a time; `x + y <= 0 ∧ x >= 5 ∧ y >= 5` needs the linear system to
    // show it. Same conclusion either way — an unreachable program point
    // entails anything — but it has to be reached before the test below,
    // which would otherwise read "no value Γ permits satisfies the goal"
    // off a Γ that permits no values at all and call it a violation.
    if matches!(
        check_satisfiability(hyp_constraints.clone()),
        SatOutcome::Contradiction
    ) {
        return DischargeResult::Proven { layer: Layer::L4 };
    }

    // Not provable — but is it definitely *false*? `Γ ∧ goal` UNSAT means
    // no value Γ permits can satisfy the goal, so the call can never be
    // valid: a compile-time error rather than a runtime check.
    let mut system = hyp_constraints;
    let mut goal_is_linear = true;
    for clause in &goal_clauses {
        match constraints_from_clause(clause) {
            // Without the expansion Fourier-Motzkin drops goal equalities,
            // and `x >= 5 ∧ x == 3` is never seen as the contradiction it is.
            Some(constraints) => system.extend(with_equality_bounds(constraints)),
            None => {
                goal_is_linear = false;
                break;
            }
        }
    }
    if goal_is_linear && matches!(check_satisfiability(system), SatOutcome::Contradiction) {
        return DischargeResult::Violated {
            counterexample: format!(
                "no value satisfying the hypotheses can satisfy `{}`",
                quote::quote!(#goal)
            ),
        };
    }

    DischargeResult::Runtime
}

/// Constraints for a hypothesis system: clauses outside the linear
/// fragment are skipped rather than failing the whole conversion (see this
/// section's own note on why dropping hypotheses is sound in both
/// directions).
fn system_constraints(clauses: &[&Expr]) -> Vec<Constraint> {
    clauses
        .iter()
        .filter_map(|clause| constraints_from_clause(clause))
        .flat_map(with_equality_bounds)
        .collect()
}

/// Each `Eq(t)` accompanied by the two inequalities it implies (`t ≤ 0`
/// and `-t ≤ 0`), so Fourier-Motzkin — which only consumes `Le` — can use
/// it. The equality itself is kept for the divisibility check.
///
/// Real MVL's `is_unsat` drops equalities from that phase entirely — a
/// limitation worth inheriting where this backend ports its algorithm
/// verbatim ([`discharge_l4`]), but not on either side of an entailment
/// query, where `x == 5` is exactly the kind of fact that needs to travel.
fn with_equality_bounds(constraints: Vec<Constraint>) -> Vec<Constraint> {
    let mut out = Vec::with_capacity(constraints.len());
    for constraint in constraints {
        if let Constraint::Eq(term) = &constraint {
            out.push(Constraint::Le(term.clone()));
            out.push(Constraint::Le(term.negate()));
        }
        out.push(constraint);
    }
    out
}

/// Whether `hypotheses ∧ ¬clause` is unsatisfiable, i.e. whether Γ entails
/// `clause`. `false` when the clause is outside the linear fragment, which
/// is treated as not entailed rather than guessed at.
///
/// An equality has no single-inequality negation — `¬(t = 0)` is
/// `t ≤ -1 ∨ t ≥ 1` — but proving one never requires negating it as a
/// whole. `t = 0` holds exactly when `t ≤ 0` and `t ≥ 0` both do, so it
/// splits into two inequality questions this backend already answers, and
/// *both* must come back unsat. The dual of the conjunctive-goal
/// decomposition, one level down: that one proves every conjunct, this one
/// refutes every disjunct (#43).
fn refutes_negation(hypotheses: &[Constraint], clause: &Expr) -> bool {
    let unsat_with = |negated: LinTerm| {
        let mut system = hypotheses.to_vec();
        system.push(Constraint::Le(negated));
        matches!(check_satisfiability(system), SatOutcome::Contradiction)
    };
    match constraints_from_clause(clause).as_deref() {
        // ¬(t ≤ 0) is t ≥ 1, i.e. `-t + 1 ≤ 0`.
        Some([Constraint::Le(term)]) => unsat_with(term.negate().plus_one()),
        // t = 0 ⟺ t ≤ 0 ∧ t ≥ 0; refute each half on its own.
        Some([Constraint::Eq(term)]) => {
            unsat_with(term.negate().plus_one()) && unsat_with(term.plus_one())
        }
        _ => false,
    }
}

/// Substitutes each named variable with an arbitrary expression — the
/// call-site substitution step (`pred[params := args]`). A quantifier that
/// binds one of the names shadows it, as in [`substitute`].
pub fn substitute_exprs(pred: &Predicate, bindings: &HashMap<String, Expr>) -> Predicate {
    match pred {
        Predicate::Expr(expr) => {
            let mut cloned = expr.clone();
            SubstituteExprs { bindings }.visit_expr_mut(&mut cloned);
            Predicate::Expr(cloned)
        }
        Predicate::Forall { var, lo, hi, body } => Predicate::Forall {
            var: var.clone(),
            lo: *lo,
            hi: *hi,
            body: Box::new(substitute_exprs(body, &without(bindings, var))),
        },
        Predicate::Exists { var, lo, hi, body } => Predicate::Exists {
            var: var.clone(),
            lo: *lo,
            hi: *hi,
            body: Box::new(substitute_exprs(body, &without(bindings, var))),
        },
    }
}

/// `bindings` minus any entry the quantifier's own bound variable shadows.
fn without(bindings: &HashMap<String, Expr>, bound: &Ident) -> HashMap<String, Expr> {
    let bound = bound.to_string();
    bindings
        .iter()
        .filter(|(name, _)| **name != bound)
        .map(|(name, expr)| (name.clone(), expr.clone()))
        .collect()
}

struct SubstituteExprs<'a> {
    bindings: &'a HashMap<String, Expr>,
}

impl VisitMut for SubstituteExprs<'_> {
    fn visit_expr_mut(&mut self, expr: &mut Expr) {
        let replacement = match &expr {
            Expr::Path(path) if path.qself.is_none() => path
                .path
                .get_ident()
                .and_then(|ident| self.bindings.get(&ident.to_string()))
                .cloned(),
            _ => None,
        };
        if let Some(replacement) = replacement {
            // Parenthesized so a compound argument (`n - 1`, `a + b`)
            // keeps its own precedence once spliced into the predicate.
            *expr = Expr::Paren(syn::ExprParen {
                attrs: vec![],
                paren_token: Default::default(),
                expr: Box::new(replacement),
            });
            return;
        }
        visit_mut::visit_expr_mut(self, expr);
    }
}

// ── L4: linear arithmetic via Fourier-Motzkin elimination (#35) ────────────
//
// Ported from `mvl-lang/mvl`'s real `src/mvl/checker/solver/layer4.rs` --
// despite the module's own name there ("Cooper's algorithm"), the actual
// technique is Fourier-Motzkin elimination plus a single-variable
// divisibility check, not full Cooper quantifier elimination (filed as
// `mvl-lang/mvl#2022`, a real naming inaccuracy found while porting this).
//
// Adapted to the coherence framing this path serves -- declaration sites,
// where there are no arguments to reason about -- rather than real MVL's
// call-site/hypothesis framing, which lives in `entail_expr` above (#38):
// every clause of the flattened `&&`-conjunction
// (not just the ones L1/L2 left `Unknown`) is converted to a `Constraint`;
// if every clause converts, the *conjunction itself* is checked for
// unsatisfiability (no negation needed, unlike real MVL's refutation-based
// validity proof -- this backend already asks "is this satisfiable", not
// "does it always hold given hypotheses"). `Violated` if UNSAT, `Proven`
// if satisfiable, `None` (→ `Runtime`) if any clause isn't linear.

/// A linear integer expression: `constant + Σ (coeff_i · var_i)`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LinTerm {
    constant: i128,
    vars: HashMap<String, i128>,
}

impl LinTerm {
    fn constant(n: i128) -> Self {
        LinTerm {
            constant: n,
            vars: HashMap::new(),
        }
    }

    fn var(name: impl Into<String>) -> Self {
        let mut vars = HashMap::new();
        vars.insert(name.into(), 1);
        LinTerm { constant: 0, vars }
    }

    fn add(&self, other: &LinTerm) -> LinTerm {
        let mut vars = self.vars.clone();
        for (k, v) in &other.vars {
            let entry = vars.entry(k.clone()).or_insert(0);
            *entry += v;
            if *entry == 0 {
                vars.remove(k);
            }
        }
        LinTerm {
            constant: self.constant + other.constant,
            vars,
        }
    }

    fn sub(&self, other: &LinTerm) -> LinTerm {
        self.add(&other.negate())
    }

    fn scale(&self, c: i128) -> LinTerm {
        if c == 0 {
            return LinTerm::constant(0);
        }
        LinTerm {
            constant: self.constant * c,
            vars: self.vars.iter().map(|(k, v)| (k.clone(), v * c)).collect(),
        }
    }

    fn negate(&self) -> LinTerm {
        self.scale(-1)
    }

    /// `self + 1` — the integer tightening behind every strict bound here
    /// (`t < 0` ⟺ `t + 1 ≤ 0`).
    fn plus_one(&self) -> LinTerm {
        LinTerm {
            constant: self.constant + 1,
            ..self.clone()
        }
    }

    fn is_constant(&self) -> bool {
        self.vars.is_empty()
    }

    fn coeff_of(&self, var: &str) -> i128 {
        self.vars.get(var).copied().unwrap_or(0)
    }

    fn without_var(&self, var: &str) -> LinTerm {
        let mut t = self.clone();
        t.vars.remove(var);
        t
    }
}

/// A linear constraint: `term OP 0`.
#[derive(Debug, Clone)]
enum Constraint {
    /// `term ≤ 0`
    Le(LinTerm),
    /// `term = 0`
    Eq(LinTerm),
    // `Ne` constraints are intentionally not represented here -- real
    // MVL's own `is_unsat` drops them from the Fourier-Motzkin phase
    // entirely (only `Le`/`Eq` feed it), the same known limitation ported
    // faithfully rather than "improved" on.
}

impl Constraint {
    fn is_trivially_false(&self) -> bool {
        match self {
            Constraint::Le(t) => t.is_constant() && t.constant > 0,
            Constraint::Eq(t) => t.is_constant() && t.constant != 0,
        }
    }

    fn is_trivially_true(&self) -> bool {
        match self {
            Constraint::Le(t) => t.is_constant() && t.constant <= 0,
            Constraint::Eq(t) => t.is_constant() && t.constant == 0,
        }
    }
}

/// Extracts a linear integer term from `expr`. Returns `None` for
/// anything non-linear (function calls, variable × variable, field
/// access, ...) -- the same bail-conservatively contract as this
/// backend's existing `int_value`/`ident_name` helpers, generalized to a
/// full linear combination rather than a single literal or bare ident.
fn linterm_from_expr(expr: &Expr) -> Option<LinTerm> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Int(int), ..
        }) => int.base10_parse::<i128>().ok().map(LinTerm::constant),
        Expr::Path(path) if path.qself.is_none() => path
            .path
            .get_ident()
            .map(|ident| LinTerm::var(ident.to_string())),
        Expr::Unary(ExprUnary {
            op: UnOp::Neg(_),
            expr,
            ..
        }) => linterm_from_expr(expr).map(|t| t.negate()),
        Expr::Paren(paren) => linterm_from_expr(&paren.expr),
        Expr::Group(group) => linterm_from_expr(&group.expr),
        Expr::Binary(bin) => {
            let l = linterm_from_expr(&bin.left)?;
            let r = linterm_from_expr(&bin.right)?;
            match bin.op {
                BinOp::Add(_) => Some(l.add(&r)),
                BinOp::Sub(_) => Some(l.sub(&r)),
                // Linear iff one side is a constant scalar.
                BinOp::Mul(_) => {
                    if l.is_constant() {
                        Some(r.scale(l.constant))
                    } else if r.is_constant() {
                        Some(l.scale(r.constant))
                    } else {
                        None
                    }
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Converts `term OP 0` to `Constraint`s, tightening strict integer
/// inequalities the same way real MVL's `cmp_to_constraints` does:
/// `t < 0` ↔ `t+1 ≤ 0`; `t > 0` ↔ `−t+1 ≤ 0`; `t >= 0` ↔ `−t ≤ 0`. `None`
/// for non-comparison operators (`&&`, `+`, ...).
fn cmp_to_constraints(op: &BinOp, term: LinTerm) -> Option<Vec<Constraint>> {
    match op {
        BinOp::Le(_) => Some(vec![Constraint::Le(term)]),
        BinOp::Lt(_) => Some(vec![Constraint::Le(term.plus_one())]),
        BinOp::Ge(_) => Some(vec![Constraint::Le(term.negate())]),
        BinOp::Gt(_) => Some(vec![Constraint::Le(term.negate().plus_one())]),
        BinOp::Eq(_) => Some(vec![Constraint::Eq(term)]),
        // `Ne` has no single-inequality representation; see the
        // `Constraint` enum's own doc comment.
        _ => None,
    }
}

/// Converts one flattened `&&`-conjunct into constraints. `None` if the
/// clause isn't in the linear fragment at all (disjunction, negation,
/// non-linear terms, opaque calls, ...) -- bailing here propagates to
/// `discharge_l4` bailing on the *whole* conjunction, matching real MVL's
/// own all-or-nothing `ref_to_constraints` contract for a single
/// obligation.
fn constraints_from_clause(expr: &Expr) -> Option<Vec<Constraint>> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Bool(b), ..
        }) => Some(if b.value {
            vec![]
        } else {
            vec![Constraint::Le(LinTerm::constant(1))]
        }),
        Expr::Paren(paren) => constraints_from_clause(&paren.expr),
        Expr::Group(group) => constraints_from_clause(&group.expr),
        Expr::Binary(bin) => {
            let l = linterm_from_expr(&bin.left)?;
            let r = linterm_from_expr(&bin.right)?;
            cmp_to_constraints(&bin.op, l.sub(&r))
        }
        _ => None,
    }
}

/// Result of checking a constraint system for satisfiability. Unlike
/// real MVL's own `is_unsat`/`fm_eliminate` (which return a bare `bool`,
/// conflating "rigorously proven satisfiable" with "gave up due to a
/// complexity guard" -- safe for *them* only because their caller
/// (`try_cooper`) never claims satisfiability from this result, only ever
/// escalating to `L5` on anything short of a proven contradiction), this
/// backend needs the distinction explicit: a complexity-guard bail must stay
/// distinguishable from a completed elimination.
///
/// Note the original reason for the split — wanting to claim `Proven` directly
/// from a satisfiable result — was itself the bug (#49). The split survives
/// because "a guard fired" and "elimination completed, satisfiable over ℚ" are
/// still different facts, even though neither is now grounds for `Proven`.
enum SatOutcome {
    /// Rigorously proven contradictory (divisibility failure, a derived
    /// constant contradiction, or a trivially-false input constraint).
    /// Always safe to act on -- no complexity guard ever produces this.
    Contradiction,
    /// Elimination ran to completion with no contradiction found — satisfiable
    /// over the **rationals**, which is all Fourier-Motzkin decides.
    ///
    /// This does **not** imply satisfiable over the integers, and is therefore
    /// never grounds for `Proven` (#49). The name says so, rather than a doc
    /// comment saying so under a name that suggests otherwise: the bug #49
    /// fixed was a caller reading `Satisfiable` and concluding the predicate
    /// was coherent.
    ///
    /// **Do not add an "exact over ℤ" variant without reading this.** The
    /// obvious candidate — trust the divisibility check, since `a | c` means
    /// `a·x + c = 0` has the integer solution `x = -c/a` — is wrong as stated,
    /// because nothing establishes that several such solutions *agree*:
    ///
    /// - `2*x == 6 && 3*x == 6` — each equality passes divisibility (x = 3 and
    ///   x = 2), `le_terms` is empty, so this arm is reached. Jointly UNSAT.
    /// - `2*x == 6 && x > 100` — divisibility passes, and the `Eq` is dropped
    ///   before the elimination phase, so Fourier-Motzkin only ever sees
    ///   `x > 100` and finds it satisfiable. Jointly UNSAT.
    ///
    /// An exact-over-ℤ verdict would have to be confined to a system that is
    /// *exactly one* single-variable equality with `a | c` and nothing else —
    /// which recovers `a*x == c` standing alone and nothing more. Both cases
    /// above are pinned by tests so the boundary stays a boundary.
    SatisfiableOverRationals,
    /// A complexity guard fired before a definite answer was reached;
    /// could be either of the above.
    Unknown,
}

/// Checks whether the conjunction of `constraints` is satisfiable over
/// the integers -- divisibility check for single-variable equalities,
/// then Fourier-Motzkin elimination over the `Le` constraints.
/// Complexity guards (ported verbatim from real MVL): more than 5 free
/// variables, coefficient magnitude, and intermediate constraint count.
fn check_satisfiability(constraints: Vec<Constraint>) -> SatOutcome {
    if constraints.iter().any(Constraint::is_trivially_false) {
        return SatOutcome::Contradiction;
    }

    let constraints: Vec<Constraint> = constraints
        .into_iter()
        .filter(|c| !c.is_trivially_true())
        .collect();

    // Equality + divisibility: `a·x + c = 0` is UNSAT iff `a` doesn't
    // divide `c` (no integer solution).
    for c in &constraints {
        if let Constraint::Eq(t) = c {
            if t.vars.len() == 1 {
                let (_name, &coeff) = t.vars.iter().next().unwrap();
                if t.constant % coeff != 0 {
                    return SatOutcome::Contradiction;
                }
            }
        }
    }

    let le_terms: Vec<LinTerm> = constraints
        .into_iter()
        .filter_map(|c| {
            if let Constraint::Le(t) = c {
                Some(t)
            } else {
                None
            }
        })
        .collect();

    if le_terms.is_empty() {
        return SatOutcome::SatisfiableOverRationals;
    }

    let mut free_vars: Vec<String> = le_terms
        .iter()
        .flat_map(|t| t.vars.keys().cloned())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    free_vars.sort();

    if free_vars.len() > 5 {
        return SatOutcome::Unknown; // Too complex.
    }

    fm_eliminate(le_terms, &free_vars)
}

/// Fourier-Motzkin elimination: eliminate variables one at a time. All
/// `constraints` have the form `term ≤ 0`.
fn fm_eliminate(constraints: Vec<LinTerm>, vars: &[String]) -> SatOutcome {
    if constraints
        .iter()
        .any(|t| t.is_constant() && t.constant > 0)
    {
        return SatOutcome::Contradiction;
    }
    if vars.is_empty() {
        return SatOutcome::SatisfiableOverRationals;
    }

    let var = &vars[0];
    let rest = &vars[1..];

    // Partition by the sign of `var`'s coefficient: positive -> upper
    // bound on var, negative -> lower bound, zero -> carries no
    // information about var and passes through unchanged.
    let mut uppers: Vec<(i128, LinTerm)> = Vec::new();
    let mut lowers: Vec<(i128, LinTerm)> = Vec::new();
    let mut new_constraints: Vec<LinTerm> = Vec::new();

    for t in constraints {
        let c = t.coeff_of(var);
        if c == 0 {
            new_constraints.push(t);
        } else if c > 0 {
            uppers.push((c, t.without_var(var)));
        } else {
            lowers.push((-c, t.without_var(var)));
        }
    }

    // Each (upper a_i, lower b_j) pair produces b_j·r_i + a_i·s_j ≤ 0.
    for (a_i, r_i) in &uppers {
        for (b_j, s_j) in &lowers {
            // Complexity guards (ported verbatim): bail conservatively on
            // huge coefficients or constraint-count blow-up.
            if a_i.unsigned_abs() > 1_000_000 || b_j.unsigned_abs() > 1_000_000 {
                return SatOutcome::Unknown;
            }
            new_constraints.push(r_i.scale(*b_j).add(&s_j.scale(*a_i)));
            if new_constraints.len() > 128 {
                return SatOutcome::Unknown;
            }
        }
    }

    fm_eliminate(new_constraints, rest)
}

/// Tries to discharge a whole `&&`-flattened conjunction via `L4`. `None`
/// if any clause isn't in the linear fragment, or if satisfiability
/// couldn't be rigorously determined either way -- the caller falls
/// through to `Runtime` in both cases.
fn discharge_l4(clauses: &[&Expr]) -> Option<DischargeResult> {
    let mut constraints = Vec::new();
    for clause in clauses {
        constraints.extend(constraints_from_clause(clause)?);
    }

    match check_satisfiability(constraints) {
        SatOutcome::Contradiction => Some(DischargeResult::Violated {
            counterexample: format!(
                "the conjunction `{}` is unsatisfiable over the integers (L4)",
                clauses
                    .iter()
                    .map(|c| quote::quote!(#c).to_string())
                    .collect::<Vec<_>>()
                    .join(" && ")
            ),
        }),
        // NOT `Proven` -- the variant name now says why. Fourier-Motzkin
        // decides satisfiability over the RATIONALS, and only its UNSAT
        // verdict transfers to the integers
        // (ℤ ⊂ ℚ, so no rational solutions means no integer ones). The
        // converse fails: `2*x >= 1 && 2*x <= 1` is satisfiable at x = ½
        // with no integer solution, and `2*x == 2*y + 1` is a parity
        // contradiction FM cannot see. Both reported `Proven { L4 }`
        // before #49.
        //
        // Real MVL acts only on UNSAT here, so this was a divergence the
        // port introduced, not a limitation inherited. Closing the gap
        // properly needs Cooper's divisibility atom (`2 | x'` is exactly
        // what the `Constraint` representation cannot hold) — deferred by
        // ADR-0006 §1 on the reference's own hit rates.
        SatOutcome::SatisfiableOverRationals => None,
        SatOutcome::Unknown => None,
    }
}

/// Flattens nested `a && b && c` (and parenthesized/grouped forms) into
/// its individual conjuncts. A predicate with no top-level `&&` is its
/// own single conjunct.
fn flatten_and<'e>(expr: &'e Expr, out: &mut Vec<&'e Expr>) {
    match expr {
        Expr::Binary(bin) if matches!(bin.op, BinOp::And(_)) => {
            flatten_and(&bin.left, out);
            flatten_and(&bin.right, out);
        }
        Expr::Paren(paren) => flatten_and(&paren.expr, out),
        Expr::Group(group) => flatten_and(&group.expr, out),
        _ => out.push(expr),
    }
}

fn classify_clause(expr: &Expr) -> Clause {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Bool(b), ..
        }) => Clause::Constant(b.value),
        Expr::Paren(paren) => classify_clause(&paren.expr),
        Expr::Group(group) => classify_clause(&group.expr),
        Expr::Binary(bin) => classify_comparison(bin),
        _ => Clause::Unknown,
    }
}

fn classify_comparison(bin: &syn::ExprBinary) -> Clause {
    // `L1` reflexivity, before any var/lit analysis: comparing a call-free
    // term with itself is decided by its operator alone, whatever the term
    // is. This is the only rule that reaches a *non-linear* identity
    // (`a * b == a * b`), which the `L4` split below cannot represent.
    //
    // `is_call_free` is checked on one side only -- `exprs_equivalent` has
    // already established the two are the same tree.
    if exprs_equivalent(&bin.left, &bin.right) && is_call_free(&bin.left) {
        match bin.op {
            BinOp::Eq(_) | BinOp::Le(_) | BinOp::Ge(_) => return Clause::Constant(true),
            BinOp::Ne(_) | BinOp::Lt(_) | BinOp::Gt(_) => return Clause::Constant(false),
            _ => {}
        }
    }

    let left_var = ident_name(&bin.left);
    let left_lit = int_value(&bin.left);
    let right_var = ident_name(&bin.right);
    let right_lit = int_value(&bin.right);

    match (left_var, left_lit, right_var, right_lit) {
        // `lit OP lit` — constant fold.
        (None, Some(l), None, Some(r)) => match constant_fold(&bin.op, l, r) {
            Some(result) => Clause::Constant(result),
            None => Clause::Unknown,
        },
        // `var OP lit`.
        (Some(var), None, None, Some(lit)) => match interval_for(&bin.op, lit) {
            Some(interval) => Clause::Bound { var, interval },
            None => Clause::Unknown,
        },
        // `lit OP var` — flip the comparison direction.
        (None, Some(lit), Some(var), None) => match interval_for(&flip(&bin.op), lit) {
            Some(interval) => Clause::Bound { var, interval },
            None => Clause::Unknown,
        },
        _ => Clause::Unknown,
    }
}

/// Structural equality on two expressions, transparent to parenthesization
/// on either side independently. Ported from `mvl-lang/mvl`'s
/// `preds_equivalent` (`src/mvl/checker/solver/layer1.rs`), whose `Grouped`
/// arm matches one-sided grouping the same way.
///
/// Per-operand transparency is what carries the weight here, not just
/// one-sided grouping at the top: [`SubstituteExprs`] wraps *each*
/// identifier it splices in, so a predicate `lo <= hi` substituted with
/// `lo := a, hi := b` arrives as `(a) <= (b)` — grouped inside each operand.
/// Peeling only the outermost layer and comparing with `==` would still see
/// those `Paren` nodes and say no. (A return-site obligation, where one
/// expression replaces `result` wholesale, is the one-sided
/// `(a + b) == a + b` shape instead; both need to work.)
///
/// Structural only: it answers "same tree?", not "same value?". Whether a
/// term may be compared with itself at all is [`is_call_free`]'s question,
/// asked separately by the one caller that needs it.
fn exprs_equivalent(a: &Expr, b: &Expr) -> bool {
    let (a, b) = (ungroup(a), ungroup(b));
    match (a, b) {
        (Expr::Binary(x), Expr::Binary(y)) => {
            x.op == y.op
                && exprs_equivalent(&x.left, &y.left)
                && exprs_equivalent(&x.right, &y.right)
        }
        (Expr::Unary(x), Expr::Unary(y)) => x.op == y.op && exprs_equivalent(&x.expr, &y.expr),
        // `syn`'s `PartialEq` (the `extra-traits` feature) compares token
        // structure and ignores spans, which is exactly the comparison
        // wanted for the leaves.
        _ => a == b,
    }
}

/// `expr` with any layers of parenthesization or invisible grouping peeled off.
fn ungroup(expr: &Expr) -> &Expr {
    match expr {
        Expr::Paren(paren) => ungroup(&paren.expr),
        Expr::Group(group) => ungroup(&group.expr),
        other => other,
    }
}

/// Whether `expr` is built only from shapes that cannot invoke user code,
/// which is what makes comparing it with itself decidable by the operator
/// alone (#43).
///
/// Without this gate reflexivity is wrong in both directions, and call-site
/// substitution reaches both: `substitute_exprs` parenthesizes each argument
/// independently, so `span(gen(), gen())` against `requires(lo <= hi)`
/// becomes `(gen ()) <= (gen ())` — `Proven`, dropping a check that can
/// genuinely fail — while `requires(a != b)` becomes `(gen ()) != (gen ())`
/// — `Violated`, a compile error on a valid call. Two calls to `gen` are the
/// same *tokens* but not the same *value*.
///
/// An allow-list rather than a list of shapes to reject, matching
/// [`linterm_from_expr`]'s bail-conservatively contract: anything
/// unrecognized is assumed able to call something. Operators are taken to
/// mean integer arithmetic rather than arbitrary `impl`s, which is the same
/// assumption the unbounded-ℤ scope already makes everywhere else here.
/// `Deref` is left out deliberately — reading through a pointer twice is a
/// question about aliasing, not about arithmetic.
///
/// Real MVL needs no such check: its `RefExpr` grammar cannot express a call
/// at all, so restricting to this fragment is what *matches* upstream. A
/// real purity signal belongs in `rust-effect`, which already models
/// `#[mvl::effect(...)]`; this is the syntactic approximation until then.
fn is_call_free(expr: &Expr) -> bool {
    match expr {
        Expr::Lit(_) => true,
        Expr::Path(path) => path.qself.is_none(),
        Expr::Paren(paren) => is_call_free(&paren.expr),
        Expr::Group(group) => is_call_free(&group.expr),
        Expr::Binary(bin) => is_call_free(&bin.left) && is_call_free(&bin.right),
        Expr::Unary(unary) => {
            matches!(unary.op, UnOp::Neg(_) | UnOp::Not(_)) && is_call_free(&unary.expr)
        }
        Expr::Field(field) => is_call_free(&field.base),
        Expr::Index(index) => is_call_free(&index.expr) && is_call_free(&index.index),
        _ => false,
    }
}

/// The interval a variable must lie in for `var OP lit` to hold, for the
/// comparison ops expressible as a single bound. `Ne` has no representation
/// as one contiguous interval (it excludes a single point), so it's left
/// undecidable by this backend rather than approximated.
fn interval_for(op: &BinOp, lit: i128) -> Option<Interval> {
    match op {
        BinOp::Lt(_) => Some(Interval::at_most(lit - 1)),
        BinOp::Le(_) => Some(Interval::at_most(lit)),
        BinOp::Gt(_) => Some(Interval::at_least(lit + 1)),
        BinOp::Ge(_) => Some(Interval::at_least(lit)),
        BinOp::Eq(_) => Some(Interval::point(lit)),
        _ => None,
    }
}

/// Flips a comparison so `lit OP var` can be re-read as `var OP' lit`
/// (e.g. `0 <= x` becomes `x >= 0`).
fn flip(op: &BinOp) -> BinOp {
    match op {
        BinOp::Lt(t) => BinOp::Gt(syn::token::Gt(t.spans)),
        BinOp::Le(t) => BinOp::Ge(syn::token::Ge(t.spans)),
        BinOp::Gt(t) => BinOp::Lt(syn::token::Lt(t.spans)),
        BinOp::Ge(t) => BinOp::Le(syn::token::Le(t.spans)),
        other => *other,
    }
}

fn constant_fold(op: &BinOp, l: i128, r: i128) -> Option<bool> {
    match op {
        BinOp::Lt(_) => Some(l < r),
        BinOp::Le(_) => Some(l <= r),
        BinOp::Gt(_) => Some(l > r),
        BinOp::Ge(_) => Some(l >= r),
        BinOp::Eq(_) => Some(l == r),
        BinOp::Ne(_) => Some(l != r),
        _ => None,
    }
}

/// Single bare identifier (`x`, `result`, ...) — not a path with multiple
/// segments, since those aren't ordinary local variables.
fn ident_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Path(path) if path.qself.is_none() => {
            let segment = path.path.get_ident()?;
            Some(segment.to_string())
        }
        Expr::Paren(paren) => ident_name(&paren.expr),
        Expr::Group(group) => ident_name(&group.expr),
        _ => None,
    }
}

/// An integer literal, including a leading unary `-` (`-5` parses as
/// `Expr::Unary(Neg, Expr::Lit(5))`, not as a single negative literal
/// token).
fn int_value(expr: &Expr) -> Option<i128> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Int(int), ..
        }) => int.base10_parse::<i128>().ok(),
        Expr::Unary(ExprUnary {
            op: UnOp::Neg(_),
            expr,
            ..
        }) => int_value(expr).map(|v| -v),
        Expr::Paren(paren) => int_value(&paren.expr),
        Expr::Group(group) => int_value(&group.expr),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn discharge(src: &str) -> DischargeResult {
        let pred: Predicate = syn::parse_str(src).unwrap();
        discharge_predicate(&pred)
    }

    #[test]
    fn literal_true_is_l1() {
        assert_eq!(
            discharge("true"),
            DischargeResult::Proven { layer: Layer::L1 }
        );
    }

    #[test]
    fn literal_false_is_violated() {
        assert!(matches!(
            discharge("false"),
            DischargeResult::Violated { .. }
        ));
    }

    #[test]
    fn constant_comparison_is_l1() {
        assert_eq!(
            discharge("5 >= 0"),
            DischargeResult::Proven { layer: Layer::L1 }
        );
    }

    #[test]
    fn satisfiable_interval_is_l2() {
        assert_eq!(
            discharge("x >= 0 && x < 100"),
            DischargeResult::Proven { layer: Layer::L2 }
        );
    }

    #[test]
    fn flipped_comparison_is_l2() {
        assert_eq!(
            discharge("0 <= result && result <= 15"),
            DischargeResult::Proven { layer: Layer::L2 }
        );
    }

    #[test]
    fn contradictory_interval_is_violated() {
        let result = discharge("x >= 10 && x < 5");
        assert!(matches!(result, DischargeResult::Violated { .. }));
    }

    #[test]
    fn equality_bound_is_l2() {
        assert_eq!(
            discharge("x == 5"),
            DischargeResult::Proven { layer: Layer::L2 }
        );
    }

    #[test]
    fn contradictory_equality_is_violated() {
        assert!(matches!(
            discharge("x == 5 && x == 6"),
            DischargeResult::Violated { .. }
        ));
    }

    #[test]
    fn function_call_is_runtime() {
        assert_eq!(discharge("len(sections) == 51"), DischargeResult::Runtime);
    }

    #[test]
    fn disjunction_is_runtime() {
        assert_eq!(discharge("x >= 0 || x < 0"), DischargeResult::Runtime);
    }

    #[test]
    fn not_equal_is_runtime() {
        assert_eq!(discharge("x != 5"), DischargeResult::Runtime);
    }

    #[test]
    fn mixed_bound_and_undecidable_stays_runtime() {
        assert_eq!(
            discharge("x >= 0 && has_valid_shape(x)"),
            DischargeResult::Runtime
        );
    }

    #[test]
    fn undecidable_does_not_hide_a_definite_contradiction() {
        assert!(matches!(
            discharge("false && has_valid_shape(x)"),
            DischargeResult::Violated { .. }
        ));
    }

    // Quantifier tests below mirror `mvl-lang/mvl`'s own real test
    // fixtures verbatim (`tests/solver/layer3/11_bounded_forall_
    // conjunction.mvl`, `13_bounded_quantifier_violation.mvl`,
    // `14_bounded_expansion_cap.mvl`), confirmed against the real
    // implementation rather than assumed.

    #[test]
    fn bounded_forall_conjunction_proves_at_l3() {
        // Mirrors 11_bounded_forall_conjunction.mvl: every instance
        // reduces to a closed comparison L1 discharges.
        assert_eq!(
            discharge("forall i in [0..9] . i < 10"),
            DischargeResult::Proven { layer: Layer::L3 }
        );
    }

    #[test]
    fn bounded_forall_violation_short_circuits_with_a_counterexample() {
        // Mirrors 13_bounded_quantifier_violation.mvl: fails at i = 5.
        let result = discharge("forall i in [0..9] . i < 5");
        match result {
            DischargeResult::Violated { counterexample } => {
                assert!(counterexample.contains("i = 5"), "{counterexample}");
            }
            other => panic!("expected Violated, got {other:?}"),
        }
    }

    #[test]
    fn bounded_forall_over_cap_falls_to_runtime_not_l5() {
        // Mirrors 14_bounded_expansion_cap.mvl: width 2001 > 1000, so this
        // must not be expanded at all -- straight to Runtime.
        assert_eq!(
            discharge("forall i in [0..2000] . i >= 0"),
            DischargeResult::Runtime
        );
    }

    #[test]
    fn forall_over_an_opaque_call_falls_through_to_runtime() {
        // The require_dense_fleet shape: L3 expansion is not the same as
        // proof -- an unrecognized call inside the body still yields
        // Runtime per-instance, so the aggregate is Runtime, not Proven.
        assert_eq!(
            discharge("forall i in [1..50] . sections_get(i) != 0"),
            DischargeResult::Runtime
        );
    }

    #[test]
    fn forall_over_empty_range_is_vacuously_proven() {
        assert_eq!(
            discharge("forall i in [5..3] . i > 100"),
            DischargeResult::Proven { layer: Layer::L3 }
        );
    }

    #[test]
    fn exists_with_a_satisfying_witness_proves_at_l3() {
        assert_eq!(
            discharge("exists i in [0..9] . i == 5"),
            DischargeResult::Proven { layer: Layer::L3 }
        );
    }

    #[test]
    fn exists_with_no_witness_is_violated() {
        assert!(matches!(
            discharge("exists i in [0..9] . i == 50"),
            DischargeResult::Violated { .. }
        ));
    }

    #[test]
    fn exists_over_empty_range_is_violated() {
        assert!(matches!(
            discharge("exists i in [5..3] . true"),
            DischargeResult::Violated { .. }
        ));
    }

    #[test]
    fn nested_quantifiers_discharge_correctly() {
        // forall i in [0..2]: exists j in [0..2], i == j -- always true
        // since j can always equal i.
        assert_eq!(
            discharge("forall i in [0..2] . exists j in [0..2] . i == j"),
            DischargeResult::Proven { layer: Layer::L3 }
        );
    }

    #[test]
    fn shadowed_bound_variable_is_not_substituted_by_the_outer_quantifier() {
        // The inner `forall i` rebinds `i`; substituting the outer `i`
        // must not reach inside it.
        assert_eq!(
            discharge("forall i in [0..2] . forall i in [10..12] . i >= 10"),
            DischargeResult::Proven { layer: Layer::L3 }
        );
    }

    #[test]
    fn negative_bounds_discharge_correctly() {
        assert_eq!(
            discharge("forall i in [-5..5] . i >= -5 && i <= 5"),
            DischargeResult::Proven { layer: Layer::L3 }
        );
    }

    // L4 tests (#35): cross-variable linear reasoning L2's per-variable
    // interval model can't reach on its own.

    #[test]
    fn cross_variable_contradiction_is_violated_at_l4() {
        // No single clause is a contradiction on its own; the conjunction
        // is, via a > c, b > 0 => a + b > c, contradicting a + b <= c.
        let result = discharge("a > c && b > 0 && a + b <= c");
        match result {
            DischargeResult::Violated { counterexample } => {
                assert!(counterexample.contains("L4"), "{counterexample}");
            }
            other => panic!("expected Violated, got {other:?}"),
        }
    }

    #[test]
    fn paper_motivating_example_adapted_to_satisfiability_is_violated() {
        // The paper's own L4 example proves `x + 5 <= 245` given
        // `x <= 240`; adapted to this backend's satisfiability framing,
        // asserting both `x <= 240` and its negation-adjacent
        // `x + 5 > 245` in one predicate is the analogous contradiction.
        assert!(matches!(
            discharge("x <= 240 && x + 5 > 245"),
            DischargeResult::Violated { .. }
        ));
    }

    #[test]
    fn a_rationally_satisfiable_relation_is_not_proven_coherent() {
        // Integer-satisfiable in fact (a=1, b=1, c=0), but Fourier-Motzkin
        // only established ℚ-satisfiability — the integer solution is a
        // coincidence it did not derive. Acting on that verdict is what #49
        // fixed, so `Runtime` here is the honest outcome even though the
        // predicate is coherent.
        assert_eq!(
            discharge("a > c && b > 0 && a + b >= c"),
            DischargeResult::Runtime
        );
    }

    #[test]
    fn single_variable_equality_with_no_divisor_is_violated() {
        // 2*x - 5 = 0 has no integer solution (2 does not divide 5).
        assert!(matches!(
            discharge("2 * x == 5"),
            DischargeResult::Violated { .. }
        ));
    }

    #[test]
    fn two_equalities_each_passing_divisibility_can_still_be_jointly_unsat() {
        // Pins the boundary documented on `SatOutcome::SatisfiableOverRationals`
        // (#60). `2*x == 6` gives x = 3 and `3*x == 6` gives x = 2; each passes
        // the divisibility check independently, `le_terms` is empty, so the
        // "elimination completed" arm is reached -- but there is no joint
        // solution. Trusting divisibility as an exact-over-ℤ verdict would
        // report this `Proven`, reintroducing #49 by a different route.
        assert_eq!(
            discharge("2 * x == 6 && 3 * x == 6"),
            DischargeResult::Runtime
        );
    }

    #[test]
    fn an_equality_and_an_inequality_can_be_jointly_unsat_unseen() {
        // The second boundary case (#60): divisibility passes, and the `Eq` is
        // dropped before the elimination phase, so Fourier-Motzkin only ever
        // sees `x > 100` and finds it satisfiable. `x` must be 3, so the
        // conjunction is UNSAT -- and nothing in this backend establishes that.
        assert_eq!(discharge("2 * x == 6 && x > 100"), DischargeResult::Runtime);
    }

    #[test]
    fn a_rationally_satisfiable_integer_unsatisfiable_predicate_is_not_proven() {
        // #49. Fourier-Motzkin decides ℚ-satisfiability; only its UNSAT
        // verdict transfers to ℤ. `2*x >= 1 && 2*x <= 1` is satisfiable at
        // x = ½ and has no integer solution — eliminating x yields
        // (-1)·2 + 1·2 = 0, and 0 is not > 0, so no contradiction is derived
        // and FM reports satisfiable. Reported `Proven { L4 }` before the fix.
        assert_eq!(
            discharge("2 * x >= 1 && 2 * x <= 1"),
            DischargeResult::Runtime
        );
    }

    #[test]
    fn a_parity_contradiction_is_not_proven() {
        // #49, the sharper case: `2*x == 2*y + 1` is integer-unsatisfiable
        // for every x and y. It escapes the divisibility check because that
        // only fires on a single-variable equality, and FM cannot represent
        // the divisibility atom (`2 | x'`) that would settle it — which is
        // exactly what Cooper's algorithm adds and ADR-0006 §1 defers.
        assert_eq!(discharge("2 * x == 2 * y + 1"), DischargeResult::Runtime);
    }

    #[test]
    fn an_honestly_satisfiable_predicate_still_proves_at_l2() {
        // The fix must not cost the cases L2 already decides exactly:
        // interval containment over literal bounds is sound over ℤ.
        assert_eq!(
            discharge("x > 0 && x < 10"),
            DischargeResult::Proven { layer: Layer::L2 }
        );
    }

    #[test]
    fn single_variable_equality_with_a_divisor_is_no_longer_proven() {
        // `2*x == 6` IS integer-satisfiable (x = 3), and the divisibility
        // check establishes that exactly: `a | c` means `x = -c/a` is an
        // integer. So the old `Proven { L4 }` was sound for this shape.
        //
        // #49's fix drops it anyway, because `check_satisfiability` collapses
        // "satisfiable, proven exactly over ℤ by divisibility" and
        // "satisfiable over ℚ only" into one `Satisfiable` verdict, and the
        // second is unsound to act on. Recovering this case needs the two
        // distinguished — tracked separately rather than smuggled into a
        // soundness fix. A precision regression, deliberately taken.
        assert_eq!(discharge("2 * x == 6"), DischargeResult::Runtime);
    }

    #[test]
    fn more_than_five_free_variables_falls_to_runtime() {
        assert_eq!(
            discharge("a + b + c + d + e + f <= 100 && a + b + c + d + e + f >= 200"),
            DischargeResult::Runtime
        );
    }

    #[test]
    fn non_linear_term_falls_to_runtime_not_l4() {
        assert_eq!(discharge("x * y > 0 && x < 0"), DischargeResult::Runtime);
    }

    #[test]
    fn l1_l2_still_take_priority_over_l4_when_they_fully_decide() {
        // A pure per-variable interval case must still report L2, not L4,
        // even though L4 could also decide it -- L4 only engages when
        // L1/L2 leave an undecidable residue.
        assert_eq!(
            discharge("x >= 0 && x < 100"),
            DischargeResult::Proven { layer: Layer::L2 }
        );
    }
}
