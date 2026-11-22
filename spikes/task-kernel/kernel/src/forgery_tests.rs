//! Slice 5: a forger who holds every operator key, against the real ask, a real test notary and the
//! stand-in provider. Chains are made here under test keys on a stand-in log, so each forgery passes
//! the log step and is refused by the check it is meant to meet. No test contacts the real provider
//! or the public log, and every key and credential is made up in a temporary directory.

use super::audit_tests::Rig;
use super::*;
use crate::live_tests::{built, locked, random_salt};
use crate::{Answer, AuditInputs, Transcript, Verifier};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

const NAME: &str = "provider.test";
const QUESTIONS: [&str; 8] = ["Question zero", "Question one", "The same question", "Question three", "Question four", "The same question", "Question six", "Question seven"];

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

/// A test notary, the stand-in provider, an operator's home and a chain of the operator's own.
struct World {
    rig: Rig,
    log: String,
    ledger_key: String,
    children: Vec<Child>,
    settings: Vec<(String, String)>,
    signer: String,
    operator_transport: String,
    /// The version of the record and links this world writes.
    version: u32,
}

impl Drop for World {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Starts a program and reads the lines it prints before it serves.
fn start(children: &mut Vec<Child>, mut command: Command, fields: &[&str]) -> Vec<String> {
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
    children.push(child);
    values
}

impl World {
    fn new(label: &str) -> Self {
        Self::of_version(label, crate::CURRENT_VERSION)
    }

    fn of_version(label: &str, version: u32) -> Self {
        let rig = Rig::new(&format!("forgery-{label}"));
        let (log, ledger_key) = (rig.key("test-log"), rig.key("operator"));
        let home = rig.root.join("home");
        fs::create_dir_all(home.join(".config/hyalos-spike")).unwrap();
        fs::write(home.join(".config/hyalos-spike/anthropic.key"), format!("{}\n", "x".repeat(106))).unwrap();
        let transport_key = rig.root.join("operator-transport.key");
        let made = Command::new(built("attest")).arg("link-key").arg(&transport_key).output().unwrap();
        assert!(made.status.success());
        let operator_transport = String::from_utf8(made.stdout).unwrap().trim().to_string();

        let mut children = Vec::new();
        let root = rig.root.join("root.der");
        let mut standin = Command::new(built("standin"));
        standin.arg(&root).arg(NAME);
        let provider = start(&mut children, standin, &["listening"]).remove(0);
        let settings = vec![
            ("HOME".to_string(), home.display().to_string()),
            ("HYALOS_TRUST_SERVER_NAME".to_string(), NAME.to_string()),
            ("HYALOS_TRUST_ROOT".to_string(), root.display().to_string()),
            ("HYALOS_TRUST_PROVIDER_ADDRESS".to_string(), provider),
            ("HYALOS_OPERATOR_TRANSPORT_KEY".to_string(), transport_key.display().to_string()),
        ];
        let mut world = Self { rig, log, ledger_key, children, settings, signer: String::new(), operator_transport, version };
        let (signer, transport, address) = world.notary("notary");
        world.signer = signer;
        world.point_at(&transport, &address, &world.signer.clone());
        fs::write(world.rig.path("notary.pub"), format!("{}\n", world.signer)).unwrap();
        world
    }

    /// Starts a notary of its own directory, and gives its signing key, transport key and address.
    fn notary(&mut self, dir: &str) -> (String, String, String) {
        let mut command = Command::new(built("notary"));
        command
            .args(["--listen", "127.0.0.1:0", "--approved-server", NAME, "--stop-on-stdin-close", "--operator-transport-public", &self.operator_transport, "--dir"])
            .arg(self.rig.root.join(dir))
            .arg("--root")
            .arg(self.rig.root.join("root.der"));
        let mut values = start(&mut self.children, command, &["signing", "transport", "listening"]);
        let address = values.remove(2);
        let transport = values.remove(1);
        (values.remove(0), transport, address)
    }

    fn point_at(&mut self, transport: &str, address: &str, signer: &str) {
        self.settings.retain(|(name, _)| !name.starts_with("HYALOS_NOTARY_"));
        self.settings.push(("HYALOS_NOTARY_ADDRESS".to_string(), address.to_string()));
        self.settings.push(("HYALOS_NOTARY_TRANSPORT_PUBLIC".to_string(), transport.to_string()));
        self.settings.push(("HYALOS_NOTARY_SIGNER_PUBLIC".to_string(), signer.to_string()));
    }

    /// One real attested call through `ask`. `told` is the run the notary is told, when it is not
    /// the request's.
    fn ask(&self, file: &str, intent: &str, position: usize, question: &str, told: Option<&str>) -> Value {
        let mut command = Command::new(built("attest"));
        command.args(["ask", file, intent, &position.to_string()]).env_clear().envs(self.settings.iter().map(|(n, v)| (n, v)));
        if let Some(run) = told {
            command.env("HYALOS_TEST_TOLD_RUN", run);
        }
        let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(question.as_bytes()).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "ask refused: {}", String::from_utf8_lossy(&output.stdout));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn record(&self) -> String {
        self.rig.path("record")
    }

    fn presentation(&self, index: usize) -> String {
        format!("{}.attest/{index:03}.presentation", self.record())
    }

    fn scratch_file(&self, name: &str) -> String {
        self.rig.path(&format!("asked-{name}"))
    }

    /// The transcript's frame. The guest and input are labels: this audit does not run the guest.
    fn frame(&self) -> Transcript {
        let guest_sha256 = sha(b"the forger's guest");
        let task = if self.version == 1 { String::new() } else { crate::task_v2(&guest_sha256, &[b"one input".to_vec()]).unwrap() };
        Transcript {
            version: self.version,
            task,
            guest_sha256,
            inputs_hex: vec![crate::hex(b"one input")],
            answers: Vec::new(),
            effects: Vec::new(),
            result_hex: String::new(),
            fuel_used: 1,
        }
    }

    /// Logs an intent for the frame's task and gives its hash.
    fn declare(&self) -> String {
        let frame = self.frame();
        let inputs = vec![b"one input".to_vec()];
        let intent = if self.version == 1 {
            json!({"v": 1, "kind": "intent", "task": crate::task_of(&frame.guest_sha256, &inputs), "guest_sha256": frame.guest_sha256})
        } else {
            let key = public_der(&fs::read_to_string(self.rig.path("operator.pub")).unwrap()).unwrap();
            json!({"v": self.version, "kind": "intent", "task": frame.task, "guest_sha256": frame.guest_sha256, "operator": sha(&key)})
        };
        self.rig.push(&self.log, &self.ledger_key, intent)
    }

    /// Writes the record over these answers, files already in place, and logs its result link.
    /// The log then signs `intent_time` and `result_time` for the run's two links.
    fn close(&self, intent: &str, answers: Vec<Answer>, intent_time: u64, result_time: u64) {
        let mut t = self.frame();
        t.answers = answers;
        let salt = random_salt();
        fs::write(self.record(), serde_json::to_vec_pretty(&t).unwrap()).unwrap();
        fs::write(format!("{}.salt", self.record()), &salt).unwrap();
        let commitment = sha([salt.as_bytes(), &serde_json::to_vec(&t).unwrap()].concat().as_slice());
        self.rig.push(&self.log, &self.ledger_key, json!({"v": self.version, "kind": "result", "intent": intent, "outcome": "ok", "transcript": commitment}));
        self.sign_times(intent_time, result_time);
    }

    /// Has the stand-in log sign these times for the run's two links.
    fn sign_times(&self, intent_time: u64, result_time: u64) {
        for (seq, time) in [(0usize, intent_time), (1, result_time)] {
            let link = fs::read(self.rig.root.join(format!("ledger/{seq:05}.json"))).unwrap();
            let reply = self.rig.entry_at(&self.log, &self.ledger_key, &link, 100 + seq as u64, time);
            for dir in ["ledger", "log"] {
                fs::write(self.rig.root.join(format!("{dir}/{seq:05}.rekor.json")), &reply).unwrap();
            }
        }
    }

    fn audit(&self) -> Result<String, String> {
        self.audit_with(false)
    }

    fn audit_with(&self, legacy: bool) -> Result<String, String> {
        let inputs = AuditInputs {
            dir: Some(self.rig.path("ledger")),
            operator: Some(self.rig.path("operator.pub")),
            notary: self.signer.clone(),
            verify: built("attest").display().to_string(),
            legacy,
        };
        let trust = self.settings.iter().filter(|(name, _)| name.starts_with("HYALOS_TRUST_")).map(|(n, v)| (n.clone(), v.into())).collect();
        let verifier = Verifier { program: inputs.verify.clone(), trust };
        crate::audit_record(&self.record(), &inputs, Some((self.rig.path("log"), self.rig.path("test-log.pub"))), &verifier)
    }

    fn refused(&self) -> String {
        self.audit().expect_err("the audit must refuse this record")
    }

    /// Asks each position once, in order, with its presentation kept at that position.
    fn honest(&self, intent: &str, questions: &[&str]) -> Vec<Answer> {
        fs::create_dir_all(format!("{}.attest", self.record())).unwrap();
        questions.iter().enumerate().map(|(position, question)| {
            let file = self.presentation(position);
            answer_from(&self.ask(&file, intent, position, question, None), question)
        }).collect()
    }
}

fn answer_from(said: &Value, question: &str) -> Answer {
    Answer {
        schema: 0,
        question_sha256: sha(question.as_bytes()),
        label: said["label"].as_i64().unwrap() as i32,
        model: said["model"].as_str().unwrap().to_string(),
        response_id: said["response_id"].as_str().unwrap().to_string(),
        attestation: said["presentation_sha256"].as_str().unwrap().to_string(),
    }
}

/// An honest record of eight answers, in a window around its sessions.
fn honest_eight(label: &str) -> (World, String, Vec<Answer>, u64) {
    let world = World::new(label);
    let intent = world.declare();
    let before = now() - 1;
    let answers = world.honest(&intent, &QUESTIONS);
    let after = now() + 1;
    world.close(&intent, answers.clone(), before, after);
    (world, intent, answers, before)
}

#[test]
fn s_03_2_e1_s_03_3_e1_s_03_5_e1_s_04_3_e1_an_honest_record_of_eight_passes_and_others_are_refused() {
    let _one = locked();
    let (world, intent, answers, before) = honest_eight("honest");

    // S-03.2.e1, S-03.3.e1, S-03.5.e1, S-04.3.e1, S-04.5.e1.
    let report = world.audit().unwrap();
    assert!(report.contains("all 8 answers were attested by the trusted notary for this run and position"), "{report}");
    assert!(report.contains("test trust settings: the provider here is a stand-in"), "{report}");
    assert_eq!(report.lines().filter(|l| l.starts_with("not proven")).count(), 5, "{report}");
    assert!(!report.contains("not checked"), "{report}");

    // S-03.3.e2: the log signed the intent after the sessions.
    let (first, last) = session_times(&world, answers.len());
    world.sign_times(first, last + 1);
    assert_eq!(world.refused(), "answer 0: the session is not later than the run's intent", "a session in the intent's own second");
    world.sign_times(last + 1, last + 2);
    assert_eq!(world.refused(), "answer 0: the session is not later than the run's intent");
    // S-03.3.e3: the log signed the result before the last session.
    world.sign_times(first - 1, last - 1);
    assert!(world.refused().ends_with("the session is later than the run's result"));
    // The window's own edge: a session in the result's second is inside it.
    world.sign_times(first - 1, last);
    assert!(world.audit().is_ok());
    world.sign_times(before, last + 1);

    // S-03.2.e3: answers 2 and 5 ask the same question, and answer 5 reuses answer 2's presentation.
    let kept = fs::read(world.presentation(5)).unwrap();
    fs::copy(world.presentation(2), world.presentation(5)).unwrap();
    let mut reused = answers.clone();
    reused[5] = answers[2].clone();
    rewrite(&world, &intent, reused);
    assert_eq!(world.refused(), "answer 5: the binding names another position");
    fs::write(world.presentation(5), kept).unwrap();

    // S-04.3.e2: one answer names no presentation.
    let mut unnamed = answers.clone();
    unnamed[3].attestation.clear();
    rewrite(&world, &intent, unnamed);
    assert_eq!(world.refused(), "answer 3: the attestation is not the one the record names");

    // An answer the notary did not see, under its real presentation.
    let mut changed = answers.clone();
    changed[1].label = (changed[1].label + 1) % 4;
    rewrite(&world, &intent, changed);
    assert_eq!(world.refused(), "answer 1: the notary saw a different question or answer than the record claims");

    // A record with no answers proves nothing about any.
    rewrite(&world, &intent, Vec::new());
    assert_eq!(world.refused(), "this record has no attested answers");
}

/// Replaces the record and its result link, keeping the run's intent. The chain holds the new
/// result in place of the old one, so each variation meets the attestation checks.
fn rewrite(world: &World, intent: &str, answers: Vec<Answer>) {
    let times: Vec<u64> = [0, 1].iter().map(|seq| signed(world, *seq)).collect();
    for dir in ["ledger", "log"] {
        let _ = fs::remove_file(world.rig.root.join(format!("{dir}/00001.rekor.json")));
    }
    fs::remove_file(world.rig.root.join("ledger/00001.json")).unwrap();
    world.close(intent, answers, times[0], times[1]);
}

fn signed(world: &World, seq: usize) -> u64 {
    let reply: Value = serde_json::from_slice(&fs::read(world.rig.root.join(format!("ledger/{seq:05}.rekor.json"))).unwrap()).unwrap();
    reply.as_object().unwrap().values().next().unwrap()["integratedTime"].as_u64().unwrap()
}

/// The earliest and latest session times among the answers, as verify reports them.
fn session_times(world: &World, count: usize) -> (u64, u64) {
    let times: Vec<u64> = (0..count)
        .map(|index| {
            let output = Command::new(built("attest"))
                .args(["verify", &world.presentation(index), &world.signer])
                .env_clear()
                .envs(world.settings.iter().filter(|(name, _)| name.starts_with("HYALOS_TRUST_")).map(|(n, v)| (n, v)))
                .output()
                .unwrap();
            serde_json::from_slice::<Value>(&output.stdout).unwrap()["time"].as_u64().unwrap()
        })
        .collect();
    (*times.iter().min().unwrap(), *times.iter().max().unwrap())
}

#[test]
fn s_03_2_e2_s_04_4_e2_a_real_presentation_from_another_run_is_refused() {
    let _one = locked();
    let world = World::new("other-run");
    fs::create_dir_all(format!("{}.attest", world.record())).unwrap();
    let earlier = "ab".repeat(32);
    let said = world.ask(&world.presentation(0), &earlier, 0, QUESTIONS[0], None);
    let intent = world.declare();
    world.close(&intent, vec![answer_from(&said, QUESTIONS[0])], now() - 60, now() + 1);
    assert_eq!(world.refused(), "answer 0: the binding names another run");
}

#[test]
fn s_03_5_e2_a_question_asked_twice_with_the_first_answer_discarded_is_refused() {
    let _one = locked();
    let world = World::new("re-ask");
    let intent = world.declare();
    fs::create_dir_all(format!("{}.attest", world.record())).unwrap();
    let before = now() - 1;
    let mut answers = world.honest(&intent, &QUESTIONS[..3]);
    let _discarded = world.ask(&world.scratch_file("discarded"), &intent, 3, QUESTIONS[3], None);
    answers.push(answer_from(&world.ask(&world.presentation(3), &intent, 3, QUESTIONS[3], None), QUESTIONS[3]));
    world.close(&intent, answers, before, now() + 1);
    assert_eq!(world.refused(), "answer 3: the sequence number is not the answer's position, so an attestation is unaccounted for");
}

#[test]
fn s_03_5_e4_the_first_question_asked_three_times_is_refused() {
    let _one = locked();
    let world = World::new("re-ask-first");
    let intent = world.declare();
    fs::create_dir_all(format!("{}.attest", world.record())).unwrap();
    let before = now() - 1;
    for attempt in 0..2 {
        world.ask(&world.scratch_file(&format!("discarded-{attempt}")), &intent, 0, QUESTIONS[0], None);
    }
    let kept = answer_from(&world.ask(&world.presentation(0), &intent, 0, QUESTIONS[0], None), QUESTIONS[0]);
    world.close(&intent, vec![kept], before, now() + 1);
    assert_eq!(world.refused(), "answer 0: the sequence number is not the answer's position, so an attestation is unaccounted for");
}

#[test]
fn adr_014_a_session_left_after_reading_the_reply_cannot_be_asked_again() {
    let _one = locked();
    let world = World::new("left");
    let intent = world.declare();
    fs::create_dir_all(format!("{}.attest", world.record())).unwrap();
    let before = now() - 1;
    // The forger reads the answer, dislikes it, and leaves before the proof.
    let mut leaving = Command::new(built("attest"));
    leaving.args(["ask", &world.scratch_file("left"), &intent, "0"]).env_clear().envs(world.settings.iter().map(|(n, v)| (n, v))).env("HYALOS_TEST_LEAVE_AFTER_REPLY", "1");
    let mut child = leaving.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(QUESTIONS[0].as_bytes()).unwrap();
    let left = child.wait_with_output().unwrap();
    assert!(!left.status.success());
    let kept = answer_from(&world.ask(&world.presentation(0), &intent, 0, QUESTIONS[0], None), QUESTIONS[0]);
    world.close(&intent, vec![kept], before, now() + 1);
    assert_eq!(world.refused(), "answer 0: the sequence number is not the answer's position, so an attestation is unaccounted for");
}

#[test]
fn s_03_5_e5_an_operator_who_tells_the_notary_another_run_is_refused() {
    let _one = locked();
    let world = World::new("told");
    let intent = world.declare();
    fs::create_dir_all(format!("{}.attest", world.record())).unwrap();
    let before = now() - 1;
    let other = "cd".repeat(32);
    let said = world.ask(&world.presentation(0), &intent, 0, QUESTIONS[0], Some(&other));
    world.close(&intent, vec![answer_from(&said, QUESTIONS[0])], before, now() + 1);
    assert_eq!(world.refused(), "answer 0: the notary was told another run");
}

#[test]
fn s_04_4_e1_an_answer_attested_by_the_forgers_own_notary_is_refused() {
    let _one = locked();
    let mut world = World::new("own-notary");
    let trusted = world.signer.clone();
    let (forged, transport, address) = world.notary("forgers-notary");
    world.point_at(&transport, &address, &forged);
    let intent = world.declare();
    fs::create_dir_all(format!("{}.attest", world.record())).unwrap();
    let before = now() - 1;
    let said = world.ask(&world.presentation(0), &intent, 0, QUESTIONS[0], None);
    world.close(&intent, vec![answer_from(&said, QUESTIONS[0])], before, now() + 1);
    assert_eq!(world.signer, trusted);
    assert_eq!(world.refused(), "answer 0: not attested by the trusted notary");
}

#[test]
fn s_04_4_e3_an_attestation_obtained_before_the_intent_was_logged_is_refused() {
    let _one = locked();
    let world = World::new("before-intent");
    // The forger knows the intent's hash before logging it, and attests first.
    let intent = world.declare();
    fs::create_dir_all(format!("{}.attest", world.record())).unwrap();
    let said = world.ask(&world.presentation(0), &intent, 0, QUESTIONS[0], None);
    let (_, session) = session_times(&world, 1);
    world.close(&intent, vec![answer_from(&said, QUESTIONS[0])], session + 1, session + 5);
    assert_eq!(world.refused(), "answer 0: the session is not later than the run's intent");
}

#[test]
fn s_03_2_e4_an_attestation_whose_request_carries_no_binding_is_refused() {
    // What verify reports for a request with no binding has neither field. Verify itself refuses
    // such a request: `s_03_4_a_request_with_no_binding_or_a_loose_one_is_refused` in `attest/src/lib.rs`.
    let answer = Answer { schema: 0, question_sha256: "q".into(), label: 0, model: "m".into(), response_id: "r".into(), attestation: "a".into() };
    let seen = json!({"run": "r", "sequence": 0, "question_sha256": "q", "label": 0, "model": "m", "response_id": "r", "time": 5});
    assert_eq!(crate::check_answer(&seen, &answer, 0, "r", Some((1, 9))).unwrap_err(), "answer 0: the binding names another run");
}

#[test]
fn t_5_the_window_is_strict_at_the_intent_and_inclusive_at_the_result() {
    let answer = Answer { schema: 0, question_sha256: "q".into(), label: 0, model: "m".into(), response_id: "r".into(), attestation: "a".into() };
    let at = |time: u64| json!({"intent_hash": "r", "position": 0, "run": "r", "sequence": 0, "question_sha256": "q", "label": 0, "model": "m", "response_id": "r", "time": time});
    for (time, passes) in [(9, false), (10, false), (11, true), (20, true), (21, false)] {
        assert_eq!(crate::check_answer(&at(time), &answer, 0, "r", Some((10, 20))).is_ok(), passes, "{time}");
    }
}

#[test]
fn s_04_1_the_whole_audit_passes_with_an_empty_home() {
    let _one = locked();
    let world = World::new("cli");
    let intent = world.declare();
    let before = now() - 1;
    let answers = world.honest(&intent, &QUESTIONS[..1]);
    world.close(&intent, answers, before, now() + 1);
    let empty = world.rig.root.join("auditor-home");
    fs::create_dir_all(&empty).unwrap();
    let program = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("task-kernel");
    assert!(program.exists(), "the monitor is not built: run cargo build --release");
    let output = Command::new(program)
        .arg("audit")
        .arg(world.record())
        .arg(world.rig.path("ledger"))
        .args(["--operator-public", &world.rig.path("operator.pub"), "--notary-public", &world.rig.path("notary.pub"), "--verify"])
        .arg(built("attest"))
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap())
        .env("HOME", &empty)
        .env("HYALOS_KERNEL", "/nonexistent/hyalos-kernel")
        .env("HYALOS_TRUST_LOG_DIR", world.rig.path("log"))
        .env("HYALOS_TRUST_LOG_KEY", world.rig.path("test-log.pub"))
        .envs(world.settings.iter().filter(|(name, _)| name.starts_with("HYALOS_TRUST_")).map(|(n, v)| (n, v)))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let report = String::from_utf8(output.stdout).unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(report.starts_with("interim audit passed: attempt budget not yet checked\ntest trust settings"), "{report}");
    assert!(!report.contains("public log"), "a stand-in report claims nothing of the public log: {report}");
    assert_eq!(fs::read_dir(&empty).unwrap().count(), 0, "the home directory stays empty");
}

#[test]
fn t_5_1_the_monitor_names_no_notary_setting() {
    assert!(!include_str!("main.rs").contains(concat!("HYALOS_", "NOTARY")));
}

/// A world holding one honest attested answer in a record of `version`.
fn one_answer(label: &str, version: u32) -> World {
    let world = World::of_version(label, version);
    let intent = world.declare();
    let before = now() - 1;
    let answers = world.honest(&intent, &QUESTIONS[..1]);
    world.close(&intent, answers, before, now() + 1);
    world
}

/// The audit of a record file that its version check refuses, so no key or log is reached.
fn audit_file(record: &str, legacy: bool) -> Result<String, String> {
    let inputs = AuditInputs { dir: None, operator: Some("/nonexistent".into()), notary: format!("02{}", "11".repeat(32)), verify: "/nonexistent".into(), legacy };
    crate::audit_record(record, &inputs, None, &Verifier { program: "/nonexistent".into(), trust: Vec::new() })
}

fn record_of_version(rig: &Rig, version: u32) -> String {
    let record = rig.path("versioned");
    fs::write(&record, json!({"version": version, "guest_sha256": "g", "inputs_hex": [], "answers": [], "effects": [], "result_hex": "", "fuel_used": 1}).to_string()).unwrap();
    record
}

#[test]
fn record_s_01_1_e1_an_unknown_version_is_refused_and_named() {
    let rig = Rig::new("r-01-1-e1");
    let record = record_of_version(&rig, 3);
    assert_eq!(audit_file(&record, false).unwrap_err(), "record version 3 is not known");
    assert_eq!(crate::read_transcript(&fs::read(&record).unwrap()).unwrap_err(), "record version 3 is not known");
}

#[test]
fn record_s_01_1_e2_version_0_is_refused_even_with_the_legacy_option() {
    let rig = Rig::new("r-01-1-e2");
    assert_eq!(audit_file(&record_of_version(&rig, 0), true).unwrap_err(), "record version 0 is not known");
}

#[test]
fn record_s_01_2_e1_a_version_1_record_without_the_option_is_refused_naming_it() {
    let rig = Rig::new("r-01-2-e1");
    let refused = audit_file(&record_of_version(&rig, 1), false).unwrap_err();
    assert!(refused.contains("--legacy-v1"), "{refused}");
}

#[test]
fn record_s_01_2_e2_a_version_2_record_is_not_refused_for_its_version() {
    let _one = locked();
    assert!(one_answer("r-01-2-e2", 2).audit().is_ok());
}

#[test]
fn record_s_01_3_e1_a_valid_version_1_record_passes_as_legacy() {
    let _one = locked();
    let report = one_answer("r-01-3-e1", 1).audit_with(true).unwrap();
    assert_eq!(report.lines().next(), Some("legacy audit passed: no attempt budget"));
}

#[test]
fn record_s_01_3_e2_a_version_2_record_with_the_option_is_audited_as_version_2() {
    let _one = locked();
    let report = one_answer("r-01-3-e2", 2).audit_with(true).unwrap();
    assert_eq!(report.lines().next(), Some("interim audit passed: attempt budget not yet checked"));
    assert!(!report.contains("legacy"), "{report}");
}

#[test]
fn record_s_01_7_e1_a_legacy_pass_never_opens_with_audit_passed() {
    let _one = locked();
    let report = one_answer("r-01-7-e1", 1).audit_with(true).unwrap();
    assert!(!report.starts_with("audit passed"), "{report}");
}

#[test]
fn record_1_1_a_version_2_pass_says_the_budget_is_not_checked() {
    let _one = locked();
    let report = one_answer("r-1-1", 2).audit().unwrap();
    let (first, rest) = report.split_once('\n').unwrap();
    assert_eq!(first, "interim audit passed: attempt budget not yet checked");
    assert!(!first.starts_with("audit passed"));
    assert!(!rest.contains("attempt") && !rest.contains("ceiling"), "{report}");
}
