//! End-to-end enforcement: real code, real `#[mvl::requires]`/`#[mvl::ensures]`
//! expansion, real panics. `crates/mvl-macros/src/inject.rs`'s unit tests
//! check the generated tokens; this file checks the *compiled, executed*
//! behavior through the actual facade a user depends on -- the property
//! spec 007 Requirements 3-5 are written against.
//!
//! **Requirement 3's "not elided in release" scenario is not re-tested
//! here.** A real release-mode check would mean spawning `cargo build
//! --release` and running the binary, which belongs with `make examples`'s
//! style of check, not a unit test. The actual guarantee is structural:
//! `inject.rs` emits a bare `assert!` with no `cfg(debug_assertions)` gate
//! anywhere in it, which `ensures_uses_assert_not_debug_assert` /
//! `requires_uses_assert_not_debug_assert` (`mvl-macros`) pin directly. A
//! profile-conditional check would show up there as a change to what those
//! tests assert, not as a passing test in a different profile.

#[mvl::requires(x > 0)]
fn needs_positive(x: i64) -> i64 {
    x * 2
}

#[test]
fn a_satisfied_precondition_runs_normally() {
    assert_eq!(needs_positive(5), 10);
}

#[test]
#[should_panic(expected = "`#[mvl::requires]` violated: x > 0")]
fn a_violated_precondition_aborts() {
    needs_positive(-1);
}

#[mvl::ensures(result > 100)]
fn via_explicit_return(x: i64) -> i64 {
    // Requirement 3's headline scenario: upstream's Rust backend checks
    // only the implicit tail and misses this path entirely (verified there
    // directly -- a function shaped like this returned an out-of-contract
    // value with no diagnostic and no abort).
    if x < 10 {
        return x;
    }
    x * 1000
}

#[test]
fn a_satisfying_tail_runs_normally() {
    assert_eq!(via_explicit_return(50), 50000);
}

#[test]
#[should_panic(expected = "`#[mvl::ensures]` violated: result > 100")]
fn a_violating_explicit_return_aborts() {
    via_explicit_return(3);
}

#[mvl::ensures(result.is_ok())]
fn via_try_operator(x: i64) -> Result<i64, ()> {
    // The `?` early return has no `Expr::Return` node -- only `Expr::Try` --
    // so `ReturnRewriter` never sees it (ADR-0006 Section 4 amendment,
    // spec 007 Known Limitations). This test pins that gap: were this path
    // instrumented, returning `Err(())` here would violate `result.is_ok()`
    // and abort; instead the assertion never runs and the `Err` propagates
    // silently. If a future change closes the gap, this test should be
    // updated deliberately rather than the gap drifting further unnoticed.
    let value: Result<i64, ()> = Err(());
    let _ = value?;
    Ok(x * 1000)
}

#[test]
fn a_violating_early_return_via_try_operator_does_not_abort() {
    assert_eq!(via_try_operator(3), Err(()));
}

#[mvl::ensures(forall i in [0..3] . result > i)]
fn quantified_ok() -> i64 {
    10
}

#[mvl::ensures(forall i in [0..3] . result > i)]
fn quantified_bad() -> i64 {
    2 // fails at i = 2: 2 > 2 is false
}

#[test]
fn quantified_postcondition_is_checked_at_runtime() {
    // Named to match spec 007 Requirement 4's scenario link.
    assert_eq!(quantified_ok(), 10);
}

#[test]
#[should_panic(expected = "`#[mvl::ensures]` violated: forall i in [0..3] . result > i")]
fn quantified_postcondition_catches_a_single_bad_value() {
    quantified_bad();
}

// ── Requirement 5: the opt-out, and the `#[mvl::total]` collision it resolves ──

#[mvl::total]
#[mvl::requires(0 <= b && b <= 255)]
#[mvl::ensures(0 <= result && result <= 15)]
fn mask_low_nibble(b: i64) -> i64 {
    // The compliant demo's own function (#53's motivating case): `total`
    // means total *on its promised domain* (ADR-0003 §2), so a contract
    // assert enforcing that domain does not conflict with `total`'s
    // panic-freedom claim -- it is what makes the claim meaningful.
    b & 15
}

#[test]
fn a_total_function_with_a_satisfied_contract_runs_normally() {
    assert_eq!(mask_low_nibble(200), 8);
}

#[test]
#[should_panic(expected = "`#[mvl::requires]` violated")]
fn a_total_function_still_enforces_its_contract() {
    mask_low_nibble(-1);
}

#[mvl::unchecked]
#[mvl::ensures(result > 100)]
fn opted_out_above(x: i64) -> i64 {
    x
}

#[mvl::ensures(result > 100)]
#[mvl::unchecked]
fn opted_out_below(x: i64) -> i64 {
    x
}

#[test]
fn unchecked_suppresses_enforcement_regardless_of_attribute_order() {
    // Both orders must work -- attribute macros expand outside-in, so
    // `#[mvl::unchecked]` needs a mechanism for each position (see its doc
    // comment in `mvl-macros`). A working opt-out in only one order is worse
    // than no opt-out: the author believes it works and it silently doesn't
    // in the other position.
    assert_eq!(opted_out_above(3), 3);
    assert_eq!(opted_out_below(3), 3);
}
