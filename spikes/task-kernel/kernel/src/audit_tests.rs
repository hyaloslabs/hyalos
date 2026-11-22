//! The audit over copies of the real stored log replies, and over chains made here under test keys.
//! Nothing here contacts the network or opens a key outside its own temporary directory.

use super::*;
use std::process::Stdio;

const REAL_LINKS: usize = 4;

fn source(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(path)
}

struct Rig {
    root: PathBuf,
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl Rig {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!("hyalos-audit-test-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("ledger")).unwrap();
        fs::create_dir_all(root.join("log")).unwrap();
        fs::create_dir_all(root.join("home")).unwrap();
        Self { root }
    }

    fn path(&self, name: &str) -> String {
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
        fs::write(self.path("notary.pub"), "not read until Slice 5\n").unwrap();
        self.auditor("operator.pub", "log.pub")
    }

    fn auditor(&self, operator: &str, log_key: &str) -> Auditor {
        Auditor::new(&self.path("ledger"), &self.path(operator), Some((self.path("log"), self.path(log_key)))).unwrap()
    }

    /// Makes a P-256 key pair and returns the private key's path. The public key is `<name>.pub`.
    fn key(&self, name: &str) -> String {
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
        let log_der = public_der(&fs::read_to_string(format!("{}.pub", log.trim_end_matches(".pem"))).unwrap()).unwrap();
        let log_id = sha(&log_der);
        let signer_public = fs::read(format!("{}.pub", signer.trim_end_matches(".pem"))).unwrap();
        let body = json!({"apiVersion": "0.0.1", "kind": "hashedrekord", "spec": {
            "data": {"hash": {"algorithm": "sha256", "value": sha(link)}},
            "signature": {"content": B64.encode(self.sign(signer, link)), "publicKey": {"content": B64.encode(signer_public)}},
        }});
        let body_bytes = serde_json::to_vec(&body).unwrap();
        let body_text = B64.encode(&body_bytes);
        let canonical = format!("{{\"body\":\"{body_text}\",\"integratedTime\":1795297278,\"logID\":\"{log_id}\",\"logIndex\":{index}}}");
        let leaf = Sha256::digest([&[0u8], body_bytes.as_slice()].concat()).to_vec();
        let note = format!("test log\n1\n{}", B64.encode(&leaf));
        let note_signature = self.sign(log, format!("{note}\n").as_bytes());
        let hint = crate::unhex(&log_id).unwrap();
        let line = B64.encode([&hint[..4], note_signature.as_slice()].concat());
        let reply = json!({ format!("entry{index}"): {
            "body": body_text, "integratedTime": 1795297278u64, "logID": log_id, "logIndex": index,
            "verification": {
                "signedEntryTimestamp": B64.encode(self.sign(log, canonical.as_bytes())),
                "inclusionProof": {"checkpoint": format!("{note}\n\n— test log {line}\n"), "hashes": [], "logIndex": 0, "rootHash": crate::hex(&leaf), "treeSize": 1},
            },
        }});
        serde_json::to_vec(&reply).unwrap()
    }

    /// Writes a chain under test keys and returns the commitment of its first result.
    fn chain(&self, kinds: &[&str], log: &str, operator: &str) -> String {
        let (mut prev, mut intent, mut first) = (String::new(), String::new(), String::new());
        for (seq, kind) in kinds.iter().enumerate() {
            let commitment = format!("commitment-{seq}");
            let link = if *kind == "intent" {
                json!({"v": 1, "kind": "intent", "task": "one task", "seq": seq, "prev": prev})
            } else {
                json!({"v": 1, "kind": "result", "intent": intent, "outcome": "ok", "transcript": commitment, "seq": seq, "prev": prev})
            };
            let bytes = serde_json::to_vec(&link).unwrap();
            if *kind == "intent" {
                intent = sha(&bytes);
            } else if first.is_empty() {
                first = commitment;
            }
            prev = sha(&bytes);
            fs::write(self.root.join(format!("ledger/{seq:05}.json")), &bytes).unwrap();
            let reply = self.entry(log, operator, &bytes, 100 + seq as u64);
            for dir in ["ledger", "log"] {
                fs::write(self.root.join(format!("{dir}/{seq:05}.rekor.json")), &reply).unwrap();
            }
        }
        first
    }
}

/// The commitment of the real record that the second real link names.
fn real_commitment() -> String {
    let link: Value = serde_json::from_slice(&fs::read(source("ledger/00001.json")).unwrap()).unwrap();
    link["transcript"].as_str().unwrap().to_string()
}

/// The built program, beside this test's own binary.
fn program() -> PathBuf {
    let path = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("task-kernel");
    assert!(path.exists(), "task-kernel is not built: run cargo build --release");
    path
}

fn run_audit(rig: &Rig, extra: &[&str]) -> std::process::Output {
    let mut command = Command::new(program());
    command
        .arg("audit")
        .arg(source("out/live2.json"))
        .arg(rig.path("ledger"))
        .args(extra)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap())
        .env("HOME", rig.path("home"))
        .env("HYALOS_TRUST_LOG_DIR", rig.path("log"))
        .env("HYALOS_TRUST_LOG_KEY", rig.path("log.pub"))
        .stdin(Stdio::null());
    command.output().unwrap()
}

#[test]
fn t_4_the_pinned_key_is_the_one_in_the_trusted_root_and_hashes_to_the_log_id() {
    let root = fs::read_to_string(source("../../core/hyalos/tests/fixtures/marketplace-r10/trusted-root.json")).unwrap();
    assert_eq!(root.lines().nth(106).unwrap().split('"').nth(3), Some(REKOR_KEY));
    assert_eq!(sha(&B64.decode(REKOR_KEY).unwrap()), "c0d23d6ad406973f9559f3ba2d1ca01f84147d8ffc5b8445c224f98b9591801d");
}

#[test]
fn s_04_1_e1_e2_t_4_1_the_audit_completes_with_an_empty_home_and_two_public_keys() {
    let rig = Rig::new("s-04-1");
    rig.real();
    let keys = [rig.path("operator.pub"), rig.path("notary.pub")];
    let output = run_audit(&rig, &["--operator-public", &keys[0], "--notary-public", &keys[1]]);
    let report = String::from_utf8(output.stdout).unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(report.starts_with("audit passed\ntest trust settings"), "{report}");
    assert!(!report.contains("public log"), "a stand-in report claims nothing of the public log: {report}");
    assert_eq!(fs::read_dir(rig.path("home")).unwrap().count(), 0, "the home directory stays empty");
}

#[test]
fn s_04_1_a_missing_public_key_is_refused_and_named() {
    let rig = Rig::new("s-04-1-keys");
    rig.real();
    let operator = rig.path("operator.pub");
    let notary = rig.path("notary.pub");
    let no_notary = run_audit(&rig, &["--operator-public", &operator]);
    assert!(!no_notary.status.success());
    assert!(String::from_utf8_lossy(&no_notary.stderr).contains("notary's public key"));
    let no_operator = run_audit(&rig, &["--notary-public", &notary]);
    assert!(!no_operator.status.success());
    assert!(String::from_utf8_lossy(&no_operator.stderr).contains("operator's public key"));
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
    let audited = rig.real().audit(&real_commitment()).unwrap();
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
    assert_eq!(auditor.audit(&real_commitment()).err().unwrap(), "link 2: the inclusion proof does not verify");
}

#[test]
fn s_04_2_e3_a_link_signed_with_a_test_key_is_refused_naming_the_link() {
    // Among the real entries: the replacement cannot carry the real log's proofs.
    let rig = Rig::new("s-04-2-e3");
    let auditor = rig.real();
    let (log, stranger) = (rig.key("test-log"), rig.key("stranger"));
    let link = fs::read(rig.root.join("ledger/00001.json")).unwrap();
    fs::write(rig.root.join("ledger/00001.rekor.json"), rig.entry(&log, &stranger, &link, 7)).unwrap();
    assert_eq!(auditor.audit(&real_commitment()).err().unwrap(), "link 1: the reply is not from the log's key");

    // Under a log that accepts it: only the operator's key can fail it.
    let rig = Rig::new("s-04-2-e3-log");
    let (log, operator, stranger) = (rig.key("test-log"), rig.key("operator"), rig.key("stranger"));
    let commitment = rig.chain(&["intent", "result"], &log, &operator);
    let link = fs::read(rig.root.join("ledger/00001.json")).unwrap();
    fs::write(rig.root.join("ledger/00001.rekor.json"), rig.entry(&log, &stranger, &link, 101)).unwrap();
    let auditor = rig.auditor("operator.pub", "test-log.pub");
    assert_eq!(auditor.audit(&commitment).err().unwrap(), "link 1: the link is not signed by the operator's key");
}

#[test]
fn t_4_a_reply_for_another_link_is_refused() {
    let rig = Rig::new("t-4-other");
    let auditor = rig.real();
    fs::copy(rig.root.join("ledger/00000.rekor.json"), rig.root.join("ledger/00001.rekor.json")).unwrap();
    assert_eq!(auditor.audit(&real_commitment()).err().unwrap(), "link 1: the reply is for another link");
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
    assert_eq!(auditor.audit(&real_commitment()).err().unwrap(), "link 3: the signed time does not verify");
}

#[test]
fn s_04_5_e1_a_passing_report_lists_what_was_proven_then_the_five_limits() {
    let rig = Rig::new("s-04-5-e1");
    let audited = rig.real().audit(&real_commitment()).unwrap();
    let report = crate::audit_report(&audited, None);
    let lines: Vec<&str> = report.lines().collect();
    let first_limit = lines.iter().position(|l| l.starts_with("not proven")).unwrap();
    assert!(lines[..first_limit].iter().filter(|l| l.starts_with("proven: ")).count() == 4, "{report}");
    assert!(lines[..first_limit].contains(&"not checked: the answers' attestations"), "{report}");
    let limits = &lines[first_limit..];
    assert_eq!(limits.len(), 5, "{report}");
    for name in ["notary's independence", "model's name", "correctness", "every entry", "hidden bytes"] {
        assert!(limits.iter().any(|l| l.contains(name)), "{name}: {report}");
    }
}

#[test]
fn t_4_with_attestations_checked_the_report_has_no_not_checked_line_and_five_limits() {
    let rig = Rig::new("t-4-attested");
    let audited = rig.real().audit(&real_commitment()).unwrap();
    let attested = crate::attested_report(8, false);
    let report = crate::audit_report(&audited, Some(&attested));
    assert!(report.contains("attested: 8 of 8") && !report.contains("not checked"), "{report}");
    assert_eq!(report.lines().filter(|l| l.starts_with("not proven")).count(), 5, "{report}");
}

#[test]
fn s_04_5_e2_a_failing_audit_names_the_first_check_and_prints_no_proof() {
    let rig = Rig::new("s-04-5-e2");
    rig.real();
    fs::remove_file(rig.root.join("ledger/00002.rekor.json")).unwrap();
    let keys = [rig.path("operator.pub"), rig.path("notary.pub")];
    let output = run_audit(&rig, &["--operator-public", &keys[0], "--notary-public", &keys[1]]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "no statement of proof");
    assert_eq!(String::from_utf8_lossy(&output.stderr).trim(), "refused: link 2: no stored log reply");
}

#[test]
fn s_04_5_e3_the_report_counts_three_runs_of_one_task() {
    let rig = Rig::new("s-04-5-e3");
    let (log, operator) = (rig.key("test-log"), rig.key("operator"));
    let commitment = rig.chain(&["intent", "result", "intent", "result", "intent", "result"], &log, &operator);
    let audited = rig.auditor("operator.pub", "test-log.pub").audit(&commitment).unwrap();
    assert_eq!(audited.runs, 3);
    assert!(crate::audit_report(&audited, None).contains("the task was run 3 times"));
}

#[test]
fn t_4_an_entry_the_chain_does_not_show_is_refused() {
    let rig = Rig::new("t-4-hidden");
    let (log, operator) = (rig.key("test-log"), rig.key("operator"));
    let commitment = rig.chain(&["intent", "result"], &log, &operator);
    fs::write(rig.root.join("log/00099.rekor.json"), rig.entry(&log, &operator, b"a hidden link", 500)).unwrap();
    let refused = rig.auditor("operator.pub", "test-log.pub").audit(&commitment).err().unwrap();
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
    let refused = auditor.audit(&real_commitment()).err().unwrap();
    assert_eq!(refused, "the public log could not be asked which entries it holds under the operator's key");
}
