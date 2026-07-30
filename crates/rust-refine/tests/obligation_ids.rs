//! Obligation ids are addresses, and an address has to be unique (#51).
//!
//! `AssuranceLeaf.obligation_id` is how a certification reviewer walks a
//! report's leaf back to the obligation it claims to discharge. Before #51
//! the id was `{fn}::{kind}` with no discriminator, so a function's two
//! `requires` clauses -- or its two calls to one callee, or its two return
//! points -- produced the same id, and the walk-back was ambiguous exactly
//! where an evidence trail must not be.
//!
//! Nothing keyed on the id when it was introduced, which is why #46 left it.
//! These tests exist so it cannot silently regress now that something does.

use std::collections::HashSet;

use rust_refine::checks::find_obligations;

/// Every obligation id in `source`, in visit order.
fn ids(source: &str) -> Vec<String> {
    find_obligations(source)
        .expect("fixture parses")
        .iter()
        .map(|found| found.id())
        .collect()
}

/// The three colliding shapes from #51's report, in one function: two
/// `requires` clauses, two calls to the same callee, and two return points.
const ALL_THREE_COLLISIONS: &str = "\
#[mvl::requires(x > 0)]
#[mvl::requires(x < 100)]
#[mvl::ensures(result > 0)]
fn caller(x: i32, flag: bool) -> i32 {
    need_pos(x);
    need_pos(x);
    if flag {
        return x;
    }
    x + 1
}

#[mvl::requires(n > 0)]
fn need_pos(n: i32) -> i32 { n }
";

#[test]
fn every_obligation_in_a_file_has_a_distinct_id() {
    let ids = ids(ALL_THREE_COLLISIONS);
    let distinct: HashSet<&String> = ids.iter().collect();
    assert_eq!(
        distinct.len(),
        ids.len(),
        "ids must be unique, got {ids:#?}"
    );
}

#[test]
fn each_colliding_shape_is_numbered_independently() {
    // Per-`(function, stem)` counters: the two `requires` number 0 and 1
    // without being perturbed by the two calls, which also number 0 and 1.
    // A single global counter would pass the uniqueness test above while
    // making every id depend on unrelated obligations earlier in the file.
    let ids = ids(ALL_THREE_COLLISIONS);
    assert_eq!(
        ids,
        vec![
            "caller::requires#0",
            "caller::requires#1",
            "caller::ensures#0",
            "need_pos::requires#0",
            "caller::calls::need_pos::requires#0",
            "caller::calls::need_pos::requires#1",
            "caller::returns::ensures#0",
            "caller::returns::ensures#1",
        ]
    );
}

#[test]
fn the_occurrence_suffix_is_present_on_a_lone_obligation() {
    // Uniformly suffixed, including `#0`. Omitting it for the
    // single-occurrence case would read as a distinction that isn't there:
    // `f::requires` beside `f::requires#1` gives a reader no way to tell
    // "the only one" from "the first of several".
    assert_eq!(
        ids("#[mvl::requires(x > 0)]\nfn f(x: i32) {}"),
        vec!["f::requires#0"]
    );
}

#[test]
fn numbering_is_scoped_to_the_function_not_the_file() {
    // Two functions each with one `requires` both get `#0` -- the counter is
    // per-function, so adding a function above does not renumber the ones
    // below it. That stability is what lets the id serve as a cache key for
    // ADR-0006 4's injection design.
    assert_eq!(
        ids("#[mvl::requires(x > 0)]\nfn f(x: i32) {}\n#[mvl::requires(y > 0)]\nfn g(y: i32) {}"),
        vec!["f::requires#0", "g::requires#0"]
    );
}

#[test]
fn an_edit_elsewhere_in_the_file_does_not_renumber_an_obligation() {
    // The reason #51 chose an occurrence index over a span. A span-derived
    // discriminator changes on any edit above it, which would invalidate a
    // discharge cache for every obligation below an inserted line.
    let before =
        ids("#[mvl::requires(x > 0)]\nfn f(x: i32) {}\n#[mvl::requires(y > 0)]\nfn g(y: i32) {}");
    let after = ids("#[mvl::requires(x > 0)]\nfn f(x: i32) {}\n\
         fn inserted() {}\n\
         #[mvl::requires(y > 0)]\nfn g(y: i32) {}");
    assert!(after.contains(&"g::requires#0".to_string()));
    assert_eq!(before, after);
}
