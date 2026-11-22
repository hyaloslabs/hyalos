//! The `audit` command as an auditor runs it: public keys, an empty home directory, and a stand-in
//! log holding copies of the real stored replies. Nothing here contacts the network.

use base64::{Engine, engine::general_purpose::STANDARD as B64};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

fn source(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(path)
}

struct Rig(PathBuf);

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

impl Rig {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!("hyalos-audit-cli-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for dir in ["ledger", "log", "home"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        for seq in 0..4 {
            fs::copy(source(&format!("ledger/{seq:05}.json")), root.join(format!("ledger/{seq:05}.json"))).unwrap();
            for dir in ["ledger", "log"] {
                fs::copy(source(&format!("ledger/{seq:05}.rekor.json")), root.join(format!("{dir}/{seq:05}.rekor.json"))).unwrap();
            }
        }
        let reply: Value = serde_json::from_slice(&fs::read(source("ledger/00000.rekor.json")).unwrap()).unwrap();
        let entry = reply.as_object().unwrap().values().next().unwrap();
        let body: Value = serde_json::from_slice(&B64.decode(entry["body"].as_str().unwrap()).unwrap()).unwrap();
        fs::write(root.join("operator.pub"), B64.decode(body["spec"]["signature"]["publicKey"]["content"].as_str().unwrap()).unwrap()).unwrap();
        let trusted = fs::read_to_string(source("../../core/hyalos/tests/fixtures/marketplace-r10/trusted-root.json")).unwrap();
        let key = trusted.lines().nth(106).unwrap().split('"').nth(3).unwrap().to_string();
        fs::write(root.join("log.pub"), format!("-----BEGIN PUBLIC KEY-----\n{key}\n-----END PUBLIC KEY-----\n")).unwrap();
        fs::write(root.join("notary.pub"), format!("02{}\n", "11".repeat(32))).unwrap();
        fs::write(root.join("verify"), "").unwrap();
        Self(root)
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).display().to_string()
    }

    fn keys(&self) -> [String; 6] {
        ["--operator-public".to_string(), self.path("operator.pub"), "--notary-public".to_string(), self.path("notary.pub"), "--verify".to_string(), self.path("verify")]
    }

    fn audit(&self, extra: &[String], kernel: Option<&str>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_task-kernel"));
        command
            .arg("audit")
            .arg(source("out/live2.json"))
            .arg(self.path("ledger"))
            .args(extra)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap())
            .env("HOME", self.path("home"))
            .env("HYALOS_TRUST_LOG_DIR", self.path("log"))
            .env("HYALOS_TRUST_LOG_KEY", self.path("log.pub"))
            .stdin(Stdio::null());
        if let Some(path) = kernel {
            command.env("HYALOS_KERNEL", path);
        }
        command.output().unwrap()
    }
}

// The whole audit passing from an empty home is `s_04_1_the_whole_audit_passes_with_an_empty_home`
// in `kernel/src/forgery_tests.rs`, over a record with real presentations.

#[test]
fn t_5_a_record_made_before_this_pack_is_refused_at_its_attestations() {
    let rig = Rig::new("t-5-old");
    let output = rig.audit(&rig.keys(), None);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "no statement of proof");
    assert_eq!(String::from_utf8_lossy(&output.stderr).trim(), "refused: answer 0: no attestation");
    assert_eq!(fs::read_dir(rig.path("home")).unwrap().count(), 0, "the home directory stays empty");
}

#[test]
fn t_4_1_the_audit_does_not_open_the_kernel_file() {
    let rig = Rig::new("t-4-1-kernel");
    let output = rig.audit(&rig.keys(), Some("/nonexistent/hyalos-kernel"));
    assert_eq!(String::from_utf8_lossy(&output.stderr).trim(), "refused: answer 0: no attestation");
}

#[test]
fn s_04_1_a_missing_public_key_is_refused_and_named() {
    let rig = Rig::new("s-04-1-keys");
    let keys = rig.keys();
    let no_notary = rig.audit(&[&keys[..2], &keys[4..]].concat(), None);
    assert!(!no_notary.status.success());
    assert!(String::from_utf8_lossy(&no_notary.stderr).contains("notary's public key"));
    let no_operator = rig.audit(&keys[2..], None);
    assert!(!no_operator.status.success());
    assert!(String::from_utf8_lossy(&no_operator.stderr).contains("operator's public key"));
}

#[test]
fn t_5_1_no_notary_setting_replaces_the_key_and_no_verify_program_is_taken_from_the_environment() {
    let rig = Rig::new("t-5-1");
    let keys = rig.keys();
    let mut no_key = Command::new(env!("CARGO_BIN_EXE_task-kernel"));
    no_key.arg("audit").arg(source("out/live2.json")).arg(rig.path("ledger")).args(&keys[..2]).args(&keys[4..]);
    let refused = no_key.env_clear().env("PATH", std::env::var("PATH").unwrap()).env("HOME", rig.path("home")).env("HYALOS_NOTARY", "02").output().unwrap();
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("notary's public key is not given"));
    let mut no_verify = Command::new(env!("CARGO_BIN_EXE_task-kernel"));
    no_verify.arg("audit").arg(source("out/live2.json")).arg(rig.path("ledger")).args(&keys[..4]);
    let refused = no_verify.env_clear().env("PATH", std::env::var("PATH").unwrap()).env("HOME", rig.path("home")).env("HYALOS_ATTEST", rig.path("verify")).output().unwrap();
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("verify program is not given"));
}

#[test]
fn t_5_1_a_notary_key_that_is_not_a_signing_key_is_refused() {
    let rig = Rig::new("t-5-1-key");
    fs::write(rig.path("notary.pub"), "not a key\n").unwrap();
    let output = rig.audit(&rig.keys(), None);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not a signing public key"));
}

#[test]
fn s_04_5_e2_a_failing_audit_names_the_first_check_and_prints_no_proof() {
    let rig = Rig::new("s-04-5-e2");
    fs::remove_file(rig.0.join("ledger/00002.rekor.json")).unwrap();
    let output = rig.audit(&rig.keys(), None);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "no statement of proof");
    assert_eq!(String::from_utf8_lossy(&output.stderr).trim(), "refused: link 2: no stored log reply");
}
