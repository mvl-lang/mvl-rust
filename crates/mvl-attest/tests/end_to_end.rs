//! End to end against a real binary: `examples/mvl-manifest-demo` built with
//! `cargo auditable`, verified through a fake `gh` that returns attestations
//! in the shape the real one does (the signature itself is gh's job and is
//! exercised for real by `.github/workflows/attest.yml`).
//!
//! Needs `cargo-auditable` on PATH, so it is ignored by default; the attest
//! workflow runs it with `--include-ignored`.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use mvl_attest::provenance::{BUILD_ENV, SLSA};
use mvl_attest::verify::{verify, Options, Policy};
use serde_json::{json, Value};

const REPO: &str = "mvl-lang/mvl-rust";
const WF: &str = ".github/workflows/attest.yml";
const REF: &str = "refs/heads/main";
const LOCK: &str = "examples/mvl-manifest-demo/Cargo.lock";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn build_demo(target: &Path) -> PathBuf {
    let demo = repo_root().join("examples/mvl-manifest-demo");
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(&demo)
        .args(["auditable", "build", "--locked", "--quiet"])
        .env("CARGO_TARGET_DIR", target)
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .output()
        .expect("run cargo auditable");
    assert!(
        out.status.success(),
        "cargo auditable build failed (is cargo-auditable installed?):\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    target.join("debug/mvl-manifest-demo")
}

/// One attestation result in gh's JSON shape.
fn attestation(sha: &str, commit: &str, predicate_type: &str, predicate: Value) -> Value {
    let signer = format!("https://github.com/{REPO}/{WF}@{REF}");
    json!([{"verificationResult": {
        "statement": {
            "subject": [{"name": "mvl-manifest-demo", "digest": {"sha256": sha}}],
            "predicateType": predicate_type,
            "predicate": predicate,
        },
        "signature": {"certificate": {
            "buildSignerURI": signer,
            "sourceRepositoryURI": format!("https://github.com/{REPO}"),
            "sourceRepositoryDigest": commit,
            "sourceRepositoryRef": REF,
            "runInvocationURI": format!("https://github.com/{REPO}/actions/runs/1/attempts/1"),
        }},
    }}])
}

/// A `gh` stand-in: answers `attestation verify` per predicate type and logs
/// its arguments.
fn fake_gh(dir: &Path, slsa: &Value, env: &Value) -> PathBuf {
    fs::write(dir.join("slsa.json"), slsa.to_string()).unwrap();
    fs::write(dir.join("env.json"), env.to_string()).unwrap();
    let gh = dir.join("gh");
    fs::write(
        &gh,
        format!(
            "#!/bin/sh\necho \"$@\" >> '{d}/args.log'\ncase \"$*\" in\n  *{BUILD_ENV}*) cat '{d}/env.json' ;;\n  *) cat '{d}/slsa.json' ;;\nesac\n",
            d = dir.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
    gh
}

#[test]
#[ignore = "needs cargo-auditable; run with --include-ignored"]
fn real_auditable_binary_verifies_and_a_modified_copy_does_not() {
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR")).join("attest-e2e");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    let bin = build_demo(&Path::new(env!("CARGO_TARGET_TMPDIR")).join("attest-demo-target"));

    // What the binary carries.
    let bytes = fs::read(&bin).unwrap();
    let record = mvl_attest::record::find(&bytes).expect("embedded record");
    let audit = mvl_attest::auditable::read(&bin).expect("cargo-auditable data");
    assert_eq!(record.get("auditable"), "true");
    assert!(audit.packages.iter().any(|p| p.name == "mvl-manifest-demo"));
    let commit = record.get("vcs_commit").to_string();
    assert_eq!(commit.len(), 40);

    // The predicate CI would sign.
    let root = repo_root();
    let predicate = {
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(&root).unwrap();
        let p = mvl_attest::predicate::generate(&bin, LOCK);
        std::env::set_current_dir(prev).unwrap();
        p.expect("predicate")
    };
    assert_eq!(predicate["auditable_digest"], audit.digest.as_str());
    assert_eq!(predicate["lockfile_digest"], record.get("lockfile_digest"));

    let sha = mvl_attest::sha256_hex(&bytes);
    let slsa_pred = json!({"buildDefinition": {"resolvedDependencies": [
        {"uri": format!("git+https://github.com/{REPO}@{REF}"), "digest": {"gitCommit": commit}}
    ]}});
    let gh = fake_gh(
        &tmp,
        &attestation(&sha, &commit, SLSA, slsa_pred.clone()),
        &attestation(&sha, &commit, BUILD_ENV, predicate.clone()),
    );
    let opts = |artifact: PathBuf| Options {
        artifact,
        policy: Policy {
            repo: REPO.into(),
            workflow: WF.into(),
            ref_pattern: "refs/heads/*".into(),
        },
        source: Some(root.clone()),
        lockfile: None,
        gh: gh.clone(),
        key: None,
    };

    let report = verify(&opts(bin.clone()));
    // A local tree may be dirty; everything else must hold.
    let expected_failures: Vec<&str> = if record.get("vcs_dirty") == "true" {
        vec!["clean-build"]
    } else {
        vec![]
    };
    let failed = report.failed();
    // The lockfile check reads the committed lockfile; a local edit to it is
    // the only other legitimate difference.
    let failed: Vec<&str> = failed
        .into_iter()
        .filter(|n| !(*n == "lockfile" && record.get("vcs_dirty") == "true"))
        .collect();
    assert_eq!(failed, expected_failures, "{:#?}", report.checks);

    // gh was asked to enforce repo and signer workflow for both predicates.
    let args = fs::read_to_string(tmp.join("args.log")).unwrap();
    assert!(args.contains(&format!("--repo {REPO}")));
    assert!(args.contains(&format!("--signer-workflow {REPO}/{WF}")));
    assert!(args.contains(SLSA) && args.contains(BUILD_ENV));

    // A modified copy: one flipped byte, same attestations.
    let mut tampered = bytes.clone();
    let n = tampered.len() / 2;
    tampered[n] ^= 0xff;
    let copy = tmp.join("tampered");
    fs::write(&copy, &tampered).unwrap();
    let report = verify(&opts(copy));
    assert!(
        report.failed().contains(&"signature"),
        "{:#?}",
        report.checks
    );
    assert!(!report.ok());
}

/// Run cosign with a throwaway key's password; panics with its stderr on failure.
fn cosign(dir: &Path, args: &[&str]) {
    let out = Command::new("cosign")
        .current_dir(dir)
        .env("COSIGN_PASSWORD", "test")
        .args(args)
        .output()
        .expect("run cosign (is it installed?)");
    assert!(
        out.status.success(),
        "cosign {args:?} failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
#[ignore = "needs cargo-auditable and cosign; run with --include-ignored"]
fn key_mode_with_real_cosign_verifies_and_rejects_tampering_wrong_key_and_wrong_repo() {
    use mvl_attest::predicate::{slsa, SlsaParams};
    use mvl_attest::verify::KeyMode;

    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR")).join("attest-key-mode");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    let built = build_demo(&Path::new(env!("CARGO_TARGET_TMPDIR")).join("attest-demo-target"));
    let bin = tmp.join("app");
    fs::copy(&built, &bin).unwrap();
    let record = mvl_attest::record::find(&fs::read(&bin).unwrap()).unwrap();
    let commit = record.get("vcs_commit").to_string();

    // Two throwaway key pairs: the signer's, and a stranger's.
    cosign(
        &tmp,
        &["generate-key-pair", "--output-key-prefix", "signer"],
    );
    cosign(
        &tmp,
        &["generate-key-pair", "--output-key-prefix", "stranger"],
    );

    // What CI would sign: our own SLSA predicate and the build environment.
    let root = repo_root();
    let env_pred = {
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(&root).unwrap();
        let p = mvl_attest::predicate::generate(&bin, LOCK);
        std::env::set_current_dir(prev).unwrap();
        p.unwrap()
    };
    let slsa_pred = slsa(&SlsaParams {
        repo: Some(REPO.into()),
        workflow: Some(WF.into()),
        git_ref: Some(REF.into()),
        commit: Some(commit.clone()),
    })
    .unwrap();
    fs::write(tmp.join("slsa.json"), slsa_pred.to_string()).unwrap();
    fs::write(tmp.join("env.json"), env_pred.to_string()).unwrap();
    for (pred, ty, bundle) in [
        ("slsa.json", SLSA, "app.slsa.bundle"),
        ("env.json", BUILD_ENV, "app.build-env.bundle"),
    ] {
        cosign(
            &tmp,
            &[
                "attest-blob",
                "--key",
                "signer.key",
                "--predicate",
                pred,
                "--type",
                ty,
                "--bundle",
                bundle,
                "--use-signing-config=false",
                "--tlog-upload=false",
                "--yes",
                "app",
            ],
        );
    }

    let opts = |artifact: PathBuf, key: &str, repo: &str| {
        let (slsa_bundle, build_env_bundle) = KeyMode::bundles_next_to(&tmp.join("app"));
        Options {
            artifact,
            policy: Policy {
                repo: repo.into(),
                workflow: WF.into(),
                ref_pattern: "refs/heads/*".into(),
            },
            source: Some(root.clone()),
            lockfile: None,
            gh: PathBuf::from("gh-must-not-be-called"),
            key: Some(KeyMode {
                key: tmp.join(key).display().to_string(),
                cosign: PathBuf::from("cosign"),
                slsa_bundle,
                build_env_bundle,
                tlog: false,
            }),
        }
    };
    // A local tree may be dirty; that and the committed lockfile are the only
    // legitimate local differences (as in the keyless test above).
    let tolerated =
        |n: &&str| record.get("vcs_dirty") == "true" && (*n == "clean-build" || *n == "lockfile");

    let report = verify(&opts(bin.clone(), "signer.pub", REPO));
    let failed: Vec<&str> = report
        .failed()
        .into_iter()
        .filter(|n| !tolerated(n))
        .collect();
    assert!(failed.is_empty(), "{:#?}", report.checks);

    // A modified binary: cosign finds the digest does not match the statement.
    let mut bytes = fs::read(&bin).unwrap();
    let n = bytes.len() / 2;
    bytes[n] ^= 0xff;
    let tampered = tmp.join("tampered");
    fs::write(&tampered, &bytes).unwrap();
    let r = verify(&opts(tampered, "signer.pub", REPO));
    assert!(r.failed().contains(&"signature"), "{:#?}", r.checks);

    // The wrong key: the signature does not verify.
    let r = verify(&opts(bin.clone(), "stranger.pub", REPO));
    assert!(r.failed().contains(&"signature"), "{:#?}", r.checks);

    // The wrong repository: the signed predicate names another one.
    let r = verify(&opts(bin, "signer.pub", "someone/else"));
    assert!(r.failed().contains(&"repository"), "{:#?}", r.checks);
}
