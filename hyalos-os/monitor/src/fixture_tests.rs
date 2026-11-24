//! `local_4_2_generate` owns the fixtures in `verify/tests/fixtures/`. It runs every refusing test of
//! `forgery_tests.rs` and `audit_tests.rs` with a sink set, so each audit call those tests refuse is
//! kept as one case: the forged record, its links, its stand-in log and only public keys. With
//! `HYALOS_REGENERATE_FIXTURES=1` it rewrites them. Otherwise it regenerates them in a temporary
//! directory and fails unless every verdict and named check equals the committed one. Signatures
//! differ on each run, so they are never compared.

use super::*;
use crate::live_tests::built;
use crate::{AuditInputs, Verifier, read_transcript};
use hyalos_audit::{Failure, FailureKind};
use std::{cell::RefCell, collections::BTreeMap, ffi::OsString};

/// Refusing tests that have no forged record for the verifier to read, each with its reason.
const OUTSIDE: &[(&str, &str)] = &[
    ("record_s_05_7_e4_a_second_log_key_gets_no_more_attempts", "the notary refuses during a live run, before any record exists"),
    ("s_03_2_e4_an_attestation_whose_request_carries_no_binding_is_refused", "it calls the answer check directly on a verify report, with no record"),
    ("record_s_05_8_e1_a_version_2_audit_with_no_guest_is_refused", "a missing option and no forged input: the verifier is inconclusive, shown by local_3_3"),
    ("t_4_1_a_private_key_given_as_the_operator_key_is_refused", "it reads a key and no record, and a fixture would commit a private key"),
    ("t_4_2_the_proofs_need_no_network_and_a_log_that_cannot_be_asked_refuses_the_audit", "an unreachable log and no forged input: the verifier is inconclusive, shown by S-03.3.e1"),
];

const TRUST_NAME: &str = "provider.test";
const TRUST_ADDRESS: &str = "127.0.0.1:1";
const NO_NOTARY: &str = "021111111111111111111111111111111111111111111111111111111111111111";

struct Sink {
    dir: PathBuf,
    test: String,
    kept: usize,
    cases: Vec<(String, String)>,
}

thread_local! {
    static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
}

/// What a test's temporary directories are prefixed with, so a generator run cannot meet the same
/// test running at the same time.
pub(super) fn rig_prefix() -> &'static str {
    if SINK.with(|sink| sink.borrow().is_some()) { "generated-" } else { "" }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// One audit call's inputs.
pub(super) struct Spec<'a> {
    pub(super) record: &'a Path,
    pub(super) attest: bool,
    pub(super) links: Option<&'a Path>,
    pub(super) log: Option<&'a Path>,
    pub(super) log_key: Option<&'a Path>,
    pub(super) operator: Option<&'a Path>,
    pub(super) notary: &'a str,
    pub(super) guest: Option<&'a Path>,
    pub(super) root: Option<&'a Path>,
    pub(super) legacy: bool,
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let text = name.to_string_lossy();
        if text.ends_with(".json") || text.ends_with(".presentation") {
            fs::copy(entry.path(), to.join(&name)).unwrap();
        }
    }
}

/// Writes the case's files and judges them the way the built verifier does.
fn write_case(dest: &Path, spec: &Spec) -> Result<String, Failure> {
    fs::create_dir_all(dest).unwrap();
    let record = dest.join("record");
    fs::copy(spec.record, &record).unwrap();
    let salt = PathBuf::from(format!("{}.salt", spec.record.display()));
    if salt.exists() {
        fs::copy(&salt, dest.join("record.salt")).unwrap();
    }
    let attest = PathBuf::from(format!("{}.attest", spec.record.display()));
    if spec.attest && attest.exists() {
        copy_dir(&attest, &dest.join("record.attest"));
    }
    fs::create_dir_all(dest.join("links")).unwrap();
    fs::create_dir_all(dest.join("log")).unwrap();
    if let Some(links) = spec.links {
        copy_dir(links, &dest.join("links"));
    }
    if let Some(log) = spec.log {
        copy_dir(log, &dest.join("log"));
    }
    let placeholder = root().join("audit/tests/golden/log.pub");
    fs::copy(spec.log_key.unwrap_or(&placeholder), dest.join("log.pub")).unwrap();
    fs::copy(spec.operator.unwrap_or(&placeholder), dest.join("operator.pub")).unwrap();
    fs::write(dest.join("notary.pub"), format!("{}\n", spec.notary)).unwrap();
    if let Some(guest) = spec.guest {
        fs::copy(guest, dest.join("guest.wasm")).unwrap();
    }
    if let Some(trust_root) = spec.root {
        fs::copy(trust_root, dest.join("root.der")).unwrap();
    }
    let case = json!({"legacy": spec.legacy, "guest": spec.guest.is_some(), "trust": spec.root.is_some()});
    fs::write(dest.join("case.json"), serde_json::to_vec_pretty(&case).unwrap()).unwrap();
    let path = |name: &str| dest.join(name).display().to_string();
    let inputs = AuditInputs {
        dir: Some(path("links")),
        operator: Some(path("operator.pub")),
        notary: spec.notary.to_string(),
        verify: built("attest").display().to_string(),
        legacy: spec.legacy,
        guest: spec.guest.map(|_| path("guest.wasm")),
    };
    let trust = if spec.root.is_some() {
        vec![
            ("HYALOS_TRUST_SERVER_NAME".to_string(), OsString::from(TRUST_NAME)),
            ("HYALOS_TRUST_ROOT".to_string(), OsString::from(path("root.der"))),
            ("HYALOS_TRUST_PROVIDER_ADDRESS".to_string(), OsString::from(TRUST_ADDRESS)),
        ]
    } else {
        Vec::new()
    };
    let verifier = Verifier { program: inputs.verify.clone(), trust };
    crate::audit_record(&path("record"), &inputs, LogSource::StandIn(path("log"), path("log.pub")), &verifier, &crate::PlainFiles)
}

/// Keeps one refused audit call as a case. `said` is what the test was told, and `exact` says the
/// fixture must be refused by that same check. A test that reads the record through a shim of the
/// verify program is not exact, because the built verifier sees the real output.
pub(super) fn keep(spec: Spec, said: &str, exact: bool) {
    SINK.with(|sink| {
        let mut sink = sink.borrow_mut();
        let Some(sink) = sink.as_mut() else { return };
        let id = format!("{}/{:02}", sink.test, sink.kept);
        sink.kept += 1;
        let dest = sink.dir.join("cases").join(&id);
        let failure = match write_case(&dest, &spec) {
            Err(failure) => failure,
            Ok(_) => panic!("{id}: a forgery in the corpus passes"),
        };
        assert!(matches!(failure.kind, FailureKind::Refused), "{id}: not a refusal: {}", failure.message);
        if exact {
            assert_eq!(failure.message, said, "{id}: the fixture meets another check");
        }
        let expected = json!({"verdict": "refused", "check": failure.message});
        fs::write(dest.join("expected.json"), serde_json::to_vec_pretty(&expected).unwrap()).unwrap();
        sink.cases.push((id, failure.message.clone()));
    });
}

/// A record's bytes that its parser refuses, kept as a case with placeholder keys.
pub(super) fn keep_unparsable(bytes: &[u8], said: &str) {
    if SINK.with(|sink| sink.borrow().is_none()) {
        return;
    }
    let dir = std::env::temp_dir().join(format!("hyalos-fixture-bytes-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let record = dir.join("record");
    fs::write(&record, bytes).unwrap();
    keep(Spec { record: &record, attest: false, links: None, log: None, log_key: None, operator: None, notary: NO_NOTARY, guest: None, root: None, legacy: true }, said, true);
    fs::remove_dir_all(&dir).unwrap();
}

/// A record the version check refuses, kept with placeholder keys.
pub(super) fn keep_version(record: &Path, legacy: bool, said: &str) {
    keep(Spec { record, attest: false, links: None, log: None, log_key: None, operator: None, notary: NO_NOTARY, guest: None, root: None, legacy }, said, true);
}

/// Keeps a passing record outside the corpus, and gives its report.
pub(super) fn keep_pass(into: &Path, name: &str, spec: Spec) -> String {
    let dest = into.join("pass").join(name);
    let report = write_case(&dest, &spec).unwrap_or_else(|failure| panic!("{name}: {}", failure.message));
    fs::write(dest.join("expected.json"), serde_json::to_vec_pretty(&json!({"verdict": "passed"})).unwrap()).unwrap();
    report
}

/// Every test in a file's source that asserts a refusal.
fn refusing(source: &str) -> Vec<String> {
    let mut found = Vec::new();
    let pieces: Vec<&str> = source.split("#[test]").skip(1).collect();
    for piece in pieces {
        let body = piece.split("\n}\n").next().unwrap();
        let name = body.split("fn ").nth(1).unwrap().split('(').next().unwrap().to_string();
        if [".refused()", "unwrap_err", ".err()", "expect_err", "is_err()", "refused_real(", "parse_refused("].iter().any(|mark| body.contains(mark)) {
            found.push(name);
        }
    }
    found
}

fn corpus_names() -> Vec<&'static str> {
    super::audit_tests::CORPUS.iter().chain(super::forgery_tests::CORPUS).map(|(name, _)| *name).collect()
}

fn generate(into: &Path) -> Value {
    let _ = fs::remove_dir_all(into);
    fs::create_dir_all(into).unwrap();
    let mut corpus = Vec::new();
    for (name, test) in super::audit_tests::CORPUS.iter().chain(super::forgery_tests::CORPUS) {
        SINK.with(|sink| *sink.borrow_mut() = Some(Sink { dir: into.to_path_buf(), test: name.to_string(), kept: 0, cases: Vec::new() }));
        test();
        let done = SINK.with(|sink| sink.borrow_mut().take()).unwrap();
        assert!(!done.cases.is_empty(), "{name} kept no case");
        let cases: Vec<Value> = done.cases.iter().map(|(id, check)| json!({"id": id, "check": check})).collect();
        corpus.push(json!({"test": name, "cases": cases}));
    }
    SINK.with(|sink| *sink.borrow_mut() = Some(Sink { dir: into.to_path_buf(), test: "pass".to_string(), kept: 0, cases: Vec::new() }));
    super::forgery_tests::pass_fixtures(into);
    SINK.with(|sink| *sink.borrow_mut() = None);
    let outside: Vec<Value> = OUTSIDE.iter().map(|(test, reason)| json!({"test": test, "reason": reason})).collect();
    let manifest = json!({"corpus": corpus, "outside": outside});
    fs::write(into.join("manifest.json"), serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    manifest
}

fn checks(manifest: &Value) -> BTreeMap<String, String> {
    let mut all = BTreeMap::new();
    for test in manifest["corpus"].as_array().unwrap() {
        for case in test["cases"].as_array().unwrap() {
            all.insert(case["id"].as_str().unwrap().to_string(), case["check"].as_str().unwrap().to_string());
        }
    }
    all
}

#[test]
fn local_4_2_generate() {
    // The manifest's tests are exactly the refusing tests in the two files.
    let mut sources = refusing(include_str!("audit_tests.rs"));
    sources.extend(refusing(include_str!("forgery_tests.rs")));
    let outside: Vec<&str> = OUTSIDE.iter().map(|(test, _)| *test).collect();
    let mut wanted: Vec<&str> = sources.iter().map(String::as_str).filter(|name| !outside.contains(name)).collect();
    let mut listed = corpus_names();
    wanted.sort();
    listed.sort();
    assert_eq!(listed, wanted, "the corpus lists are not the refusing tests");

    let committed = root().join("verify/tests/fixtures");
    if std::env::var_os("HYALOS_REGENERATE_FIXTURES").is_some() {
        generate(&committed);
        return;
    }
    let scratch = std::env::temp_dir().join(format!("hyalos-fixtures-{}", std::process::id()));
    let made = generate(&scratch);
    let held: Value = serde_json::from_slice(&fs::read(committed.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(checks(&made), checks(&held), "the fixtures have drifted: rerun with HYALOS_REGENERATE_FIXTURES=1");
    assert_eq!(made["outside"], held["outside"]);
    for id in checks(&made).keys() {
        let read = |base: &Path| -> Value { serde_json::from_slice(&fs::read(base.join("cases").join(id).join("expected.json")).unwrap()).unwrap() };
        assert_eq!(read(&scratch), read(&committed), "{id}");
    }
    for name in ["unknown-proof", "five-answers"] {
        let read = |base: &Path| -> Value { serde_json::from_slice(&fs::read(base.join("pass").join(name).join("expected.json")).unwrap()).unwrap() };
        assert_eq!(read(&scratch), read(&committed), "{name}");
    }
    fs::remove_dir_all(&scratch).unwrap();
}

/// A record's bytes must be refused by the parser, and the refusal is kept.
pub(super) fn parse_refused(bytes: &[u8]) -> String {
    let said = read_transcript(bytes).unwrap_err();
    keep_unparsable(bytes, &said);
    said
}
