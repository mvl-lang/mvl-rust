//! Demonstrates the two ways a refinement obligation genuinely fails: a
//! self-contradictory declaration, and a call site whose arguments can
//! never satisfy the callee's precondition given what's known there.
//!
//! The declaration case first:
//! this is valid, compiling Rust (the attribute is a no-op pass-through),
//! but no integer `x` can ever satisfy `x >= 10 && x < 5` -- `rust-refine`
//! flags it as violated rather than silently accepting it. Run
//! `cargo mvl-refine src/main.rs` (with the binary on `PATH`) to see the
//! diagnostic.

#[mvl::requires(x >= 10 && x < 5)]
fn impossible(x: i32) -> i32 {
    x
}

// ── Call-site violations (#38) ───────────────────────────────────────────

#[mvl::requires(n > 0)]
fn require_positive(n: i32) -> i32 {
    n
}

// The argument is substituted into the callee's precondition, giving
// `(-5) > 0` -- constant-false, so this call can never be valid.
fn calls_with_a_bad_literal() -> i32 {
    require_positive(-5)
}

// Found only because the `else` arm's hypothesis context carries the
// negated condition: `x <= 0` is known here, and no such `x` satisfies
// `n > 0`.
fn calls_from_the_wrong_branch(x: i32) -> i32 {
    if x > 0 {
        0
    } else {
        require_positive(x)
    }
}

// ── Return-site violations (#42) ──────────────────────────────────────────
//
// The postcondition is coherent on its own -- some integer is greater than
// zero -- so the declaration-site check passes. What fails is the body: with
// `result` bound to what is actually returned, the claim becomes `(-1) > 0`.
//
// Before #42 this reported green, and worse than green: a caller doing
// `let y = always_positive(a)` picked up `y > 0` as a hypothesis and proved
// further obligations from it. An unestablished postcondition is not merely
// bad documentation once something reasons from it.
#[mvl::ensures(result > 0)]
fn always_positive(a: i32) -> i32 {
    let _ = a;
    -1
}

// Each arm a value can leave by is its own return point, so the `else` is
// caught even though the `then` arm is fine.
#[mvl::ensures(result > 0)]
fn positive_on_one_branch_only(x: i32) -> i32 {
    if x > 0 {
        x
    } else {
        0
    }
}

fn main() {
    let bad_literal = calls_with_a_bad_literal();
    let wrong_branch = calls_from_the_wrong_branch(-1);
    let not_positive = always_positive(3);
    let one_branch = positive_on_one_branch_only(-2);

    println!("{}", impossible(7));
    println!("calls_with_a_bad_literal() = {bad_literal}");
    println!("calls_from_the_wrong_branch(-1) = {wrong_branch}");
    println!("always_positive(3) = {not_positive}");
    println!("positive_on_one_branch_only(-2) = {one_branch}");
}
