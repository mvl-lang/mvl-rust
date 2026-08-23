use rust_total::checks::{check_source, check_source_with, CheckSet};

#[test]
fn compliant_total_function_has_no_diagnostics() {
    let source = r#"
        #[mvl::total]
        fn abs(x: i32) -> i32 {
            if x < 0 { -x } else { x }
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics, got {diagnostics:?}"
    );
}

#[test]
fn non_total_functions_are_not_scanned_at_all() {
    // Plenty of panic-risk constructs here, but with no #[mvl::total] this
    // function is entirely out of scope for rust-total.
    let source = r#"
        fn f(v: Vec<i32>, i: usize) -> i32 {
            v[i].checked_add(1).unwrap()
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics for a non-#[total] function, got {diagnostics:?}"
    );
}

// ── termination-trigger (`decreases`) scenarios ─────────────────────────

#[test]
fn wildcard_arm_panic_is_rejected() {
    // rustc itself already rejects a *genuinely* non-exhaustive match (no
    // wildcard, missing variants) as a hard compile error before our tool
    // ever runs -- nothing for rust-total to add there. The gap rustc
    // doesn't cover is a wildcard arm that copies out with a panic, which
    // is syntactically exhaustive but not actually total. That's just a
    // `panic!` sitting inside a match arm, which panic-freedom already
    // catches with no special-casing.
    let source = r#"
        #[mvl::total]
        fn describe(x: Option<i32>) -> i32 {
            match x {
                Some(n) => n,
                None => panic!("unexpected"),
            }
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("panic!"));
}

#[test]
fn terminating_recursion_with_decreases_is_accepted() {
    let source = r#"
        #[mvl::total]
        #[mvl::decreases(n)]
        fn factorial(n: u64) -> u64 {
            if n == 0 { 1 } else { n * factorial(n - 1) }
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics, got {diagnostics:?}"
    );
}

#[test]
fn missing_decreases_on_recursive_total_function_is_rejected() {
    let source = r#"
        #[mvl::total]
        fn factorial(n: u64) -> u64 {
            if n == 0 { 1 } else { n * factorial(n - 1) }
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("factorial"));
    assert!(diagnostics[0].message.contains("decreases"));
}

#[test]
fn non_decreasing_measure_is_rejected() {
    // ADR-0009: presence is no longer enough. `n` is passed unchanged, so
    // the measure never decreases, and the tool must now say so instead of
    // accepting it on presence alone.
    let source = r#"
        #[mvl::total]
        #[mvl::decreases(n)]
        fn loops_forever(n: u64) -> u64 {
            if n == 0 { 0 } else { loops_forever(n) }
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("loops_forever"));
    assert!(diagnostics[0]
        .message
        .contains("does not provably decrease"));
}

#[test]
fn division_is_never_provably_decreasing() {
    // Division/modulo is outside the native solver's linear-arithmetic
    // fragment entirely (ADR-0009): `discharge_entailment` returns
    // `Runtime` for `(n / 2) < n` regardless of hypotheses, confirmed
    // empirically against a `n > 0` hypothesis too. With no runtime
    // enforcement fallback for `decreases`, that's a rejection -- alongside
    // panic_freedom's pre-existing, unrelated division-by-zero diagnostic
    // (spec 003 Requirement 1).
    let source = r#"
        #[mvl::total]
        #[mvl::decreases(n)]
        fn halve(n: u64) -> u64 {
            if n == 0 { 0 } else { halve(n / 2) }
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 2, "got {diagnostics:?}");
    assert!(diagnostics
        .iter()
        .any(|d| d.message.contains("division/modulo")));
    assert!(diagnostics
        .iter()
        .any(|d| d.message.contains("does not provably decrease")));
}

#[test]
fn a_symbolic_decrement_is_proved_given_a_requires_hypothesis() {
    // The solver-backed check generalizes beyond literal constants: given
    // `#[mvl::requires(k > 0)]`, `fuel - k` is proved to decrease `fuel` at
    // L4 (Fourier-Motzkin), even though `k` is not itself a literal.
    let source = r#"
        #[mvl::total]
        #[mvl::decreases(fuel)]
        #[mvl::requires(k > 0)]
        fn countdown(fuel: u64, k: u64) -> u64 {
            if fuel == 0 { 0 } else { countdown(fuel - k, k) }
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics, got {diagnostics:?}"
    );
}

#[test]
fn a_symbolic_decrement_without_a_positivity_hypothesis_is_rejected() {
    // Same shape as the previous test, minus the `requires(k > 0)` -- with
    // no hypothesis bounding `k`, the solver cannot rule out `k <= 0`
    // (which would not decrease `fuel`), so the call is rejected.
    let source = r#"
        #[mvl::total]
        #[mvl::decreases(fuel)]
        fn countdown(fuel: u64, k: u64) -> u64 {
            if fuel == 0 { 0 } else { countdown(fuel - k, k) }
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0]
        .message
        .contains("does not provably decrease"));
}

#[test]
fn non_parameter_measure_is_rejected() {
    // ADR-0009 §1: the measure must be a bare parameter identifier. A
    // computed expression isn't analyzable by a syn-only, no-type-info
    // check, so it's rejected rather than silently accepted.
    let source = r#"
        #[mvl::total]
        #[mvl::decreases(n - 1)]
        fn factorial(n: u64) -> u64 {
            if n == 0 { 1 } else { n * factorial(n - 1) }
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("factorial"));
    assert!(diagnostics[0].message.contains("bare parameter"));
}

#[test]
fn a_shadowed_measure_is_rejected_even_though_it_looks_decreasing() {
    // A real bug found by manual probing, not a hypothetical: before this
    // guard existed, this function was accepted with zero diagnostics.
    // `n` is rebound to `n + 100` before the recursive call, so the `n` in
    // `shadowed(n - 1)` refers to the *shadowed local* (original `n` + 99),
    // not the parameter -- the function never terminates (each call's
    // argument is strictly larger than the last). With no name resolution,
    // the check cannot tell this from an honestly-decreasing `n - 1`, since
    // both build the identical goal `(n - 1) < n` on the bare identifier
    // text. Rejecting any shadow of the measure is the sound response.
    let source = r#"
        #[mvl::total]
        #[mvl::decreases(n)]
        fn shadowed(n: u64) -> u64 {
            let n = n + 100;
            if n == 0 { 0 } else { shadowed(n - 1) }
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1, "got {diagnostics:?}");
    assert!(diagnostics[0].message.contains("shadowed"));
    assert!(diagnostics[0].message.contains("rebound"));
}

#[test]
fn non_recursive_total_function_needs_no_decreases() {
    let source = r#"
        #[mvl::total]
        fn double(n: u64) -> u64 {
            n + n
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics, got {diagnostics:?}"
    );
}

// ── panic-freedom, one construct at a time ──────────────────────────────

#[test]
fn unwrap_is_rejected() {
    let source = r#"
        #[mvl::total]
        fn f(x: Option<i32>) -> i32 {
            x.unwrap()
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("unwrap"));
}

#[test]
fn expect_is_rejected() {
    let source = r#"
        #[mvl::total]
        fn f(x: Option<i32>) -> i32 {
            x.expect("must be present")
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("expect"));
}

#[test]
fn panic_macro_as_a_statement_is_rejected() {
    let source = r#"
        #[mvl::total]
        fn f() -> i32 {
            panic!("nope");
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("panic!"));
}

#[test]
fn todo_and_unimplemented_are_rejected() {
    let source = r#"
        #[mvl::total]
        fn f() -> i32 {
            todo!()
        }
        #[mvl::total]
        fn g() -> i32 {
            unimplemented!()
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 2);
    assert!(diagnostics[0].message.contains("todo!"));
    assert!(diagnostics[1].message.contains("unimplemented!"));
}

#[test]
fn unreachable_is_rejected() {
    let source = r#"
        #[mvl::total]
        fn f() -> i32 {
            unreachable!()
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("unreachable!"));
}

#[test]
fn raw_indexing_is_rejected() {
    let source = r#"
        #[mvl::total]
        fn f(v: Vec<i32>, i: usize) -> i32 {
            v[i]
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("indexing"));
}

#[test]
fn division_and_modulo_are_rejected() {
    let source = r#"
        #[mvl::total]
        fn f(a: i32, b: i32) -> i32 {
            (a / b) + (a % b)
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 2);
    assert!(diagnostics
        .iter()
        .all(|d| d.message.contains("division/modulo")));
}

#[test]
fn malformed_source_returns_parse_error() {
    let result = check_source("fn f( {{{");
    assert!(result.is_err());
}

#[test]
fn binary_arithmetic_in_a_total_function_is_accepted() {
    // Spec 003 Requirement 2: `+`/`-`/`*` are deliberately NOT flagged for
    // overflow. Without type information, flagging them would flag nearly all
    // numeric code, so the rule is omitted on false-positive grounds rather
    // than because the risk is absent. The existing compliant fixture only
    // exercises unary negation, so it does not evidence this.
    let source = r#"
        #[mvl::total]
        fn combine(a: i32, b: i32, c: i32) -> i32 {
            a + b * c - a
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        diagnostics.is_empty(),
        "binary arithmetic must not be flagged, got {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn requires_and_ensures_on_a_total_function_are_not_flagged() {
    // Since #53 these attributes expand to a real `assert!`, which panics on
    // failure -- but this checker scans the author's source, never the
    // macro-expanded body, so there is no `assert!` token here to flag even
    // in principle. The decision (ADR-0003 §2, "total on its promised
    // domain"): a contract assert firing means a caller broke that domain,
    // which is outside what `#[mvl::total]` promises, not a counterexample
    // to it. This pins the silence as deliberate -- a regression here would
    // most likely arrive as someone adding `assert` to `PANICKING_MACROS`
    // without registering that distinction.
    let source = r#"
        #[mvl::total]
        #[mvl::requires(x > 0)]
        #[mvl::ensures(result > 0)]
        fn f(x: i32) -> i32 {
            x
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        diagnostics.is_empty(),
        "a contract assert must not be treated as violating #[mvl::total]'s panic-freedom promise, got {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ── loop-termination (`loop_decreases!`) scenarios (spec 003 Requirement 6, ADR-0010) ──

#[test]
fn loop_missing_decreases_marker_is_rejected() {
    // Spec 003 Requirement 6, scenario "A loop with no loop_decreases!
    // marker is rejected". Confirmed as a real, previously-undocumented
    // gap before this check existed: this compiled and ran forever with
    // zero diagnostics from cargo-mvl-total.
    let source = r#"
        #[mvl::total]
        fn spins_forever() -> u64 {
            let mut n = 0;
            loop {
                n += 1;
            }
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1, "got {diagnostics:?}");
    assert!(diagnostics[0].message.contains("loop_decreases"));
}

#[test]
fn loop_non_identifier_measure_is_rejected() {
    // Spec 003 Requirement 6, scenario "A loop measure that isn't a bare
    // identifier is rejected".
    let source = r#"
        #[mvl::total]
        fn f(mut n: u64) -> u64 {
            while n > 0 {
                mvl::loop_decreases!(n - 1);
                n -= 1;
            }
            n
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1, "got {diagnostics:?}");
    assert!(diagnostics[0].message.contains("bare local variable"));
}

#[test]
fn loop_with_literal_decrement_is_accepted() {
    // Spec 003 Requirement 6, scenario "A loop whose measure provably
    // decreases is accepted" -- the literal case, mirroring
    // `terminating_recursion_with_decreases_is_accepted`.
    let source = r#"
        #[mvl::total]
        fn countdown(mut n: u64) -> u64 {
            while n > 0 {
                mvl::loop_decreases!(n);
                n -= 1;
            }
            n
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics, got {diagnostics:?}"
    );
}

#[test]
fn loop_with_symbolic_decrement_is_proved_given_a_requires_hypothesis() {
    // Spec 003 Requirement 6 -- the symbolic case, mirroring
    // `a_symbolic_decrement_is_proved_given_a_requires_hypothesis`. `k` is
    // not a literal; only provable because `#[mvl::requires(k > 0)]`
    // supplies the hypothesis the solver needs.
    let source = r#"
        #[mvl::total]
        #[mvl::requires(k > 0)]
        fn countdown_by(mut fuel: u64, k: u64) -> u64 {
            while fuel > 0 {
                mvl::loop_decreases!(fuel);
                fuel -= k;
            }
            fuel
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics, got {diagnostics:?}"
    );
}

#[test]
fn loop_symbolic_decrement_without_a_positivity_hypothesis_is_rejected() {
    // Same shape as above, minus the `requires(k > 0)` -- with no
    // hypothesis bounding `k`, the solver cannot rule out `k == 0` (which
    // would not decrease `fuel`), so the loop is rejected.
    let source = r#"
        #[mvl::total]
        fn countdown_by(mut fuel: u64, k: u64) -> u64 {
            while fuel > 0 {
                mvl::loop_decreases!(fuel);
                fuel -= k;
            }
            fuel
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1, "got {diagnostics:?}");
    assert!(diagnostics[0]
        .message
        .contains("does not provably decrease"));
}

#[test]
fn loop_division_is_never_provably_decreasing() {
    // Spec 003 Requirement 6, scenario "A loop whose measure does not
    // provably decrease is rejected" -- division. Mirrors
    // `division_is_never_provably_decreasing`: genuinely terminates at
    // runtime (integer division reaches 0), but the native solver's
    // linear-arithmetic system cannot represent division at all, so it's
    // never provable regardless of hypotheses.
    let source = r#"
        #[mvl::total]
        fn halve(mut n: u64) -> u64 {
            while n > 0 {
                mvl::loop_decreases!(n);
                n /= 2;
            }
            n
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1, "got {diagnostics:?}");
    assert!(diagnostics[0]
        .message
        .contains("does not provably decrease"));
}

#[test]
fn loop_conditional_mutation_is_rejected() {
    // Spec 003 Requirement 6, scenario "A conditional or duplicated
    // assignment to the measure is rejected" -- the conditional case. The
    // only assignment to `n` is nested inside an `if`, so it doesn't run
    // every iteration -- not a sound per-iteration decrease even though
    // its shape (`n -= 1`) would otherwise qualify.
    let source = r#"
        #[mvl::total]
        fn maybe_decrement(mut n: u64, flag: bool) -> u64 {
            while n > 0 {
                mvl::loop_decreases!(n);
                if flag {
                    n -= 1;
                }
            }
            n
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1, "got {diagnostics:?}");
    assert!(diagnostics[0].message.contains("conditional"));
}

#[test]
fn loop_multiple_mutations_are_rejected() {
    // Spec 003 Requirement 6, scenario "A conditional or duplicated
    // assignment to the measure is rejected" -- the duplicate case.
    let source = r#"
        #[mvl::total]
        fn double_decrement(mut n: u64) -> u64 {
            while n > 0 {
                mvl::loop_decreases!(n);
                n -= 1;
                n -= 1;
            }
            n
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1, "got {diagnostics:?}");
    assert!(diagnostics[0].message.contains("more than once"));
}

#[test]
fn loop_shadowed_measure_is_rejected() {
    // Spec 003 Requirement 6, scenario "A measure shadowed in the loop
    // body is rejected". Same class of bug as
    // `a_shadowed_measure_is_rejected_even_though_it_looks_decreasing`:
    // the `n -= 1` inside the shadowing block refers to the fresh local,
    // never the outer loop variable, so the outer `n > 0` condition never
    // changes and the loop never terminates.
    let source = r#"
        #[mvl::total]
        fn shadowed(mut n: u64) -> u64 {
            while n > 0 {
                mvl::loop_decreases!(n);
                let mut n = n + 100;
                n -= 1;
            }
            n
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1, "got {diagnostics:?}");
    assert!(diagnostics[0].message.contains("rebound"));
}

#[test]
fn nested_loops_are_each_checked_independently() {
    // Spec 003 Requirement 6 -- ADR-0010 §5: each loop needs its own
    // marker and is checked on its own. Both are honestly decreasing
    // here, so both are accepted.
    let source = r#"
        #[mvl::total]
        fn nested(mut n: u64, mut m: u64) -> u64 {
            while n > 0 {
                mvl::loop_decreases!(n);
                while m > 0 {
                    mvl::loop_decreases!(m);
                    m -= 1;
                }
                n -= 1;
            }
            n
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics, got {diagnostics:?}"
    );
}

// ── impl-method scope (#89) ─────────────────────────────────────────────

#[test]
fn a_total_impl_method_is_checked_like_a_free_function() {
    let source = r#"
        struct Calc;
        impl Calc {
            #[mvl::total]
            fn abs(x: i32) -> i32 {
                if x < 0 { -x } else { x }
            }
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics for a compliant impl method, got {diagnostics:?}"
    );
}

#[test]
fn a_panicking_total_impl_method_is_flagged() {
    let source = r#"
        struct Calc;
        impl Calc {
            #[mvl::total]
            fn first(v: Vec<i32>) -> i32 {
                v[0]
            }
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        !diagnostics.is_empty(),
        "expected the raw index to be flagged inside an impl method"
    );
}

#[test]
fn a_non_total_impl_method_is_not_scanned() {
    let source = r#"
        struct Calc;
        impl Calc {
            fn first(v: Vec<i32>) -> i32 {
                v[0]
            }
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics for a non-#[total] impl method, got {diagnostics:?}"
    );
}

#[test]
fn let_underscore_call_is_rejected() {
    let source = r#"
        #[mvl::total]
        fn f() {
            let _ = write_config();
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("silently discards"));
}

#[test]
fn let_underscore_bare_variable_is_not_rejected() {
    // Not a call -- discarding an already-bound value isn't swallowing a
    // fallible result, it's just an unused-binding pattern.
    let source = r#"
        #[mvl::total]
        fn f(x: i32) {
            let _ = x;
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics for `let _ = <bare ident>`, got {diagnostics:?}"
    );
}

#[test]
fn drop_of_a_call_result_is_rejected() {
    let source = r#"
        #[mvl::total]
        fn f() {
            drop(write_config());
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("silently discards"));
}

#[test]
fn mem_drop_of_a_call_result_is_rejected() {
    let source = r#"
        #[mvl::total]
        fn f() {
            std::mem::drop(write_config());
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("silently discards"));
}

#[test]
fn map_discarding_closure_is_rejected() {
    let source = r#"
        #[mvl::total]
        fn f(r: Result<i32, String>) {
            r.map(|_| ());
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0]
        .message
        .contains("discards the wrapped value"));
}

#[test]
fn map_with_real_transform_is_not_rejected() {
    let source = r#"
        #[mvl::total]
        fn f(r: Result<i32, String>) -> Result<i32, String> {
            r.map(|x| x + 1)
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics for a real `.map` transform, got {diagnostics:?}"
    );
}

#[test]
fn swallow_check_does_not_scan_non_total_functions() {
    let source = r#"
        fn f() {
            let _ = write_config();
            drop(write_config());
        }
    "#;
    let diagnostics = check_source(source).unwrap();
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics for a non-#[total] function, got {diagnostics:?}"
    );
}

#[test]
fn check_set_parse_accepts_a_single_name() {
    let set = CheckSet::parse("panic").unwrap();
    assert!(set.panic);
    assert!(!set.termination);
    assert!(!set.swallow);
}

#[test]
fn check_set_parse_accepts_a_comma_separated_subset() {
    let set = CheckSet::parse("termination,swallow").unwrap();
    assert!(!set.panic);
    assert!(set.termination);
    assert!(set.swallow);
}

#[test]
fn check_set_parse_rejects_an_unknown_name() {
    assert!(CheckSet::parse("pnaic").is_err());
}

#[test]
fn check_source_with_panic_only_skips_swallow_violations() {
    let source = r#"
        #[mvl::total]
        fn f() {
            let _ = write_config();
            some_call().unwrap();
        }
    "#;
    let diagnostics = check_source_with(source, CheckSet::parse("panic").unwrap()).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("unwrap"));
}

#[test]
fn check_source_with_swallow_only_skips_panic_violations() {
    let source = r#"
        #[mvl::total]
        fn f() {
            let _ = write_config();
            some_call().unwrap();
        }
    "#;
    let diagnostics = check_source_with(source, CheckSet::parse("swallow").unwrap()).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("silently discards"));
}

#[test]
fn check_source_with_all_matches_check_source() {
    let source = r#"
        #[mvl::total]
        fn f() {
            let _ = write_config();
            some_call().unwrap();
        }
    "#;
    let via_default = check_source(source).unwrap();
    let via_explicit_all = check_source_with(source, CheckSet::ALL).unwrap();
    assert_eq!(via_default.len(), via_explicit_all.len());
    assert_eq!(via_default.len(), 2);
}
