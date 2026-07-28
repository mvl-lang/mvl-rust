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

fn main() {
    let bad_literal = calls_with_a_bad_literal();
    let wrong_branch = calls_from_the_wrong_branch(-1);

    println!("{}", impossible(7));
    println!("calls_with_a_bad_literal() = {bad_literal}");
    println!("calls_from_the_wrong_branch(-1) = {wrong_branch}");
}
