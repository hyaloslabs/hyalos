//! The audit over copies of the real stored log replies, and over chains made here under test keys.
//! Nothing here contacts the network or opens a key outside its own temporary directory.

use super::*;

const REAL_LINKS: usize = 4;

fn source(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(path)
}

pub(super) struct Rig {
    pub(super) root: PathBuf,
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl Rig {
    pub(super) fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!("hyalos-audit-test-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("ledger")).unwrap();
        fs::create_dir_all(root.join("log")).unwrap();
        fs::create_dir_all(root.join("home")).unwrap();
        Self { root }
    }

    pub(super) fn path(&self, name: &str) -> String {
        self.root.join(name).display().to_string()
    }

    /// Copies of the first four real links and replies, with the real pinned key and operator key.
    fn real(&self) -> Auditor {
        for seq in 0..REAL_LINKS {
            fs::copy(source(&format!("ledger/{seq:05}.json")), self.root.join(format!("ledger/{seq:05}.json"))).unwrap();
            for dir in ["ledger", "log"] {
                fs::copy(source(&format!("ledger/{seq:05}.rekor.json")), self.root.join(format!("{dir}/{seq:05}.rekor.json"))).unwrap();
            }
        }
        let reply: Value = serde_json::from_slice(&fs::read(source("ledger/00000.rekor.json")).unwrap()).unwrap();
        let body: Value = serde_json::from_slice(&B64.decode(reply.as_object().unwrap().values().next().unwrap()["body"].as_str().unwrap()).unwrap()).unwrap();
        let operator = B64.decode(body["spec"]["signature"]["publicKey"]["content"].as_str().unwrap()).unwrap();
        fs::write(self.path("operator.pub"), operator).unwrap();
        fs::write(self.path("log.pub"), public_pem(&B64.decode(REKOR_KEY).unwrap())).unwrap();
        self.auditor("operator.pub", "log.pub")
    }

    pub(super) fn auditor(&self, operator: &str, log_key: &str) -> Auditor {
        Auditor::new(&self.path("ledger"), &self.path(operator), Some((self.path("log"), self.path(log_key)))).unwrap()
    }

    /// Makes a P-256 key pair and returns the private key's path. The public key is `<name>.pub`.
    pub(super) fn key(&self, name: &str) -> String {
        let private = self.path(&format!("{name}.pem"));
        assert!(openssl(&["genpkey", "-algorithm", "EC", "-pkeyopt", "ec_paramgen_curve:P-256", "-out", &private]).is_some());
        let public = openssl(&["pkey", "-in", &private, "-pubout"]).unwrap();
        fs::write(self.path(&format!("{name}.pub")), public).unwrap();
        private
    }

    fn sign(&self, key: &str, message: &[u8]) -> Vec<u8> {
        let file = self.path("to-sign");
        fs::write(&file, message).unwrap();
        openssl(&["dgst", "-sha256", "-sign", key, &file]).unwrap()
    }

    /// A reply from a test log: the entry alone in a tree of one, signed as the real log signs.
    fn entry(&self, log: &str, signer: &str, link: &[u8], index: u64) -> Vec<u8> {
        self.entry_at(log, signer, link, index, 1795297278)
    }

    /// The same reply, with the log signing `time` for the entry.
    pub(super) fn entry_at(&self, log: &str, signer: &str, link: &[u8], index: u64, time: u64) -> Vec<u8> {
        let log_der = public_der(&fs::read_to_string(format!("{}.pub", log.trim_end_matches(".pem"))).unwrap()).unwrap();
        let log_id = sha(&log_der);
        let signer_public = fs::read(format!("{}.pub", signer.trim_end_matches(".pem"))).unwrap();
        let body = json!({"apiVersion": "0.0.1", "kind": "hashedrekord", "spec": {
            "data": {"hash": {"algorithm": "sha256", "value": sha(link)}},
            "signature": {"content": B64.encode(self.sign(signer, link)), "publicKey": {"content": B64.encode(signer_public)}},
        }});
        let body_bytes = serde_json::to_vec(&body).unwrap();
        let body_text = B64.encode(&body_bytes);
        let canonical = format!("{{\"body\":\"{body_text}\",\"integratedTime\":{time},\"logID\":\"{log_id}\",\"logIndex\":{index}}}");
        let leaf = Sha256::digest([&[0u8], body_bytes.as_slice()].concat()).to_vec();
        let note = format!("test log\n1\n{}", B64.encode(&leaf));
        let note_signature = self.sign(log, format!("{note}\n").as_bytes());
        let hint = crate::unhex(&log_id).unwrap();
        let line = B64.encode([&hint[..4], note_signature.as_slice()].concat());
        let reply = json!({ format!("entry{index}"): {
            "body": body_text, "integratedTime": time, "logID": log_id, "logIndex": index,
            "verification": {
                "signedEntryTimestamp": B64.encode(self.sign(log, canonical.as_bytes())),
                "inclusionProof": {"checkpoint": format!("{note}\n\n— test log {line}\n"), "hashes": [], "logIndex": 0, "rootHash": crate::hex(&leaf), "treeSize": 1},
            },
        }});
        serde_json::to_vec(&reply).unwrap()
    }

    /// Adds one link to the chain under test keys, naming the one before it. Returns its hash.
    pub(super) fn push(&self, log: &str, operator: &str, mut link: Value) -> String {
        let chain = read_links(&self.root.join("ledger"));
        let seq = chain.len();
        link["seq"] = json!(seq);
        link["prev"] = json!(chain.last().map(|l| sha(l)).unwrap_or_default());
        let bytes = serde_json::to_vec(&link).unwrap();
        fs::write(self.root.join(format!("ledger/{seq:05}.json")), &bytes).unwrap();
        let reply = self.entry(log, operator, &bytes, 100 + seq as u64);
        for dir in ["ledger", "log"] {
            fs::write(self.root.join(format!("{dir}/{seq:05}.rekor.json")), &reply).unwrap();
        }
        sha(&bytes)
    }

    /// Writes a chain under test keys and returns the commitment of its first result.
    fn chain(&self, kinds: &[&str], log: &str, operator: &str) -> String {
        let (mut intent, mut first) = (String::new(), String::new());
        for (seq, kind) in kinds.iter().enumerate() {
            if *kind == "intent" {
                intent = self.push(log, operator, intent_link());
            } else {
                let commitment = format!("commitment-{seq}");
                self.push(log, operator, result_link(&intent, &commitment));
                if first.is_empty() {
                    first = commitment;
                }
            }
        }
        first
    }
}

fn intent_link() -> Value {
    json!({"v": 1, "kind": "intent", "task": "one task", "guest_sha256": "one guest"})
}

fn result_link(intent: &str, commitment: &str) -> Value {
    json!({"v": 1, "kind": "result", "intent": intent, "outcome": "ok", "transcript": commitment})
}

fn test_claim(commitment: &str) -> Claim {
    Claim { commitment: commitment.to_string(), task: "one task".to_string(), guest: "one guest".to_string(), version: 1 }
}

/// The real record's claim, recomputed from `out/live2.json`.
fn real_claim() -> Claim {
    let t: crate::Transcript = serde_json::from_slice(&fs::read(source("out/live2.json")).unwrap()).unwrap();
    crate::claim_of(&t, real_commitment()).unwrap()
}

/// The commitment of the real record that the second real link names.
fn real_commitment() -> String {
    let link: Value = serde_json::from_slice(&fs::read(source("ledger/00001.json")).unwrap()).unwrap();
    link["transcript"].as_str().unwrap().to_string()
}

#[test]
fn t_4_the_pinned_key_is_the_one_in_the_trusted_root_and_hashes_to_the_log_id() {
    let root = fs::read_to_string(source("../../core/hyalos/tests/fixtures/marketplace-r10/trusted-root.json")).unwrap();
    assert_eq!(root.lines().nth(106).unwrap().split('"').nth(3), Some(REKOR_KEY));
    assert_eq!(sha(&B64.decode(REKOR_KEY).unwrap()), "c0d23d6ad406973f9559f3ba2d1ca01f84147d8ffc5b8445c224f98b9591801d");
}

#[test]
fn t_4_1_a_private_key_given_as_the_operator_key_is_refused() {
    let rig = Rig::new("t-4-1");
    let private = rig.key("someone");
    let refused = Auditor::new(&rig.path("ledger"), &private, None);
    assert!(refused.err().is_some_and(|e| e.contains("not a readable public key")));
}

#[test]
fn s_04_2_e1_the_stored_entries_verify() {
    let rig = Rig::new("s-04-2-e1");
    let audited = rig.real().audit(&real_claim()).unwrap();
    assert_eq!((audited.link, audited.links, audited.runs), (1, 4, 2));
}

#[test]
fn s_04_2_e2_an_altered_inclusion_proof_is_refused_naming_the_link() {
    let rig = Rig::new("s-04-2-e2");
    let auditor = rig.real();
    let file = rig.root.join("ledger/00002.rekor.json");
    let mut reply: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    let id = reply.as_object().unwrap().keys().next().unwrap().clone();
    let hash = reply[&id]["verification"]["inclusionProof"]["hashes"][3].as_str().unwrap().to_string();
    let flipped = if hash.starts_with('0') { "1" } else { "0" };
    reply[&id]["verification"]["inclusionProof"]["hashes"][3] = json!(format!("{flipped}{}", &hash[1..]));
    fs::write(&file, serde_json::to_vec(&reply).unwrap()).unwrap();
    assert_eq!(auditor.audit(&real_claim()).err().unwrap(), "link 2: the inclusion proof does not verify");
}

#[test]
fn s_04_2_e3_a_link_signed_with_a_test_key_is_refused_naming_the_link() {
    // Among the real entries: the replacement cannot carry the real log's proofs.
    let rig = Rig::new("s-04-2-e3");
    let auditor = rig.real();
    let (log, stranger) = (rig.key("test-log"), rig.key("stranger"));
    let link = fs::read(rig.root.join("ledger/00001.json")).unwrap();
    fs::write(rig.root.join("ledger/00001.rekor.json"), rig.entry(&log, &stranger, &link, 7)).unwrap();
    assert_eq!(auditor.audit(&real_claim()).err().unwrap(), "link 1: the reply is not from the log's key");

    // Under a log that accepts it: only the operator's key can fail it.
    let rig = Rig::new("s-04-2-e3-log");
    let (log, operator, stranger) = (rig.key("test-log"), rig.key("operator"), rig.key("stranger"));
    let commitment = rig.chain(&["intent", "result"], &log, &operator);
    let link = fs::read(rig.root.join("ledger/00001.json")).unwrap();
    fs::write(rig.root.join("ledger/00001.rekor.json"), rig.entry(&log, &stranger, &link, 101)).unwrap();
    let auditor = rig.auditor("operator.pub", "test-log.pub");
    assert_eq!(auditor.audit(&test_claim(&commitment)).err().unwrap(), "link 1: the link is not signed by the operator's key");
}

#[test]
fn t_4_a_reply_for_another_link_is_refused() {
    let rig = Rig::new("t-4-other");
    let auditor = rig.real();
    fs::copy(rig.root.join("ledger/00000.rekor.json"), rig.root.join("ledger/00001.rekor.json")).unwrap();
    assert_eq!(auditor.audit(&real_claim()).err().unwrap(), "link 1: the reply is for another link");
}

#[test]
fn t_4_an_altered_signed_time_is_refused() {
    let rig = Rig::new("t-4-time");
    let auditor = rig.real();
    let file = rig.root.join("ledger/00003.rekor.json");
    let mut reply: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    let id = reply.as_object().unwrap().keys().next().unwrap().clone();
    reply[&id]["integratedTime"] = json!(reply[&id]["integratedTime"].as_u64().unwrap() + 1);
    fs::write(&file, serde_json::to_vec(&reply).unwrap()).unwrap();
    assert_eq!(auditor.audit(&real_claim()).err().unwrap(), "link 3: the signed time does not verify");
}

#[test]
fn s_04_5_e1_a_passing_report_lists_what_was_proven_then_the_five_limits() {
    let rig = Rig::new("s-04-5-e1");
    let audited = rig.real().audit(&real_claim()).unwrap();
    let report = crate::audit_report(&audited, 8, false, None, 0);
    let lines: Vec<&str> = report.lines().collect();
    let first_limit = lines.iter().position(|l| l.starts_with("not proven")).unwrap();
    assert!(lines[..first_limit].iter().filter(|l| l.starts_with("proven: ")).count() == 5, "{report}");
    assert!(lines[..first_limit].iter().any(|l| l.contains("all 8 answers were attested by the trusted notary for this run and position")), "{report}");
    assert!(!report.contains("not checked"), "{report}");
    let limits = &lines[first_limit..];
    assert_eq!(limits.len(), 5, "{report}");
    for name in ["notary's independence", "model's name", "correctness", "every entry", "hidden bytes"] {
        assert!(limits.iter().any(|l| l.contains(name)), "{name}: {report}");
    }
}

#[test]
fn s_04_5_e3_the_report_counts_three_runs_of_one_task() {
    let rig = Rig::new("s-04-5-e3");
    let (log, operator) = (rig.key("test-log"), rig.key("operator"));
    let commitment = rig.chain(&["intent", "result", "intent", "result", "intent", "result"], &log, &operator);
    let audited = rig.auditor("operator.pub", "test-log.pub").audit(&test_claim(&commitment)).unwrap();
    assert_eq!(audited.runs, 3);
    assert!(crate::audit_report(&audited, 1, false, None, 0).contains("the task was run 3 times"));
}

#[test]
fn t_4_an_entry_the_chain_does_not_show_is_refused() {
    let rig = Rig::new("t-4-hidden");
    let (log, operator) = (rig.key("test-log"), rig.key("operator"));
    let commitment = rig.chain(&["intent", "result"], &log, &operator);
    fs::write(rig.root.join("log/00099.rekor.json"), rig.entry(&log, &operator, b"a hidden link", 500)).unwrap();
    let refused = rig.auditor("operator.pub", "test-log.pub").audit(&test_claim(&commitment)).err().unwrap();
    assert!(refused.contains("the log lists 3 entries") && refused.contains("has 2"), "{refused}");
}

#[test]
fn t_4_2_the_proofs_need_no_network_and_a_log_that_cannot_be_asked_refuses_the_audit() {
    let rig = Rig::new("t-4-2");
    rig.real();
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    let auditor = Auditor {
        dir: rig.root.join("ledger"),
        operator: public_der(&fs::read_to_string(rig.path("operator.pub")).unwrap()).unwrap(),
        log_key: B64.decode(REKOR_KEY).unwrap(),
        log: Log::Public { base, agent: ureq::AgentBuilder::new().timeout(Duration::from_secs(2)).build() },
    };
    let refused = auditor.audit(&real_claim()).err().unwrap();
    assert_eq!(refused, "the public log could not be asked which entries it holds under the operator's key");
}

/// A test chain and its keys, for the forgeries below.
fn forged(label: &str) -> (Rig, String, String) {
    let rig = Rig::new(label);
    let (log, operator) = (rig.key("test-log"), rig.key("operator"));
    (rig, log, operator)
}

fn audit_of(rig: &Rig, claim: &Claim) -> Result<Audited, String> {
    rig.auditor("operator.pub", "test-log.pub").audit(claim)
}

#[test]
fn t_4_a_discarded_result_with_its_intent_respelled_is_refused() {
    let (rig, log, operator) = forged("t-4-respelled");
    let intent = rig.push(&log, &operator, intent_link());
    rig.push(&log, &operator, result_link(&intent.to_uppercase(), "discarded"));
    rig.push(&log, &operator, result_link(&intent, "kept"));
    assert_eq!(audit_of(&rig, &test_claim("kept")).err().unwrap(), "link 1: the result names no earlier intent");
}

#[test]
fn t_4_a_link_of_another_kind_is_refused() {
    let (rig, log, operator) = forged("t-4-kind");
    let intent = rig.push(&log, &operator, intent_link());
    let mut hidden = result_link(&intent, "discarded");
    hidden["kind"] = json!("Result");
    rig.push(&log, &operator, hidden);
    rig.push(&log, &operator, result_link(&intent, "kept"));
    assert_eq!(audit_of(&rig, &test_claim("kept")).err().unwrap(), "link 1: it is neither an intent nor a result");
}

#[test]
fn t_4_a_second_result_for_one_run_is_refused_whichever_record_is_shown() {
    let (rig, log, operator) = forged("t-4-second");
    let intent = rig.push(&log, &operator, intent_link());
    rig.push(&log, &operator, result_link(&intent, "first"));
    rig.push(&log, &operator, result_link(&intent, "second"));
    for shown in ["first", "second"] {
        assert_eq!(audit_of(&rig, &test_claim(shown)).err().unwrap(), "link 2: a second result was logged for one run");
    }
}

#[test]
fn t_4_one_record_claimed_by_two_runs_is_refused() {
    let (rig, log, operator) = forged("t-4-twice");
    for _ in 0..2 {
        let intent = rig.push(&log, &operator, intent_link());
        rig.push(&log, &operator, result_link(&intent, "same"));
    }
    assert_eq!(audit_of(&rig, &test_claim("same")).err().unwrap(), "2 results claim this record");
}

#[test]
fn t_4_a_result_naming_a_link_that_is_not_an_intent_is_refused() {
    let (rig, log, operator) = forged("t-4-not-intent");
    let intent = rig.push(&log, &operator, intent_link());
    let result = rig.push(&log, &operator, result_link(&intent, "first"));
    rig.push(&log, &operator, result_link(&result, "second"));
    assert_eq!(audit_of(&rig, &test_claim("second")).err().unwrap(), "link 2: the result names no earlier intent");
}

#[test]
fn t_4_an_intent_labelled_with_another_task_or_guest_is_refused() {
    for (label, field) in [("task", "task"), ("guest", "guest_sha256")] {
        let (rig, log, operator) = forged(&format!("t-4-label-{label}"));
        let mut relabelled = intent_link();
        relabelled[field] = json!("something else");
        let intent = rig.push(&log, &operator, relabelled);
        rig.push(&log, &operator, result_link(&intent, "kept"));
        assert_eq!(audit_of(&rig, &test_claim("kept")).err().unwrap(), "link 0: the intent names another task than this record's");
    }
}

#[test]
fn t_4_a_run_relabelled_as_another_task_still_counts_in_the_total() {
    let (rig, log, operator) = forged("t-4-total");
    let mut other = intent_link();
    other["task"] = json!("a label for a discarded run");
    let discarded = rig.push(&log, &operator, other);
    rig.push(&log, &operator, result_link(&discarded, "discarded"));
    let intent = rig.push(&log, &operator, intent_link());
    rig.push(&log, &operator, result_link(&intent, "kept"));
    let audited = audit_of(&rig, &test_claim("kept")).unwrap();
    assert_eq!((audited.runs, audited.all_runs), (1, 2));
    let report = crate::audit_report(&audited, 1, false, None, 0);
    assert!(report.contains("as the intent links name the task") && report.contains("the chain holds 2 run(s) of any task"), "{report}");
}

#[test]
fn t_4_a_record_whose_result_link_says_refused_is_refused() {
    let (rig, log, operator) = forged("t-4-outcome");
    let intent = rig.push(&log, &operator, intent_link());
    let mut result = result_link(&intent, "kept");
    result["outcome"] = json!("refused:ModelRefused");
    rig.push(&log, &operator, result);
    assert_eq!(audit_of(&rig, &test_claim("kept")).err().unwrap(), "this record's result link says the run was refused");
}

#[test]
fn t_4_a_field_the_commitment_does_not_cover_is_refused() {
    let mut record: Value = serde_json::from_slice(&fs::read(source("out/live2.json")).unwrap()).unwrap();
    record["reviewed_by"] = json!("someone");
    assert!(serde_json::from_value::<crate::Transcript>(record).is_err());
}

/// A version 2 intent naming the test operator's key.
fn intent_v2(rig: &Rig) -> Value {
    let key = public_der(&fs::read_to_string(rig.path("operator.pub")).unwrap()).unwrap();
    json!({"v": 2, "kind": "intent", "task": "one task", "guest_sha256": "one guest", "operator": sha(&key)})
}

fn result_v2(intent: &str, commitment: &str) -> Value {
    json!({"v": 2, "kind": "result", "intent": intent, "outcome": "ok", "transcript": commitment})
}

fn claim_v2(commitment: &str) -> Claim {
    Claim { version: 2, ..test_claim(commitment) }
}

#[test]
fn record_s_01_4_e1_a_version_2_record_whose_intent_says_1_is_refused() {
    let (rig, log, operator) = forged("r-01-4-e1");
    let intent = rig.push(&log, &operator, intent_link());
    rig.push(&log, &operator, result_link(&intent, "kept"));
    assert_eq!(audit_of(&rig, &claim_v2("kept")).err().unwrap(), "link 0: the run's links are not this record's version");
}

#[test]
fn record_s_01_4_e2_a_version_2_record_whose_result_says_1_is_refused() {
    let (rig, log, operator) = forged("r-01-4-e2");
    let intent = rig.push(&log, &operator, intent_v2(&rig));
    rig.push(&log, &operator, result_link(&intent, "kept"));
    assert_eq!(audit_of(&rig, &claim_v2("kept")).err().unwrap(), "link 1: its version is not its intent's");
}

#[test]
fn record_s_01_4_e3_a_chain_of_both_versions_passes_for_the_version_2_record() {
    let (rig, log, operator) = forged("r-01-4-e3");
    let old = rig.push(&log, &operator, intent_link());
    rig.push(&log, &operator, result_link(&old, "old"));
    let new = rig.push(&log, &operator, intent_v2(&rig));
    rig.push(&log, &operator, result_v2(&new, "kept"));
    assert_eq!(audit_of(&rig, &claim_v2("kept")).unwrap().version, 2);
}

#[test]
fn record_s_01_8_e1_another_link_of_an_unknown_version_is_refused() {
    let (rig, log, operator) = forged("r-01-8-e1");
    let intent = rig.push(&log, &operator, intent_v2(&rig));
    rig.push(&log, &operator, result_v2(&intent, "kept"));
    let mut unknown = intent_v2(&rig);
    unknown["v"] = json!(99);
    rig.push(&log, &operator, unknown);
    assert_eq!(audit_of(&rig, &claim_v2("kept")).err().unwrap(), "link 2: its version is not known");
}

#[test]
fn record_s_01_8_e2_a_result_differing_from_its_intent_is_refused_for_any_record() {
    let (rig, log, operator) = forged("r-01-8-e2");
    let intent = rig.push(&log, &operator, intent_v2(&rig));
    rig.push(&log, &operator, result_v2(&intent, "kept"));
    let other = rig.push(&log, &operator, intent_v2(&rig));
    rig.push(&log, &operator, result_link(&other, "other"));
    assert_eq!(audit_of(&rig, &claim_v2("kept")).err().unwrap(), "link 3: its version is not its intent's");
}

#[test]
fn record_s_01_8_e3_the_committed_chain_is_not_refused_for_its_versions() {
    let rig = Rig::new("r-01-8-e3");
    assert_eq!(rig.real().audit(&real_claim()).unwrap().version, 1);
}

#[test]
fn record_s_05_9_e1_an_intent_naming_another_key_is_refused() {
    let (rig, log, operator) = forged("r-05-9-e1");
    let mut other = intent_v2(&rig);
    other["operator"] = json!("ab".repeat(32));
    let intent = rig.push(&log, &operator, other);
    rig.push(&log, &operator, result_v2(&intent, "kept"));
    assert_eq!(audit_of(&rig, &claim_v2("kept")).err().unwrap(), "link 0: the intent names another operator key");
}

#[test]
fn record_s_05_9_e2_an_intent_naming_this_key_is_not_refused() {
    let (rig, log, operator) = forged("r-05-9-e2");
    let intent = rig.push(&log, &operator, intent_v2(&rig));
    rig.push(&log, &operator, result_v2(&intent, "kept"));
    assert!(audit_of(&rig, &claim_v2("kept")).is_ok());
}

#[test]
fn record_s_05_10_e1_one_run_is_counted_under_this_log_key_only() {
    let (rig, log, operator) = forged("r-05-10-e1");
    let intent = rig.push(&log, &operator, intent_v2(&rig));
    rig.push(&log, &operator, result_v2(&intent, "kept"));
    let report = crate::audit_report(&audit_of(&rig, &claim_v2("kept")).unwrap(), 1, false, None, 0);
    assert!(report.contains("the task was run 1 time under this log key only"), "{report}");
    assert!(report.contains("the chain holds 1 run(s) of any task under this log key only"), "{report}");
}

#[test]
fn record_s_05_10_e2_three_runs_are_counted_under_this_log_key_only() {
    let (rig, log, operator) = forged("r-05-10-e2");
    for run in 0..3 {
        let intent = rig.push(&log, &operator, intent_v2(&rig));
        rig.push(&log, &operator, result_v2(&intent, &format!("run-{run}")));
    }
    let report = crate::audit_report(&audit_of(&rig, &claim_v2("run-0")).unwrap(), 1, false, None, 0);
    assert!(report.contains("the task was run 3 times under this log key only"), "{report}");
    assert!(report.contains("the chain holds 3 run(s) of any task under this log key only"), "{report}");
}

fn identity(inputs: &[&[u8]]) -> String {
    crate::task_v2(&sha(b"a guest"), &inputs.iter().map(|i| i.to_vec()).collect::<Vec<_>>()).unwrap()
}

#[test]
fn record_s_02_1_e1_inputs_with_equal_joined_bytes_differ() {
    assert_ne!(identity(&[b"ab", b"c"]), identity(&[b"a", b"bc"]));
}

#[test]
fn record_s_02_1_e2_no_inputs_and_one_empty_input_differ() {
    assert_ne!(identity(&[]), identity(&[b""]));
}

#[test]
fn record_s_02_1_e3_the_same_list_gives_the_same_identity() {
    assert_eq!(identity(&[b"ab", b"c"]), identity(&[b"ab", b"c"]));
}

#[test]
fn record_s_02_2_e1_the_two_versions_of_one_task_differ() {
    assert_ne!(identity(&[b"ab"]), crate::task_of(&sha(b"a guest"), &[b"ab".to_vec()]));
}

#[test]
fn record_s_02_2_e2_the_two_versions_of_no_inputs_differ() {
    assert_ne!(identity(&[]), crate::task_of(&sha(b"a guest"), &[]));
}
