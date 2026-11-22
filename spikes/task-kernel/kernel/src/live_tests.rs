//! The monitor's attested path, against the built notary and stand-in provider. They are found in
//! `attest/target/release`, and a test fails when they are missing. No test contacts the real
//! provider or the public log, and every key and credential is made up in a temporary directory.

use super::*;
use crate::live::{Attest, Live};
use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
};

const INTENT: &str = "5f1d0c7a9b3e4f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8";
const NAME: &str = "provider.test";

/// A session costs seconds and several hundred megabytes, so tests take them one at a time.
static ONE: Mutex<()> = Mutex::new(());

pub(crate) fn built(program: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../attest/target/release").join(program);
    assert!(path.exists(), "{program} is not built: run cargo build --release --manifest-path attest/Cargo.toml");
    path
}

struct Rig {
    dir: PathBuf,
    children: Vec<Child>,
    settings: Vec<(String, String)>,
    operator: String,
}

impl Drop for Rig {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

impl Rig {
    /// A temporary home with a made-up credential and an operator's transport key.
    fn new(label: &str) -> Self {
        let dir = env::temp_dir().join(format!("hyalos-live-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("home/.config/hyalos-spike")).unwrap();
        fs::write(dir.join("home/.config/hyalos-spike/anthropic.key"), format!("{}\n", "x".repeat(106))).unwrap();
        let key = dir.join("operator-transport.key");
        let made = Command::new(built("attest")).arg("link-key").arg(&key).output().unwrap();
        assert!(made.status.success());
        let operator = String::from_utf8(made.stdout).unwrap().trim().to_string();
        let settings = vec![
            ("HOME".to_string(), dir.join("home").display().to_string()),
            ("HYALOS_TRUST_SERVER_NAME".to_string(), NAME.to_string()),
            ("HYALOS_OPERATOR_TRANSPORT_KEY".to_string(), key.display().to_string()),
        ];
        Self { dir, children: Vec::new(), settings, operator }
    }

    fn set(&mut self, name: &str, value: impl Into<String>) {
        self.settings.retain(|(have, _)| have != name);
        self.settings.push((name.to_string(), value.into()));
    }

    /// Starts a program and reads the lines it prints before it serves.
    fn start(&mut self, mut command: Command, fields: &[&str]) -> Vec<String> {
        let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let mut printed = BufReader::new(child.stdout.take().unwrap());
        let values = fields
            .iter()
            .map(|field| {
                let mut line = String::new();
                printed.read_line(&mut line).unwrap();
                line.trim().strip_prefix(field).unwrap_or_else(|| panic!("no {field}")).trim().to_string()
            })
            .collect();
        self.children.push(child);
        values
    }

    fn standin(&mut self, controls: &[&str]) {
        let root = self.dir.join("root.der");
        let mut command = Command::new(built("standin"));
        command.arg(&root).arg(NAME).args(controls);
        let values = self.start(command, &["listening"]);
        self.set("HYALOS_TRUST_ROOT", root.display().to_string());
        self.set("HYALOS_TRUST_PROVIDER_ADDRESS", values[0].clone());
    }

    fn notary(&mut self) {
        let mut command = Command::new(built("notary"));
        command
            .args(["--listen", "127.0.0.1:0", "--approved-server", NAME, "--stop-on-stdin-close", "--operator-transport-public", &self.operator, "--dir"])
            .arg(self.dir.join("notary"))
            .arg("--root")
            .arg(self.dir.join("root.der"));
        let values = self.start(command, &["signing", "transport", "listening"]);
        self.set("HYALOS_NOTARY_SIGNER_PUBLIC", values[0].clone());
        self.set("HYALOS_NOTARY_TRANSPORT_PUBLIC", values[1].clone());
        self.set("HYALOS_NOTARY_ADDRESS", values[2].clone());
    }

    /// Notary settings that name a peer no one should reach.
    fn unused_keys(&mut self) {
        self.set("HYALOS_TRUST_ROOT", self.dir.join("root.der").display().to_string());
        self.set("HYALOS_TRUST_PROVIDER_ADDRESS", "127.0.0.1:9");
        self.set("HYALOS_NOTARY_SIGNER_PUBLIC", "00".repeat(33));
        self.set("HYALOS_NOTARY_TRANSPORT_PUBLIC", "00".repeat(91));
        fs::write(self.dir.join("root.der"), b"").unwrap();
    }

    fn attest(&self) -> Attest {
        Attest { tool: built("attest").display().to_string(), dir: self.dir.join("record.attest").display().to_string(), settings: self.settings.clone() }
    }

    fn record(&self) -> PathBuf {
        self.dir.join("record")
    }

    /// One whole declared run, with the links kept here in place of the public log. It stops at
    /// the first model call in every test that uses it, so no notification can be sent.
    fn refused_run(&self, attest: Option<Attest>) -> (Reason, Vec<serde_json::Value>) {
        let guest = fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../guest/digest.wasm")).unwrap();
        let inputs = read_inputs(Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/issues.tsv").to_str().unwrap()).unwrap();
        let mode = Mode::Live(Live::new("test.invalid/none", 0, attest).unwrap());
        let mut links = Vec::new();
        let mut append = |link: serde_json::Value| {
            let hash = sha(link.to_string().as_bytes());
            links.push(link);
            Ok((links.len() - 1, hash))
        };
        let (outcome, _) = declared_run(&mut append, &Backend::Process, &guest, inputs, mode, &random_salt(), self.record().to_str().unwrap(), &"ab".repeat(32), &|_| Some(0)).unwrap();
        let reason = outcome.err().expect("the run must be refused");
        (reason, links)
    }

    fn assert_refused(&self, attest: Option<Attest>, expected: Reason) {
        let (reason, links) = self.refused_run(attest);
        assert_eq!(reason, expected);
        assert_eq!(links.len(), 2, "one intent link and one result link");
        assert_eq!(links[1]["outcome"], format!("refused:{expected:?}"), "the result link says why");
        assert_eq!(links[1]["transcript"], "");
        assert!(!self.record().exists(), "no record is written");
        assert!(!Path::new(&format!("{}.salt", self.record().display())).exists());
    }
}

/// A fresh salt for each run, as the `live` command makes one.
pub(crate) fn random_salt() -> String {
    let mut salt = [0u8; 32];
    std::io::Read::read_exact(&mut fs::File::open("/dev/urandom").unwrap(), &mut salt).unwrap();
    hex(&salt)
}

pub(crate) fn locked() -> std::sync::MutexGuard<'static, ()> {
    ONE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn s_02_1_e1_an_attested_answer_names_a_presentation_hash_that_matches_the_file() {
    let _one = locked();
    let mut rig = Rig::new("s-02-1");
    rig.standin(&[]);
    rig.notary();
    let attest = rig.attest();
    let file = format!("{}/000.presentation", attest.dir);
    let mut live = Live::new("test.invalid/none", 0, Some(attest)).unwrap();
    live.bind(INTENT, Some(0));
    let (label, model, response, presentation) = live.infer(b"The build fails on a clean checkout").unwrap();
    assert_eq!((label, model.as_str(), response.as_str()), (0, "standin", "msg_standin"));
    assert_eq!(presentation, sha(&fs::read(file).unwrap()));
}

#[test]
fn s_02_2_e1_nothing_listening_refuses_the_run_and_the_result_link_says_so() {
    let _one = locked();
    let mut rig = Rig::new("s-02-2-e1");
    rig.unused_keys();
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    rig.set("HYALOS_NOTARY_ADDRESS", closed.local_addr().unwrap().to_string());
    drop(closed);
    rig.assert_refused(Some(rig.attest()), Reason::NotaryUnreachable);
}

#[test]
fn s_02_2_e2_a_notary_that_goes_quiet_is_refused_at_the_deadline() {
    let mut rig = Rig::new("s-02-2-e2");
    rig.unused_keys();
    let quiet = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    rig.set("HYALOS_NOTARY_ADDRESS", quiet.local_addr().unwrap().to_string());
    let started = Instant::now();
    rig.assert_refused(Some(rig.attest()), Reason::NotaryDeadline);
    let took = started.elapsed();
    assert!(took >= Duration::from_secs(29) && took < Duration::from_secs(35), "{took:?}");
    drop(quiet);
}

#[test]
fn s_02_3_e1_an_attestation_under_another_key_is_refused() {
    let _one = locked();
    let mut rig = Rig::new("s-02-3");
    rig.standin(&[]);
    rig.notary();
    rig.set("HYALOS_NOTARY_SIGNER_PUBLIC", format!("02{}", "11".repeat(32)));
    rig.assert_refused(Some(rig.attest()), Reason::WrongNotaryKey);
}

#[test]
fn s_02_4_e1_a_notary_on_another_release_is_refused_at_the_start() {
    let _one = locked();
    let mut rig = Rig::new("s-02-4");
    rig.unused_keys();
    let peer = rig.dir.join("other-release");
    fs::create_dir_all(&peer).unwrap();
    let mut command = Command::new(built("standin"));
    command.arg("other-release").arg(&peer).arg(&rig.operator);
    let values = rig.start(command, &["transport", "listening"]);
    rig.set("HYALOS_NOTARY_TRANSPORT_PUBLIC", values[0].clone());
    rig.set("HYALOS_NOTARY_ADDRESS", values[1].clone());
    rig.assert_refused(Some(rig.attest()), Reason::ReleaseMismatch);
}

#[test]
fn t_3_3_with_no_notary_configured_the_run_is_refused() {
    let rig = Rig::new("t-3-3");
    rig.assert_refused(None, Reason::NotaryUnreachable);
}

#[test]
fn t_3_3_no_monitor_source_file_names_the_provider_or_the_credential_file() {
    let sources = [("live.rs", include_str!("live.rs")), ("main.rs", include_str!("main.rs")), ("kvm.rs", include_str!("kvm.rs")), ("ledger.rs", include_str!("ledger.rs"))];
    for (file, text) in sources {
        for banned in ["anthropic.key", "api.anthropic.com", "x-api-key", "credential:"] {
            assert!(!text.contains(banned), "{file} names {banned}");
        }
    }
}

#[test]
fn t_3_a_provider_error_is_its_own_reason() {
    let _one = locked();
    let mut rig = Rig::new("t-3-provider");
    rig.standin(&["--status", "500"]);
    rig.notary();
    rig.assert_refused(Some(rig.attest()), Reason::ProviderRefused);
}

/// A stand-in for the tool: a script with this body.
fn script(rig: &Rig, body: &str) -> Attest {
    use std::os::unix::fs::PermissionsExt;
    let path = rig.dir.join("tool.sh");
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    Attest { tool: path.display().to_string(), dir: rig.dir.join("record.attest").display().to_string(), settings: Vec::new() }
}

#[test]
fn t_3_a_word_outside_the_set_is_the_reason_for_a_failed_tool() {
    let rig = Rig::new("t-3-word");
    rig.assert_refused(Some(script(&rig, "cat >/dev/null; echo 'something else'; exit 1")), Reason::ModelUnavailable);
}

#[test]
fn t_3_an_answer_with_no_presentation_is_refused() {
    let rig = Rig::new("t-3-unattested");
    let said = r#"{"label":0,"model":"m","response_id":"r","presentation_sha256":"00"}"#;
    rig.assert_refused(Some(script(&rig, &format!("cat >/dev/null; echo '{said}'"))), Reason::ModelRefused);
}

#[test]
fn t_3_a_tool_still_running_at_35_seconds_is_ended() {
    let rig = Rig::new("t-3-limit");
    let started = Instant::now();
    rig.assert_refused(Some(script(&rig, "exec sleep 120")), Reason::ModelUnavailable);
    let took = started.elapsed();
    assert!(took >= Duration::from_secs(35) && took < Duration::from_secs(40), "{took:?}");
}

#[test]
fn t_3_3_with_no_notary_a_task_that_asks_nothing_does_not_run() {
    let rig = Rig::new("t-3-3-silent");
    let guest = fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../guest/digest.wasm")).unwrap();
    let mode = Mode::Live(Live::new("test.invalid/none", 0, None).unwrap());
    let mut links = Vec::new();
    let mut append = |link: serde_json::Value| {
        let hash = sha(link.to_string().as_bytes());
        links.push(link);
        Ok((links.len() - 1, hash))
    };
    let (outcome, _) = declared_run(&mut append, &Backend::Process, &guest, Vec::new(), mode, &random_salt(), rig.record().to_str().unwrap(), &"ab".repeat(32), &|_| Some(0)).unwrap();
    assert_eq!(outcome.err(), Some(Reason::NotaryUnreachable));
    assert_eq!(links[1]["outcome"], "refused:NotaryUnreachable");
    assert!(!rig.record().exists());
}

#[test]
fn t_3_a_descendant_holding_the_output_is_ended_at_the_limit() {
    let rig = Rig::new("t-3-descendant");
    let holder = rig.dir.join("holder");
    let started = Instant::now();
    let body = format!("cat >/dev/null; sleep 300 & echo $! > '{}'; exit 1", holder.display());
    rig.assert_refused(Some(script(&rig, &body)), Reason::ModelUnavailable);
    let took = started.elapsed();
    assert!(took >= Duration::from_secs(35) && took < Duration::from_secs(40), "{took:?}");
    let pid = fs::read_to_string(&holder).unwrap();
    let stat = format!("/proc/{}/stat", pid.trim());
    let gone = (0..100).any(|_| {
        std::thread::sleep(Duration::from_millis(20));
        fs::read_to_string(&stat).map_or(true, |text| text.rsplit(')').next().is_some_and(|rest| rest.trim_start().starts_with('Z')))
    });
    assert!(gone, "the descendant is still running");
}

#[test]
fn t_3_a_record_path_in_use_is_not_fresh() {
    let rig = Rig::new("t-3-fresh");
    let record = rig.record();
    let record = record.to_str().unwrap();
    assert!(crate::live::record_is_fresh(record));
    for used in [record.to_string(), format!("{record}.salt"), format!("{record}.attest"), format!("{record}.pending"), format!("{record}.salt.pending")] {
        fs::write(&used, b"").unwrap();
        assert!(!crate::live::record_is_fresh(record), "{used}");
        fs::remove_file(&used).unwrap();
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

#[test]
fn t_5_the_first_call_waits_until_two_seconds_past_the_intents_signed_time() {
    let rig = Rig::new("t-5-wait");
    let mut live = Live::new("test.invalid/none", 0, Some(script(&rig, "cat >/dev/null; echo provider; exit 1"))).unwrap();
    let signed = now();
    live.bind(INTENT, Some(signed));
    assert_eq!(live.infer(b"A question").unwrap_err(), Reason::ProviderRefused);
    assert!(now() >= signed + 2, "the call was made inside the wait");
}

#[test]
fn t_5_with_no_signed_time_or_one_far_ahead_no_call_is_made() {
    for (label, signed) in [("none", None), ("ahead", Some(now() + 3600))] {
        let rig = Rig::new(&format!("t-5-no-time-{label}"));
        let called = rig.dir.join("called");
        let body = format!("touch '{}'; exit 1", called.display());
        let mut live = Live::new("test.invalid/none", 0, Some(script(&rig, &body))).unwrap();
        live.bind(INTENT, signed);
        assert_eq!(live.infer(b"A question").unwrap_err(), Reason::LedgerFailed);
        assert!(!called.exists(), "the tool was started");
    }
}

fn committed(path: &str) -> Vec<u8> {
    fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(path)).unwrap()
}

#[test]
fn record_s_01_5_e1_a_committed_version_1_record_replays_identically() {
    let t = read_transcript(&committed("out/live3.json")).unwrap();
    assert_eq!(t.version, 1);
    assert_eq!(replay(&Backend::Process, &committed("guest/digest.wasm"), &t), Ok(()));
}

#[test]
fn record_s_01_5_e2_that_record_relabelled_as_version_2_never_replays() {
    let mut t = read_transcript(&committed("out/live3.json")).unwrap();
    t.version = 2;
    assert!(read_transcript(&serde_json::to_vec(&t).unwrap()).is_err());
    assert_eq!(replay(&Backend::Process, &committed("guest/digest.wasm"), &t), Err(Reason::ReplayDiverged));
}

fn version_2_run() -> Transcript {
    let inputs = read_inputs(Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/issues.tsv").to_str().unwrap()).unwrap();
    execute(&Backend::Process, &committed("guest/digest.wasm"), inputs, Mode::Record { seed: 0 }).unwrap()
}

#[test]
fn record_s_01_9_e1_a_version_2_transcript_with_no_task_is_refused() {
    let mut t = version_2_run();
    assert!(read_transcript(&serde_json::to_vec(&t).unwrap()).is_ok());
    t.task.clear();
    assert_eq!(read_transcript(&serde_json::to_vec(&t).unwrap()).unwrap_err(), "this version 2 record has no task");
}

#[test]
fn record_s_01_9_e2_a_version_2_transcript_naming_another_task_is_refused() {
    let mut t = version_2_run();
    t.task = "ab".repeat(32);
    assert_eq!(read_transcript(&serde_json::to_vec(&t).unwrap()).unwrap_err(), "this record's task is not the one its guest and inputs give");
    assert_eq!(replay(&Backend::Process, &committed("guest/digest.wasm"), &t), Err(Reason::ReplayDiverged));
}

#[test]
fn record_s_02_3_e1_the_task_command_prints_what_the_run_logs() {
    let rig = Rig::new("r-02-3-e1");
    let guest_file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../guest/digest.wasm");
    let inputs_file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/issues.tsv");
    let program = env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("task-kernel");
    assert!(program.exists(), "the monitor is not built");
    let printed = Command::new(program).arg("task").arg(&guest_file).arg(&inputs_file).env_clear().output().unwrap();
    assert!(printed.status.success());
    let mut links = Vec::new();
    let mut append = |link: serde_json::Value| {
        let hash = sha(link.to_string().as_bytes());
        links.push(link);
        Ok((links.len() - 1, hash))
    };
    let inputs = read_inputs(inputs_file.to_str().unwrap()).unwrap();
    declared_run(&mut append, &Backend::Process, &fs::read(&guest_file).unwrap(), inputs, Mode::Record { seed: 0 }, &random_salt(), rig.record().to_str().unwrap(), &"ab".repeat(32), &|_| Some(0)).unwrap();
    assert_eq!(links[0]["task"], String::from_utf8(printed.stdout).unwrap().trim());
    assert_eq!((links[0]["v"].as_u64(), links[1]["v"].as_u64()), (Some(2), Some(2)));
}

#[test]
fn record_s_02_3_e2_a_missing_guest_file_prints_no_identity() {
    let rig = Rig::new("r-02-3-e2");
    let inputs = rig.dir.join("inputs");
    fs::write(&inputs, "one input\n").unwrap();
    let program = env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("task-kernel");
    assert!(program.exists(), "the monitor is not built");
    let output = Command::new(program).arg("task").arg(rig.dir.join("no-such-guest")).arg(&inputs).env_clear().stdin(Stdio::null()).output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "no identity is printed");
}
