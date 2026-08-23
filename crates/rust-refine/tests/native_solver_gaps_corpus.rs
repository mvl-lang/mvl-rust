//! Acceptance corpus for the two native-solver gaps found in the
//! `sqlite-rs` spike (#371) and its write-up
//! (`rust-refine-native-solver-gaps.md`): #94 (an unsigned parameter's
//! implicit `>= 0` bound never reached Γ) and #95 (`self.field`/
//! `param.field` was never bindable as a solver variable at all).
//!
//! Each `#[test]` here is a standalone scenario, grouped by which issue it
//! demonstrates and whether it is **in scope** (closes at L2/L4 where it
//! used to fall to `runtime`) or **explicitly out of scope** (a documented
//! boundary, asserted so a future change can't silently widen or narrow it
//! without a test noticing). Nothing here is a mechanism-level regression
//! pin — those live in `mvl-rust-core`'s own `solver::native` unit tests
//! and in `call_sites.rs`'s Γ-construction section; this file is the
//! narrative, close-to-the-issue-text version of the same two fixes.

use mvl_rust_core::solver::{DischargeResult, Layer};
use rust_refine::checks::{find_obligations, ObligationKind};

fn only_call_site(source: &str) -> DischargeResult {
    let sites: Vec<_> = find_obligations(source)
        .expect("fixture parses")
        .iter()
        .filter(|f| matches!(f.kind, ObligationKind::CallSite { .. }))
        .map(|f| f.discharge())
        .collect();
    assert_eq!(
        sites.len(),
        1,
        "expected exactly one call-site obligation, got {sites:?}"
    );
    sites.into_iter().next().unwrap()
}

fn only_return_site(source: &str) -> DischargeResult {
    let sites: Vec<_> = find_obligations(source)
        .expect("fixture parses")
        .iter()
        .filter(|f| f.kind == ObligationKind::ReturnSite)
        .map(|f| f.discharge())
        .collect();
    assert_eq!(
        sites.len(),
        1,
        "expected exactly one return-site obligation, got {sites:?}"
    );
    sites.into_iter().next().unwrap()
}

fn assert_proven_at(result: &DischargeResult, expected: Layer) {
    match result {
        DischargeResult::Proven { layer } => assert_eq!(
            *layer, expected,
            "proven, but at {layer:?} rather than {expected:?}"
        ),
        other => panic!("expected Proven at {expected:?}, got {other:?}"),
    }
}

fn assert_runtime(result: &DischargeResult) {
    assert_eq!(
        *result,
        DischargeResult::Runtime,
        "expected this to stay unproven (a documented scope boundary)"
    );
}

// ── #94: an unsigned parameter's implicit `>= 0` bound ─────────────────────
//
// A `u8`/`u16`/`u32`/`u64`/`u128`/`usize` parameter carries `>= 0` for free
// from its type. Before #94, that fact never reached Γ, so a predicate
// provable only via that bound fell to `runtime` even though it is pure,
// closed linear arithmetic.

mod unsigned_lower_bound {
    use super::*;

    /// The issue's own motivating example, verbatim: `page_size - reserved_space`
    /// is provably `<= page_size` given `reserved_space <= page_size` *and*
    /// `reserved_space >= 0` -- and the second half used to require writing
    /// it out by hand.
    #[test]
    fn in_scope_usable_page_size_now_closes_without_writing_the_bound_by_hand() {
        let result = only_call_site(
            "#[mvl::requires(reserved_space <= page_size && reserved_space >= 0)]\n\
             #[mvl::ensures(result <= page_size)]\n\
             fn usable_page_size(page_size: u32, reserved_space: u32) -> u32 {\n\
               page_size - reserved_space\n\
             }\n\
             #[mvl::requires(reserved_space <= page_size)]\n\
             fn caller(page_size: u32, reserved_space: u32) -> u32 {\n\
               usable_page_size(page_size, reserved_space)\n\
             }",
        );
        // The caller states only `reserved_space <= page_size`; #94 supplies
        // `page_size >= 0`/`reserved_space >= 0` for both `u32` params for
        // free, closing what would otherwise need an explicit restatement.
        assert_proven_at(&result, Layer::L4);
    }

    /// Every `unsigned_param_name`-recognized width gets the same treatment,
    /// not just `u32` -- each one alone is enough for the callee's own
    /// `n >= 0` to close via #94, with no cast and no caller-side `requires`
    /// at all.
    #[test]
    fn in_scope_every_unsigned_width_gets_the_bound() {
        for ty in ["u8", "u16", "u32", "u64", "u128", "usize"] {
            let result = only_call_site(&format!(
                "#[mvl::requires(n >= 0)]\n\
                 fn require_non_negative(n: {ty}) -> {ty} {{ n }}\n\
                 fn caller(x: {ty}) -> {ty} {{ require_non_negative(x) }}"
            ));
            assert_proven_at(&result, Layer::L2);
        }
    }

    /// Adjacent, but not a #94 regression: a cast argument (`x as i64`) is
    /// outside `linterm_from_expr`'s linear fragment regardless of the
    /// parameter's own type, so this still falls to `runtime`.
    #[test]
    fn out_of_scope_a_cast_argument_stays_runtime_regardless_of_width() {
        let result = only_call_site(
            "#[mvl::requires(n >= 0)]\n\
             fn require_non_negative(n: i64) -> i64 { n }\n\
             fn caller(x: u8) -> i64 { require_non_negative(x as i64) }",
        );
        assert_runtime(&result);
    }

    /// #94 composes with an explicit `requires`, rather than only firing
    /// when nothing else is written -- neither fact alone proves the goal.
    #[test]
    fn in_scope_composes_with_an_explicit_requires() {
        let result = only_call_site(
            "#[mvl::requires(n >= 0 && n <= 100)]\n\
             fn require_bounded(n: i32) -> i32 { n }\n\
             #[mvl::requires(x <= 100)]\n\
             fn caller(x: u32) -> i32 { require_bounded(x) }",
        );
        assert_proven_at(&result, Layer::L2);
    }

    /// Out of scope, by design: a *signed* parameter (`i32`) gets no
    /// implicit bound at all -- the injection is gated strictly on the
    /// unsigned type list, not on "looks non-negative in context".
    #[test]
    fn out_of_scope_a_signed_parameter_gets_nothing() {
        let result = only_call_site(
            "#[mvl::requires(n >= 0)]\n\
             fn require_non_negative(n: i32) -> i32 { n }\n\
             fn caller(x: i32) -> i32 { require_non_negative(x) }",
        );
        assert_runtime(&result);
    }

    /// Out of scope, by design (explicitly deferred to #95): `self`'s own
    /// unsigned fields get no implicit bound from #94 alone -- #94 only
    /// reads a function's own parameter list (`sig.inputs`), never a
    /// struct definition. This composed case needs both #94's *kind* of
    /// fix and #95's *kind* of fix to work together (tracked as a further
    /// follow-up in #95's own notes, not delivered by either alone).
    #[test]
    fn out_of_scope_self_field_types_are_not_consulted() {
        let result = only_return_site(
            "struct Page { page_size: u32, reserved_space: u32 }\n\
             impl Page {\n\
             #[mvl::requires(self.reserved_space <= self.page_size)]\n\
             #[mvl::ensures(result <= self.page_size)]\n\
             pub fn usable_page_size(&self) -> u32 {\n\
               self.page_size - self.reserved_space\n\
             }\n\
             }",
        );
        assert_runtime(&result);
    }
}

// ── #95: field projections (`self.field`) as solver variables ─────────────
//
// `self.field`/`param.field` was never recognized as a bindable variable by
// either L2 (`ident_name`) or L4 (`linterm_from_expr`) -- regardless of how
// good the surrounding hypothesis context was. #94's fix alone does not
// touch this: the case above (`out_of_scope_self_field_types_are_not_consulted`)
// stays `runtime` even with #94 landed, because the blocker there is
// binding, not bounds.

mod field_projection_variables {
    use super::*;

    /// The issue's own motivating example, verbatim: identical arithmetic
    /// to #94's free-function version, with `self.field` standing in for a
    /// bare parameter, and the bounds spelled out explicitly (the part #94
    /// alone can't supply for `self`, per the boundary case above).
    #[test]
    fn in_scope_usable_page_size_as_a_method_now_closes_too() {
        let result = only_return_site(
            "struct Page { page_size: i32, reserved_space: i32 }\n\
             impl Page {\n\
             #[mvl::requires(self.reserved_space <= self.page_size && self.reserved_space >= 0 && self.page_size >= 0)]\n\
             #[mvl::ensures(result <= self.page_size)]\n\
             pub fn usable_page_size(&self) -> i32 {\n\
               self.page_size - self.reserved_space\n\
             }\n\
             }",
        );
        assert_proven_at(&result, Layer::L4);
    }

    /// A plain parameter's field projection (`param.field`), not just
    /// `self.field` -- the fix is keyed on "bare-path receiver", not on the
    /// receiver specifically being `self`.
    #[test]
    fn in_scope_a_non_self_receivers_field_works_too() {
        let result = only_call_site(
            "struct Page { size: i32 }\n\
             #[mvl::requires(n >= 0)]\n\
             fn require_non_negative(n: i32) -> i32 { n }\n\
             #[mvl::requires(p.size >= 0)]\n\
             fn caller(p: Page) -> i32 { require_non_negative(p.size) }",
        );
        assert_proven_at(&result, Layer::L2);
    }

    /// #95's own contradiction case: the same field occurring twice must be
    /// keyed identically, or the obligation looks like two unrelated opaque
    /// terms instead of a provable contradiction.
    #[test]
    fn in_scope_repeated_field_occurrences_are_recognized_as_the_same_variable() {
        let result = only_return_site(
            "struct Counter { value: i32 }\n\
             impl Counter {\n\
             #[mvl::requires(self.value == 5)]\n\
             #[mvl::ensures(result == 6)]\n\
             pub fn wrong(&self) -> i32 {\n\
               self.value\n\
             }\n\
             }",
        );
        assert!(
            matches!(result, DischargeResult::Violated { .. }),
            "self.value == 5 must be seen to contradict a claimed result == 6, got {result:?}"
        );
    }

    /// Out of scope, by design: a two-level field chain (`self.a.b`). The
    /// fix recurses exactly one level into a bare-path receiver; `self.a`
    /// is itself a field projection, not a bare path, so the outer
    /// projection is not recognized.
    #[test]
    fn out_of_scope_a_two_level_field_chain_still_falls_to_runtime() {
        let result = only_return_site(
            "struct Inner { size: i32 }\n\
             struct Outer { inner: Inner }\n\
             impl Outer {\n\
             #[mvl::requires(self.inner.size >= 0)]\n\
             #[mvl::ensures(result >= 0)]\n\
             pub fn size(&self) -> i32 {\n\
               self.inner.size\n\
             }\n\
             }",
        );
        assert_runtime(&result);
    }

    /// Out of scope, by design: an indexed receiver (`xs[i].field`). The
    /// receiver must be a bare path, and an index expression could alias in
    /// ways this purely syntactic pass has no way to reason about.
    #[test]
    fn out_of_scope_an_indexed_receiver_still_falls_to_runtime() {
        let result = only_call_site(
            "#[mvl::requires(n >= 0)]\n\
             fn require_non_negative(n: i32) -> i32 { n }\n\
             struct Item { field: i32 }\n\
             #[mvl::requires(xs[i].field >= 0)]\n\
             fn caller(xs: [Item; 4], i: usize) -> i32 { require_non_negative(xs[i].field) }",
        );
        assert_runtime(&result);
    }
}
