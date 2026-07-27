//! Demonstrates `rust-refine`'s native `L1`+`L2` obligation dispatch
//! (ADR-0001) with satisfiable interval bounds -- the `mask_low_nibble`
//! example from #22's real-`mvl`-example comparison -- plus `L3` bounded
//! quantifier expansion (#31), mirroring `mvl-lang/mvl`'s own real
//! `examples/cbtc_train_presence/invariants.mvl::require_dense_fleet` --
//! plus `L4` cross-variable linear arithmetic (#35), which `L2`'s
//! per-variable-only interval model can't reach on its own.

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

fn main() {
    println!("mask_low_nibble(200) = {}", mask_low_nibble(200));
    println!("section_occupied(25) = {}", section_occupied(25));
    println!("require_dense_fleet() = {}", require_dense_fleet());
    println!(
        "cross_variable_bound(2, 1, 1) = {}",
        cross_variable_bound(2, 1, 1)
    );
}
