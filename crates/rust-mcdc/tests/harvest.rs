//! Integration test for [`rust_mcdc::harvest::harvest`] against a
//! throwaway crate built in the OS temp dir -- exercises the real
//! scan → tag → `cargo test` → join cycle end to end, no mutation.

use std::fs;
use std::path::PathBuf;

use rust_mcdc::harvest::harvest;
use rust_mcdc::obligation::obligation_id;
use rust_mcdc::scanner::{scan_source, to_records};

fn temp_crate_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rust-mcdc-harvest-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

const LIB: &str = r#"pub fn decide(a: bool, b: bool) -> bool {
    if a || b { true } else { false }
}
"#;

/// The decision text `scan_source(LIB)` extracts, and the ids it yields
/// per file -- `mcdc__<id>__v<N>` tags below are built from these so the
/// fixtures track the id scheme (#121).
const DECISION: &str = "a || b";
const LIB_ID: &str = "lib_decide_503cd493";
const VM_ID: &str = "vm_batch_decide_503cd493";
const CODEGEN_ID: &str = "codegen_batch_decide_503cd493";

#[test]
fn fixture_ids_match_the_id_scheme() {
    assert_eq!(
        obligation_id("src/lib.rs", Some("decide"), DECISION),
        LIB_ID
    );
    assert_eq!(
        obligation_id("src/vm/batch.rs", Some("decide"), DECISION),
        VM_ID
    );
    assert_eq!(
        obligation_id("src/codegen/batch.rs", Some("decide"), DECISION),
        CODEGEN_ID
    );
}

fn scaffold(dir: &std::path::Path, tests: &str) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::create_dir_all(dir.join("tests")).unwrap();
    fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"mcdc-harvest-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(dir.join("src/lib.rs"), LIB).unwrap();
    fs::write(dir.join("tests/it.rs"), tests).unwrap();
}

#[test]
fn all_three_vectors_tagged_and_passing_discharges_the_obligation() {
    let dir = temp_crate_dir("full");
    scaffold(
        &dir,
        r#"
        #[test]
        fn mcdc__lib_decide_503cd493__v1() { assert!(mcdc_harvest_fixture::decide(true, false)); }
        #[test]
        fn mcdc__lib_decide_503cd493__v2() { assert!(!mcdc_harvest_fixture::decide(false, false)); }
        #[test]
        fn mcdc__lib_decide_503cd493__v3() { assert!(mcdc_harvest_fixture::decide(false, true)); }
        "#,
    );

    let decisions = scan_source(LIB).unwrap();
    let obligations = to_records("src/lib.rs", &decisions);

    let discharges = harvest(&obligations, &dir).unwrap();
    assert_eq!(discharges.len(), 1);
    assert!(discharges[0].discharged);
    assert_eq!(discharges[0].vectors_discharged, 3);
    assert_eq!(discharges[0].tagged_tests.len(), 3);

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_missing_vector_leaves_the_obligation_undischarged() {
    let dir = temp_crate_dir("partial");
    scaffold(
        &dir,
        r#"
        #[test]
        fn mcdc__lib_decide_503cd493__v1() { assert!(mcdc_harvest_fixture::decide(true, false)); }
        "#,
    );

    let decisions = scan_source(LIB).unwrap();
    let obligations = to_records("src/lib.rs", &decisions);

    let discharges = harvest(&obligations, &dir).unwrap();
    assert!(!discharges[0].discharged);
    assert_eq!(discharges[0].vectors_discharged, 1);

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn untagged_tests_do_not_count_toward_discharge() {
    let dir = temp_crate_dir("untagged");
    scaffold(
        &dir,
        r#"
        #[test]
        fn decide_returns_true_when_either_is_true() {
            assert!(mcdc_harvest_fixture::decide(true, false));
        }
        "#,
    );

    let decisions = scan_source(LIB).unwrap();
    let obligations = to_records("src/lib.rs", &decisions);

    let discharges = harvest(&obligations, &dir).unwrap();
    assert!(!discharges[0].discharged);
    assert_eq!(discharges[0].vectors_discharged, 0);
    assert!(discharges[0].tagged_tests.is_empty());

    fs::remove_dir_all(&dir).unwrap();
}

/// Two files sharing a stem (`vm/batch.rs`, `codegen/batch.rs`) at the
/// same line must get distinct, module-path-qualified ids so tagged
/// vectors land on the right decision (issue #121).
#[test]
fn same_stem_files_get_distinct_ids_and_vectors_attribute_to_the_right_one() {
    let dir = temp_crate_dir("same-stem");
    scaffold(
        &dir,
        r#"
        #[test]
        fn mcdc__vm_batch_decide_503cd493__v1() { assert!(mcdc_harvest_fixture::vm::batch::decide(true, false)); }
        #[test]
        fn mcdc__vm_batch_decide_503cd493__v2() { assert!(!mcdc_harvest_fixture::vm::batch::decide(false, false)); }
        #[test]
        fn mcdc__vm_batch_decide_503cd493__v3() { assert!(mcdc_harvest_fixture::vm::batch::decide(false, true)); }
        #[test]
        fn mcdc__codegen_batch_decide_503cd493__v1() { assert!(mcdc_harvest_fixture::codegen::batch::decide(true, false)); }
        "#,
    );
    // Replace the flat lib with two same-stem modules holding the same decision.
    fs::create_dir_all(dir.join("src/vm")).unwrap();
    fs::create_dir_all(dir.join("src/codegen")).unwrap();
    fs::write(
        dir.join("src/lib.rs"),
        "pub mod vm { pub mod batch; }\npub mod codegen { pub mod batch; }\n",
    )
    .unwrap();
    fs::write(dir.join("src/vm/batch.rs"), LIB).unwrap();
    fs::write(dir.join("src/codegen/batch.rs"), LIB).unwrap();

    let decisions = scan_source(LIB).unwrap();
    let mut obligations = to_records("src/vm/batch.rs", &decisions);
    obligations.extend(to_records("src/codegen/batch.rs", &decisions));
    assert_eq!(obligations[0].id, VM_ID);
    assert_eq!(obligations[1].id, CODEGEN_ID);

    let discharges = harvest(&obligations, &dir).unwrap();
    assert_eq!(discharges.len(), 2);
    assert!(discharges[0].discharged);
    assert_eq!(discharges[0].vectors_discharged, 3);
    assert!(!discharges[1].discharged);
    assert_eq!(discharges[1].vectors_discharged, 1);

    fs::remove_dir_all(&dir).unwrap();
}
