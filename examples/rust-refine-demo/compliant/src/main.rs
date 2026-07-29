//! Demonstrates `rust-refine`'s native `L1`+`L2` obligation dispatch
//! (ADR-0005) with satisfiable interval bounds -- the `mask_low_nibble`
//! example from #22's real-`mvl`-example comparison -- plus `L3` bounded
//! quantifier expansion (#31), mirroring `mvl-lang/mvl`'s own real
//! `examples/cbtc_train_presence/invariants.mvl::require_dense_fleet` --
//! plus `L4` cross-variable linear arithmetic (#35), which `L2`'s
//! per-variable-only interval model can't reach on its own -- plus
//! call-site obligations discharged against a hypothesis context (#38),
//! where the argument, the caller's own preconditions, branch conditions
//! and propagated postconditions all take part.

// Since #42 the `ensures` is also checked against the body, not just for
// coherence. Here that lands on a runtime check rather than a proof: the
// return-site goal is `0 <= (b & 15) && (b & 15) <= 15`, and `&` is not in
// the `Le`/`Eq` linear fragment the native backend reasons over -- bitwise
// masking needs L5 (#37). A runtime check is the honest outcome, and
// `at_least_ten` below shows the same obligation closing at L1 when the
// returned expression *is* linear.
#[mvl::total]
#[mvl::requires(0 <= b && b <= 255)]
#[mvl::ensures(0 <= result && result <= 15)]
fn mask_low_nibble(b: i32) -> i32 {
    b & 15
}

// Every section index in [1..50] has a matching entry -- an opaque call
// (`section_occupied`) inside the quantifier body, so L3 expands the
// range into 50 obligations but can't decide any of them; the aggregate
// falls to a runtime check rather than a compile-time proof, matching
// the real invariants.mvl's own documented expectation.
fn section_occupied(section: i32) -> bool {
    section >= 1 && section <= 50
}

#[mvl::total]
#[mvl::requires(forall i in [1..50] . section_occupied(i))]
fn require_dense_fleet() -> bool {
    true
}

// No single clause bounds `a - b` alone; L2 can't derive that a caller
// satisfying both hypotheses also satisfies the third clause, but L4's
// Fourier-Motzkin elimination confirms the three clauses are jointly
// satisfiable (e.g. a=2, b=1, c=1).
#[mvl::requires(a > c && b > 0 && a + b >= c)]
fn cross_variable_bound(a: i32, b: i32, c: i32) -> i32 {
    a + b - c
}

// ── Call-site obligations against a hypothesis context Γ (#38) ───────────
//
// Everything above is a *declaration*-site obligation: is the predicate
// coherent on its own? The functions below exercise the other question --
// does what's known at a call site entail the callee's precondition, with
// the actual arguments substituted in?

#[mvl::requires(n > 5)]
fn require_gt5(n: i32) -> i32 {
    n
}

// L4. Nothing bounds `y` on its own, so L2 can't read this at all, but
// `x > 10 ∧ y > x ∧ ¬(y > 5)` is unsatisfiable -- Fourier-Motzkin closes
// it. This is the shape `mvl-lang/mvl` needs its Z3 layer for
// (`tests/solver/layer5/01_chained_hypotheses.mvl`).
#[mvl::requires(x > 10 && y > x)]
fn chained_hypotheses(x: i32, y: i32) -> i32 {
    require_gt5(y)
}

// L2 via branch narrowing: `x` carries no refinement of its own, but
// inside the `if` its lower bound is known.
fn narrowed_by_a_branch(x: i32) -> i32 {
    if x > 5 {
        require_gt5(x)
    } else {
        0
    }
}

// Two obligations now, from one attribute: the declaration-site coherence
// check, and the return-site obligation that the body actually establishes
// it (#42) -- `result := 10` gives `(10) >= 10`, closed at L1. That second
// one is what makes the propagation below sound rather than assumed.
#[mvl::ensures(result >= 10)]
fn at_least_ten() -> i32 {
    10
}

// L2 via postcondition propagation: `at_least_ten`'s `ensures` becomes a
// fact about `y`, which is what makes the call below provable.
fn uses_a_postcondition() -> i32 {
    let y = at_least_ten();
    require_gt5(y)
}

fn main() {
    // Bound through `let` rather than passed straight to `println!`: a call
    // inside a macro invocation is invisible to `syn`, so it would produce
    // no call-site obligation at all.
    let masked = mask_low_nibble(200);
    let occupied = section_occupied(25);
    let dense = require_dense_fleet();
    let crossed = cross_variable_bound(2, 1, 1);
    let chained = chained_hypotheses(11, 12);
    let narrowed = narrowed_by_a_branch(6);
    let from_postcondition = uses_a_postcondition();

    println!("mask_low_nibble(200) = {masked}");
    println!("section_occupied(25) = {occupied}");
    println!("require_dense_fleet() = {dense}");
    println!("cross_variable_bound(2, 1, 1) = {crossed}");
    println!("chained_hypotheses(11, 12) = {chained}");
    println!("narrowed_by_a_branch(6) = {narrowed}");
    println!("uses_a_postcondition() = {from_postcondition}");
}
