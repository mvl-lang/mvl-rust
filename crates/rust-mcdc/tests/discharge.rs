//! Integration test for [`rust_mcdc::discharge::discharge_file`] against a
//! throwaway crate built in the OS temp dir -- never touches this
//! workspace's own source tree. Exercises the real
//! mutate → write → `cargo test` → restore cycle end to end.

use std::fs;
use std::path::PathBuf;

use rust_mcdc::discharge::discharge_file;

/// A minimal standalone crate with one two-leaf `||` decision: one test
/// case discharges it fully (kills every condition mutant), the other
/// only exercises half the decision and leaves it undischarged.
fn scaffold_crate(dir: &std::path::Path, lib_body: &str, test_body: &str) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::create_dir_all(dir.join("tests")).unwrap();
    fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"mcdc-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(dir.join("src/lib.rs"), lib_body).unwrap();
    fs::write(dir.join("tests/it.rs"), test_body).unwrap();
}

fn temp_crate_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rust-mcdc-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

const LIB: &str = r#"
pub fn decide(a: bool, b: bool) -> bool {
    if a || b {
        true
    } else {
        false
    }
}
"#;

#[test]
fn fully_exercised_decision_is_discharged() {
    let dir = temp_crate_dir("discharged");
    scaffold_crate(
        &dir,
        LIB,
        r#"
        #[test]
        fn a_true_b_false() {
            assert!(mcdc_fixture::decide(true, false));
        }
        #[test]
        fn a_false_b_false() {
            assert!(!mcdc_fixture::decide(false, false));
        }
        #[test]
        fn a_false_b_true() {
            assert!(mcdc_fixture::decide(false, true));
        }
        "#,
    );

    let outcomes = discharge_file(&dir.join("src/lib.rs"), &dir).unwrap();
    let decision = outcomes.iter().find(|o| !o.compiler_void).unwrap();
    assert!(
        decision.discharged(),
        "expected full discharge, survivors: {:?}",
        decision.survived_descriptions
    );
    assert_eq!(decision.mutants_killed, decision.mutants_total);

    // The source file on disk must be back to its original text.
    assert_eq!(fs::read_to_string(dir.join("src/lib.rs")).unwrap(), LIB);

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn undertested_decision_survives_a_mutant() {
    let dir = temp_crate_dir("undischarged");
    scaffold_crate(
        &dir,
        LIB,
        r#"
        #[test]
        fn only_a_true() {
            assert!(mcdc_fixture::decide(true, false));
        }
        "#,
    );

    let outcomes = discharge_file(&dir.join("src/lib.rs"), &dir).unwrap();
    let decision = outcomes.iter().find(|o| !o.compiler_void).unwrap();
    assert!(!decision.discharged());
    assert!(!decision.survived_descriptions.is_empty());

    assert_eq!(fs::read_to_string(dir.join("src/lib.rs")).unwrap(), LIB);

    fs::remove_dir_all(&dir).unwrap();
}
