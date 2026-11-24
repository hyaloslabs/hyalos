//! S-03.2: the built verifier refuses every forgery the monitor's tests know about, from committed
//! fixtures. The fixtures use a stand-in log, so these run on the `test-trust` build. S-03.7 proves
//! separately that a release build ignores a stand-in.

mod common;

use common::{assert_refused, built, cases, fixtures, manifest, root, verify};
use hyalos_audit::STAND_IN_BUILD;
use serde_json::Value;
use std::{fs, path::Path, process::Command};

fn stand_in(test: &str) -> bool {
    if !STAND_IN_BUILD {
        eprintln!("skipped: {test} needs the test-trust feature");
    }
    STAND_IN_BUILD
}

fn case_dir(id: &str) -> std::path::PathBuf {
    fixtures().join("cases").join(id)
}

#[test]
fn s_03_2_the_built_verifier_refuses_every_forgery_in_the_corpus() {
    if !stand_in("s_03_2") {
        return;
    }
    built("forging-notary");
    let manifest = manifest();
    let cases = cases(&manifest);
    assert!(!cases.is_empty());
    for (id, check) in &cases {
        let dir = case_dir(id);
        let expected: Value =
            serde_json::from_slice(&fs::read(dir.join("expected.json")).unwrap()).unwrap();
        assert_eq!(expected["verdict"], "refused", "{id}");
        assert_eq!(expected["check"], check.as_str(), "{id}");
        assert_refused(id, &verify(&dir), check);
    }
    let on_disk: usize = fs::read_dir(fixtures().join("cases"))
        .unwrap()
        .map(|test| fs::read_dir(test.unwrap().path()).unwrap().count())
        .sum();
    assert_eq!(on_disk, cases.len(), "cases on disk and in the manifest");
    println!(
        "s_03_2: ran {} cases of {} in the manifest",
        cases.len(),
        on_disk
    );
}

#[test]
fn s_03_2_e1_a_record_whose_link_has_a_second_result_is_refused() {
    if !stand_in("s_03_2_e1") {
        return;
    }
    let id = "t_4_a_second_result_for_one_run_is_refused_whichever_record_is_shown/00";
    let check = "link 2: a second result was logged for one run";
    assert_refused(id, &verify(&case_dir(id)), check);
}

#[test]
fn s_03_2_e2_a_presentation_from_an_untrusted_notary_is_refused_naming_the_answer() {
    if !stand_in("s_03_2_e2") {
        return;
    }
    let id = "s_04_4_e1_an_answer_attested_by_the_forgers_own_notary_is_refused/00";
    let check = "answer 0: not attested by the trusted notary";
    assert_refused(id, &verify(&case_dir(id)), check);
}

#[test]
fn local_4_1_the_manifest_names_each_test_outside_the_corpus_with_its_reason() {
    let manifest = manifest();
    let outside = manifest["outside"].as_array().unwrap();
    assert!(!outside.is_empty());
    for entry in outside {
        assert!(!entry["test"].as_str().unwrap().is_empty());
        assert!(!entry["reason"].as_str().unwrap().is_empty());
    }
    for test in manifest["corpus"].as_array().unwrap() {
        assert!(!test["cases"].as_array().unwrap().is_empty(), "{test}");
    }
}

fn walk(dir: &Path, found: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(&path, found);
        } else {
            found.push(path);
        }
    }
}

#[test]
fn local_4_3_only_public_keys_are_committed() {
    let mut files = Vec::new();
    walk(&fixtures(), &mut files);
    assert!(!files.is_empty());
    for file in files {
        let bytes = fs::read(&file).unwrap();
        let hidden = bytes.windows(11).any(|w| w == b"PRIVATE KEY");
        assert!(!hidden, "{}", file.display());
        assert_ne!(file.extension().and_then(|e| e.to_str()), Some("pem"));
    }
}

fn passes(name: &str) -> String {
    let output = verify(&fixtures().join("pass").join(name));
    assert_eq!(
        output.status.code(),
        Some(0),
        "{name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn local_4_4_an_unknown_provider_proof_keeps_its_report_line() {
    if !stand_in("local_4_4") {
        return;
    }
    let report = passes("unknown-proof");
    assert!(
        report
            .lines()
            .any(|l| l == "unchecked provider proofs: 1, which prove nothing"),
        "{report}"
    );
}

#[test]
fn local_4_5_the_five_answer_record_passes() {
    if !stand_in("local_4_5") {
        return;
    }
    let report = passes("five-answers");
    assert!(report.contains("all 5 answers were attested"), "{report}");
}

#[test]
fn local_4_6_the_verifier_does_not_depend_on_the_monitor() {
    let output = Command::new(std::env::var_os("CARGO").unwrap())
        .current_dir(root())
        .args([
            "tree",
            "--locked",
            "-p",
            "hyalos-verify",
            "-e",
            "normal,dev",
            "--prefix",
            "none",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let tree = String::from_utf8(output.stdout).unwrap();
    assert!(tree.starts_with("hyalos-verify "), "{tree}");
    assert!(
        !tree.lines().any(|l| l.starts_with("task-kernel ")),
        "{tree}"
    );
}
