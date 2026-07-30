//! `L5`: Z3 SMT dispatch for entailments `L1`-`L4` leave `Runtime` (#37).
//!
//! Narrowed scope, per the issue's own two rounds of re-scoping (grounded
//! against real MVL's `src/mvl/checker/solver/layer5.rs`, then re-verified
//! against this backend's own current state):
//!
//! - Real MVL's `try_z3` dispatches on predicate shape across four sorts
//!   (string, bitwise, float, int). This grammar (`crate::attrs::Predicate`)
//!   has no string/bitwise/float surface at all, so only the `Int`/QF-NIA
//!   path has anything to encode. The other three are a follow-up gated on
//!   the grammar growing that surface, not on this module.
//! - Of the two motivating reproducers the issue verified, one is now
//!   stale: equality-goal entailment (`x == 4 && y == x + 1 ⇒ y == 5`) closes
//!   at `L4` today via its equality-splitting (`#43`), which postdates the
//!   issue's own research. The other is real and current: genuine
//!   nonlinearity (`a > 2 && b > 2 ⇒ a * b > 4`) — `linterm_from_expr`
//!   refuses variable×variable by construction, so Fourier-Motzkin cannot
//!   represent it regardless of how small the system is. This module exists
//!   for that case, and for `L4`'s complexity-guard bailouts (`#37`'s own
//!   "weakest of the three motivations" — the same mechanism covers it for
//!   free, with no dedicated logic, since Z3 doesn't care *why* `L1`-`L4`
//!   gave up).
//! - **Proof direction only.** This module answers "does Γ entail every
//!   goal clause", never "is the goal violated" — disproof is already
//!   `L1`-`L4`'s job (`refutes_negation`'s sibling logic), and real MVL's
//!   own model-extraction/witness classification for the `Violated`
//!   direction is out of scope here; a future ticket can add it once a
//!   nonlinear counterexample is a demonstrated real need, not a
//!   speculative one.
//!
//! Feature-gated and default-off (`cargo build --features z3` /
//! `--features rust-refine/z3` / `--features cargo-mvl/z3`): this
//! workspace's default CI has no Z3 installed, and the point of a feature
//! gate is that its absence changes nothing about the build a casual
//! contributor runs. Disabled, [`try_entail_all`] is a no-op returning
//! `false`, so [`crate::solver::native::discharge_entailment`]'s caller
//! falls through to `DischargeResult::Runtime` exactly as it did before
//! this module existed.

use syn::Expr;

/// Whether `hypotheses` entails every clause in `goals` — checked as one
/// query, `Γ ∧ ¬(g1 ∧ g2 ∧ … ∧ gn)` UNSAT, since conjunction distributes
/// over entailment: `Γ ⊨ (A ∧ B)` iff `Γ ⊨ A` and `Γ ⊨ B`. `false` on `Sat`
/// (not entailed), `Unknown`/timeout (undecided within budget), or if any
/// hypothesis or goal falls outside the encodable fragment — comparisons
/// and boolean connectives over integer `+`/`-`/`*` and literals/identifiers.
/// Never a panic: an unencodable expression is treated the same as an
/// undecided one, which is what lets the caller safely try this
/// unconditionally rather than needing to detect "is this nonlinear" first.
#[cfg(feature = "z3")]
pub fn try_entail_all(hypotheses: &[&Expr], goals: &[&Expr]) -> bool {
    real::try_entail_all(hypotheses, goals)
}

#[cfg(not(feature = "z3"))]
pub fn try_entail_all(_hypotheses: &[&Expr], _goals: &[&Expr]) -> bool {
    false
}

#[cfg(feature = "z3")]
mod real {
    use std::collections::HashMap;

    use syn::{BinOp, Expr, ExprLit, Lit, UnOp};
    use z3::ast::{Ast, Bool, Int};
    use z3::{Config, Context, SatResult, Solver};

    /// Matches ADR-0006 Section 5's condition on enforcement's own release
    /// build (#53) and real MVL's own `try_z3` default -- a bound on
    /// *wall-clock*, not on problem shape, so it degrades gracefully on
    /// whatever this backend hands it rather than needing its own
    /// complexity heuristic.
    const TIMEOUT_MS: u64 = 1_000;

    pub fn try_entail_all(hypotheses: &[&Expr], goals: &[&Expr]) -> bool {
        if goals.is_empty() {
            return true; // Vacuously entailed -- nothing to prove.
        }

        let mut config = Config::new();
        config.set_timeout_msec(TIMEOUT_MS);
        let ctx = Context::new(&config);
        let mut vars: HashMap<String, Int> = HashMap::new();

        let Some(hyp_asts) = encode_all(&ctx, &mut vars, hypotheses) else {
            return false;
        };
        let Some(goal_asts) = encode_all(&ctx, &mut vars, goals) else {
            return false;
        };

        let solver = Solver::new(&ctx);
        for hyp in &hyp_asts {
            solver.assert(hyp);
        }
        let goal_refs: Vec<&Bool> = goal_asts.iter().collect();
        let goal_conjunction = Bool::and(&ctx, &goal_refs);
        solver.assert(&goal_conjunction.not());

        matches!(solver.check(), SatResult::Unsat)
    }

    fn encode_all<'ctx>(
        ctx: &'ctx Context,
        vars: &mut HashMap<String, Int<'ctx>>,
        clauses: &[&Expr],
    ) -> Option<Vec<Bool<'ctx>>> {
        clauses.iter().map(|e| encode_bool(ctx, vars, e)).collect()
    }

    /// A variable is encoded as a Z3 `Int` unconditionally — this grammar has
    /// no type annotations to consult (`syn`-only, no `rustc` type info), and
    /// every quantity `Predicate::Expr` can name is integer arithmetic. The
    /// same free variable seen in both hypotheses and goals must resolve to
    /// the same Z3 const, which `vars` guarantees by name.
    fn encode_int<'ctx>(
        ctx: &'ctx Context,
        vars: &mut HashMap<String, Int<'ctx>>,
        expr: &Expr,
    ) -> Option<Int<'ctx>> {
        match expr {
            Expr::Paren(paren) => encode_int(ctx, vars, &paren.expr),
            Expr::Group(group) => encode_int(ctx, vars, &group.expr),
            Expr::Path(path) => {
                let name = path.path.get_ident()?.to_string();
                if let Some(existing) = vars.get(&name) {
                    return Some(existing.clone());
                }
                let fresh = Int::new_const(ctx, name.as_str());
                vars.insert(name, fresh.clone());
                Some(fresh)
            }
            Expr::Lit(ExprLit {
                lit: Lit::Int(int), ..
            }) => {
                let value: i64 = int.base10_parse().ok()?;
                Some(Int::from_i64(ctx, value))
            }
            Expr::Unary(unary) if matches!(unary.op, UnOp::Neg(_)) => {
                let inner = encode_int(ctx, vars, &unary.expr)?;
                Some(Int::from_i64(ctx, 0) - inner)
            }
            Expr::Binary(bin) => {
                let left = encode_int(ctx, vars, &bin.left)?;
                let right = encode_int(ctx, vars, &bin.right)?;
                match bin.op {
                    BinOp::Add(_) => Some(left + right),
                    BinOp::Sub(_) => Some(left - right),
                    // The one case `L1`-`L4` cannot represent by
                    // construction (`linterm_from_expr` refuses
                    // variable*variable) — this is the whole reason this
                    // module exists.
                    BinOp::Mul(_) => Some(left * right),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn encode_bool<'ctx>(
        ctx: &'ctx Context,
        vars: &mut HashMap<String, Int<'ctx>>,
        expr: &Expr,
    ) -> Option<Bool<'ctx>> {
        match expr {
            Expr::Paren(paren) => encode_bool(ctx, vars, &paren.expr),
            Expr::Group(group) => encode_bool(ctx, vars, &group.expr),
            Expr::Lit(ExprLit {
                lit: Lit::Bool(b), ..
            }) => Some(Bool::from_bool(ctx, b.value)),
            Expr::Unary(unary) if matches!(unary.op, UnOp::Not(_)) => {
                encode_bool(ctx, vars, &unary.expr).map(|b| b.not())
            }
            Expr::Binary(bin) => match bin.op {
                BinOp::And(_) => {
                    let left = encode_bool(ctx, vars, &bin.left)?;
                    let right = encode_bool(ctx, vars, &bin.right)?;
                    Some(Bool::and(ctx, &[&left, &right]))
                }
                BinOp::Or(_) => {
                    let left = encode_bool(ctx, vars, &bin.left)?;
                    let right = encode_bool(ctx, vars, &bin.right)?;
                    Some(Bool::or(ctx, &[&left, &right]))
                }
                BinOp::Eq(_)
                | BinOp::Ne(_)
                | BinOp::Lt(_)
                | BinOp::Le(_)
                | BinOp::Gt(_)
                | BinOp::Ge(_) => {
                    let left = encode_int(ctx, vars, &bin.left)?;
                    let right = encode_int(ctx, vars, &bin.right)?;
                    Some(match bin.op {
                        BinOp::Eq(_) => left._eq(&right),
                        BinOp::Ne(_) => left._eq(&right).not(),
                        BinOp::Lt(_) => left.lt(&right),
                        BinOp::Le(_) => left.le(&right),
                        BinOp::Gt(_) => left.gt(&right),
                        BinOp::Ge(_) => left.ge(&right),
                        _ => unreachable!("matched above"),
                    })
                }
                _ => None,
            },
            _ => None,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use syn::parse_str;

        fn expr(src: &str) -> Expr {
            parse_str(src).expect("test expr parses")
        }

        #[test]
        fn nonlinear_entailment_proves() {
            // The motivating reproducer (#37): `a > 2 && b > 2 => a * b > 4`
            // is trivial for QF-NIA, out of reach for Fourier-Motzkin by
            // construction.
            let hyps = [expr("a > 2 && b > 2")];
            let goals = [expr("a * b > 4")];
            let hyp_refs: Vec<&Expr> = hyps.iter().collect();
            let goal_refs: Vec<&Expr> = goals.iter().collect();
            assert!(try_entail_all(&hyp_refs, &goal_refs));
        }

        #[test]
        fn an_unprovable_nonlinear_goal_is_not_claimed_entailed() {
            let hyps = [expr("a > 0 && b > 0")];
            let goals = [expr("a * b > 1000")];
            let hyp_refs: Vec<&Expr> = hyps.iter().collect();
            let goal_refs: Vec<&Expr> = goals.iter().collect();
            assert!(!try_entail_all(&hyp_refs, &goal_refs));
        }

        #[test]
        fn an_unencodable_clause_falls_through_rather_than_panicking() {
            // A bare function call is outside the encodable fragment on
            // either side.
            let hyps = [expr("a > 0")];
            let goals = [expr("f(a) > 0")];
            let hyp_refs: Vec<&Expr> = hyps.iter().collect();
            let goal_refs: Vec<&Expr> = goals.iter().collect();
            assert!(!try_entail_all(&hyp_refs, &goal_refs));
        }

        #[test]
        fn no_goals_is_vacuously_entailed() {
            let hyps = [expr("a > 0")];
            let hyp_refs: Vec<&Expr> = hyps.iter().collect();
            assert!(try_entail_all(&hyp_refs, &[]));
        }
    }
}
