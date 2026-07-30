//! Since #53, `#[mvl::requires]`/`#[mvl::ensures]` are **not** pass-throughs
//! — they expand to a real `assert!` (`crates/mvl/tests/enforcement.rs`
//! covers them). Everything in this file is what remains genuinely inert:
//! `total`, `effect`, `label`, `relabel` still discard their argument tokens
//! and leave the annotated item unchanged, exactly as before.

#[mvl::total]
fn total_fn() -> i32 {
    1
}

#[mvl::effect(Console)]
fn effect_fn() {}

#[mvl::label]
struct SmokeTestLabel;

#[mvl::relabel(from = "_", to = "SmokeTestLabel")]
fn smoke_test_ingest(x: i32) -> mvl::Labeled<SmokeTestLabel, i32> {
    mvl::Labeled::new(x)
}

#[test]
fn total_effect_label_and_relabel_are_still_pass_through() {
    assert_eq!(total_fn(), 1);
    effect_fn();
    assert_eq!(smoke_test_ingest(5).into_inner(), 5);
}

// Recursive termination proof: `n` is the decreases-measure, strictly
// decreasing on each call and bounded below by the `n == 0` base case.
#[mvl::total]
#[mvl::decreases(n)]
fn factorial(n: u64) -> u64 {
    if n == 0 {
        1
    } else {
        n * factorial(n - 1)
    }
}

#[test]
fn decreases_example_is_a_real_recursive_function() {
    assert_eq!(factorial(0), 1);
    assert_eq!(factorial(5), 120);
}

// ── IFC: built-in `Tainted` label, matching examples/log_to_file/paths.mvl ──
//
//   pub total fn resolve_path(raw: Tainted[String]) -> Result[String, LogSetupError] {
//       let s: String = relabel trust(raw, "LOG-PATH-001");
//       ...
//   }

#[test]
fn tainted_string_requires_explicit_trust_to_declassify() {
    let raw: mvl::Tainted<String> = mvl::Tainted::new("from the environment".to_string());
    let trusted: String = mvl::trust(raw, "LOG-PATH-001");
    assert_eq!(trusted, "from the environment");
}

// ── IFC: user-defined label, matching examples/hipaa_healthcare/ifc.mvl ────
//
//   pub label PHI
//   pub relabel hipaa_release: PHI -> _ audit
//   pub relabel ingest_phi: _ -> PHI

#[mvl::label]
struct Phi;

/// Mark raw data as Protected Health Information at ingestion boundary.
#[mvl::relabel(from = "_", to = "Phi")]
fn ingest_phi<T>(x: T) -> mvl::Labeled<Phi, T> {
    mvl::Labeled::new(x)
}

/// Release PHI data for an authorized purpose. `audit_tag` records why.
#[mvl::relabel(from = "Phi", to = "_", audit)]
fn hipaa_release<T>(x: mvl::Labeled<Phi, T>, _audit_tag: &'static str) -> T {
    x.into_inner()
}

#[test]
fn custom_phi_label_round_trips_through_ingest_and_release() {
    let record: mvl::Labeled<Phi, String> = ingest_phi("patient: Jane Doe, dx: ...".to_string());
    let released: String = hipaa_release(record, "REFERRAL-2026-0417");
    assert_eq!(released, "patient: Jane Doe, dx: ...");
}
