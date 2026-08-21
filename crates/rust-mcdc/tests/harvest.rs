//! Integration test for [`rust_mcdc::harvest::harvest`] against a
//! throwaway crate built in the OS temp dir -- exercises the real
//! scan → tag → `cargo test` → join cycle end to end, no mutation.

use std::fs;
use std::path::PathBuf;

use rust_mcdc::harvest::harvest;
use rust_mcdc::scanner::scan_source;

fn temp_crate_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rust-mcdc-harvest-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

const LIB: &str = r#"pub fn decide(a: bool, b: bool) -> bool {
    if a || b { true } else { false }
}
"#;

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
        fn mcdc__lib_2__v1() { assert!(mcdc_harvest_fixture::decide(true, false)); }
        #[test]
        fn mcdc__lib_2__v2() { assert!(!mcdc_harvest_fixture::decide(false, false)); }
        #[test]
        fn mcdc__lib_2__v3() { assert!(mcdc_harvest_fixture::decide(false, true)); }
        "#,
    );

    let decisions = scan_source(LIB).unwrap();
    let obligations: Vec<_> = decisions
        .iter()
        .map(|d| d.to_record("src/lib.rs"))
        .collect();

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
        fn mcdc__lib_2__v1() { assert!(mcdc_harvest_fixture::decide(true, false)); }
        "#,
    );

    let decisions = scan_source(LIB).unwrap();
    let obligations: Vec<_> = decisions
        .iter()
        .map(|d| d.to_record("src/lib.rs"))
        .collect();

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
    let obligations: Vec<_> = decisions
        .iter()
        .map(|d| d.to_record("src/lib.rs"))
        .collect();

    let discharges = harvest(&obligations, &dir).unwrap();
    assert!(!discharges[0].discharged);
    assert_eq!(discharges[0].vectors_discharged, 0);
    assert!(discharges[0].tagged_tests.is_empty());

    fs::remove_dir_all(&dir).unwrap();
}
