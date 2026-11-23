//! Slice 5: a forger who holds every operator key, against the real ask, a real test notary and the
//! stand-in provider. Chains are made here under test keys on a stand-in log, so each forgery passes
//! the log step and is refused by the check it is meant to meet. No test contacts the real provider
//! or the public log, and every key and credential is made up in a temporary directory.

use super::audit_tests::Rig;
use super::*;
use crate::live_tests::{built, locked, random_salt};
use crate::{Answer, AuditInputs, Backend, Mode, Transcript, Verifier};
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
    /// A real guest that asks the first questions of `QUESTIONS` and returns.
    guest: Vec<u8>,
    /// The authorisation on the notary last started.
    authorisation: String,
    /// Audit through `old-verify`, which reports a presentation as an older notary's, with no ticket.
    /// This notary always signs a ticket, and a version 1 record is from before tickets.
    old_verifier: bool,
    /// The task's one input.
    input: Vec<u8>,
    /// What the test-only `forging-notary` signs in place of the ticket.
    forge: Option<String>,
    /// What it signs in place of the pass. A version 1 or 2 record has none, so its world's notary
    /// signs none.
    forge_canary: Option<String>,
}

/// A guest that asks these questions once each, in order, then returns one byte.
fn asking(questions: &[&str]) -> Vec<u8> {
    let (mut data, mut calls, mut at) = (String::new(), String::new(), 1);
    for question in questions {
        data.push_str(&format!("(data (i32.const {at}) \"{question}\")"));
        calls.push_str(&format!("(drop (call $infer (i32.const 0) (i32.const {at}) (i32.const {})))", question.len()));
        at += question.len();
    }
    wat::parse_str(format!(
        r#"(module (import "hyalos" "infer" (func $infer (param i32 i32 i32) (result i32)))
           (import "hyalos" "return" (func $return (param i32 i32)))
           (memory (export "memory") 1) {data}
           (func (export "run") {calls} (call $return (i32.const 0) (i32.const 1))))"#
    ))
    .unwrap()
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
        Self::asking(label, crate::CURRENT_VERSION, QUESTIONS.len())
    }

    /// A world whose guest asks the first `asks` questions.
    fn asking(label: &str, version: u32, asks: usize) -> Self {
        Self::signing(label, version, asks, None)
    }

    /// A world whose notary signs the ticket `forge` names, when it names one.
    fn signing(label: &str, version: u32, asks: usize, forge: Option<&str>) -> Self {
        Self::signing_canary(label, version, asks, forge, None)
    }

    /// A world whose notary also signs the canary `canary` names.
    fn signing_canary(label: &str, version: u32, asks: usize, forge: Option<&str>, canary: Option<&str>) -> Self {
        let forge_canary = canary.or((version < 3).then_some("missing")).map(str::to_string);
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
        let mut world = Self { rig, log, ledger_key, children, settings, signer: String::new(), operator_transport, version, guest: asking(&QUESTIONS[..asks]), authorisation: String::new(), old_verifier: version == 1, input: b"one input".to_vec(), forge: forge.map(str::to_string), forge_canary };
        fs::write(world.rig.path("guest.wasm"), &world.guest).unwrap();
        let old = world.rig.path("old-verify");
        fs::write(&old, format!("#!/bin/sh\nout=$('{}' verify \"$2\" \"$3\") || exit $?\nprintf '%s\\n' \"$out\" | /usr/bin/sed 's/\"ticket\":{{[^}}]*}}/\"ticket\":null/'\n", built("attest").display())).unwrap();
        fs::set_permissions(&old, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
        let (signer, transport, address) = world.notary("notary");
        world.signer = signer;
        world.point_at(&transport, &address, &world.signer.clone());
        fs::write(world.rig.path("notary.pub"), format!("{}\n", world.signer)).unwrap();
        world
    }

    /// Starts a notary of its own directory, and gives its signing key, transport key and address.
    fn notary(&mut self, dir: &str) -> (String, String, String) {
        crate::live_tests::canary_pass(&self.rig.root.join(dir));
        let mut command = Command::new(built(if self.forge.is_some() || self.forge_canary.is_some() { "forging-notary" } else { "notary" }));
        if let Some(forge) = &self.forge {
            command.env("HYALOS_FORGE_TICKET", forge);
        }
        if let Some(canary) = &self.forge_canary {
            command.env("HYALOS_FORGE_CANARY", canary);
        }
        command
            .args(["--listen", "127.0.0.1:0", "--approved-server", NAME, "--stop-on-stdin-close", "--operator-transport-public", &self.operator_transport, "--dir"])
            .arg(self.rig.root.join(dir))
            .arg("--root")
            .arg(self.rig.root.join("root.der"));
        let mut values = start(&mut self.children, command, &["signing", "transport", "listening"]);
        let task = crate::task_v2(&sha(&self.guest), &[self.input.clone()]).unwrap();
        let made = Command::new(built("notary")).args(["authorize", "--dir"]).arg(self.rig.root.join(dir)).args(["--task", &task, "--attempts", "100"]).output().unwrap();
        assert!(made.status.success());
        self.authorisation = String::from_utf8(made.stdout).unwrap().trim().to_string();
        let address = values.remove(2);
        let transport = values.remove(1);
        (values.remove(0), transport, address)
    }

    fn point_at(&mut self, transport: &str, address: &str, signer: &str) {
        self.settings.retain(|(name, _)| !name.starts_with("HYALOS_NOTARY_"));
        self.settings.push(("HYALOS_NOTARY_ADDRESS".to_string(), address.to_string()));
        self.settings.push(("HYALOS_NOTARY_TRANSPORT_PUBLIC".to_string(), transport.to_string()));
        self.settings.push(("HYALOS_NOTARY_SIGNER_PUBLIC".to_string(), signer.to_string()));
        self.settings.push(("HYALOS_NOTARY_AUTHORISATION".to_string(), self.authorisation.clone()));
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

    /// One call that may be refused, keeping any presentation the tool's own check refuses.
    fn try_ask(&self, file: &str, intent: &str, position: usize, question: &str) -> std::process::Output {
        let mut command = Command::new(built("attest"));
        command.args(["ask", file, intent, &position.to_string()]).env_clear().envs(self.settings.iter().map(|(n, v)| (n, v))).env("HYALOS_TEST_KEEP_PRESENTATION", "1");
        let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(question.as_bytes()).unwrap();
        child.wait_with_output().unwrap()
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

    /// The transcript's frame, before a run's answers and what the guest did with them.
    fn frame(&self) -> Transcript {
        let guest_sha256 = sha(&self.guest);
        let task = if self.version == 1 { String::new() } else { crate::task_v2(&guest_sha256, &[self.input.clone()]).unwrap() };
        Transcript {
            version: self.version,
            task,
            guest_sha256,
            inputs_hex: vec![crate::hex(&self.input)],
            answers: Vec::new(),
            effects: Vec::new(),
            result_hex: String::new(),
            fuel_used: 1,
        }
    }

    /// Logs an intent for the frame's task and gives its hash.
    fn declare(&self) -> String {
        let frame = self.frame();
        let inputs = vec![self.input.clone()];
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
        // What the guest does with these answers, when it can run on them. A record it cannot
        // run on keeps the frame's, and a forgery that reaches replay diverges there.
        let mode = Mode::Replay { answers: answers.clone(), effects: Vec::new() };
        if let Ok(run) = crate::execute_as(self.version, &Backend::Process, &self.guest, vec![self.input.clone()], mode) {
            (t.result_hex, t.fuel_used) = (run.result_hex, run.fuel_used);
        }
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
        self.audit_on(legacy, Some(self.rig.path("guest.wasm")))
    }

    fn audit_on(&self, legacy: bool, guest: Option<String>) -> Result<String, String> {
        let inputs = AuditInputs {
            dir: Some(self.rig.path("ledger")),
            operator: Some(self.rig.path("operator.pub")),
            notary: self.signer.clone(),
            verify: if self.old_verifier { self.rig.path("old-verify") } else { built("attest").display().to_string() },
            legacy,
            guest,
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
        provider_proofs: Vec::new(),
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
    assert_eq!(report.lines().filter(|l| l.starts_with("not proven")).count(), 8, "{report}");
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
    let answer = Answer { schema: 0, question_sha256: "q".into(), label: 0, model: "m".into(), response_id: "r".into(), attestation: "a".into(), provider_proofs: Vec::new() };
    let seen = json!({"run": "r", "sequence": 0, "question_sha256": "q", "label": 0, "model": "m", "response_id": "r", "time": 5});
    assert_eq!(crate::check_answer(&seen, &answer, 0, "r", Some((1, 9))).unwrap_err(), "answer 0: the binding names another run");
}

#[test]
fn t_5_the_window_is_strict_at_the_intent_and_inclusive_at_the_result() {
    let answer = Answer { schema: 0, question_sha256: "q".into(), label: 0, model: "m".into(), response_id: "r".into(), attestation: "a".into(), provider_proofs: Vec::new() };
    let at = |time: u64| json!({"intent_hash": "r", "position": 0, "run": "r", "sequence": 0, "question_sha256": "q", "label": 0, "model": "m", "response_id": "r", "time": time});
    for (time, passes) in [(9, false), (10, false), (11, true), (20, true), (21, false)] {
        assert_eq!(crate::check_answer(&at(time), &answer, 0, "r", Some((10, 20))).is_ok(), passes, "{time}");
    }
}

#[test]
fn s_04_1_the_whole_audit_passes_with_an_empty_home() {
    let _one = locked();
    let world = World::asking("cli", crate::CURRENT_VERSION, 1);
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
        .args(["--operator-public", &world.rig.path("operator.pub"), "--notary-public", &world.rig.path("notary.pub"), "--guest", &world.rig.path("guest.wasm"), "--verify"])
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
    assert!(report.starts_with("audit passed\ntest trust settings"), "{report}");
    assert!(!report.contains("public log"), "a stand-in report claims nothing of the public log: {report}");
    assert_eq!(fs::read_dir(&empty).unwrap().count(), 0, "the home directory stays empty");
}

#[test]
fn t_5_1_the_monitor_names_no_notary_setting() {
    assert!(!include_str!("main.rs").contains(concat!("HYALOS_", "NOTARY")));
}

/// A world holding one honest attested answer in a record of `version`.
fn one_answer(label: &str, version: u32) -> World {
    let world = World::asking(label, version, 1);
    let intent = world.declare();
    let before = now() - 1;
    let answers = world.honest(&intent, &QUESTIONS[..1]);
    world.close(&intent, answers, before, now() + 1);
    world
}

/// The audit of a record file that its version check refuses, so no key or log is reached.
fn audit_file(record: &str, legacy: bool) -> Result<String, String> {
    let inputs = AuditInputs { dir: None, operator: Some("/nonexistent".into()), notary: format!("02{}", "11".repeat(32)), verify: "/nonexistent".into(), legacy, guest: None };
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
    let record = record_of_version(&rig, 4);
    assert_eq!(audit_file(&record, false).unwrap_err(), "record version 4 is not known");
    assert_eq!(crate::read_transcript(&fs::read(&record).unwrap()).unwrap_err(), "record version 4 is not known");
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
    assert_eq!(report.lines().next(), Some("audit passed"));
    assert!(!report.contains("legacy"), "{report}");
}

#[test]
fn record_s_01_7_e1_a_legacy_pass_never_opens_with_audit_passed() {
    let _one = locked();
    let report = one_answer("r-01-7-e1", 1).audit_with(true).unwrap();
    assert!(!report.starts_with("audit passed"), "{report}");
}

#[test]
fn record_s_01_7_e2_a_version_2_pass_opens_with_audit_passed() {
    let _one = locked();
    let report = one_answer("r-01-7-e2", 2).audit().unwrap();
    assert_eq!(report.lines().next(), Some("audit passed"));
}

#[test]
fn record_s_05_8_e1_a_version_2_audit_with_no_guest_is_refused() {
    let _one = locked();
    let world = one_answer("r-05-8-e1", 2);
    assert_eq!(world.audit_on(false, None).unwrap_err(), "the guest is not given: --guest <file>");
}

#[test]
fn record_s_05_8_e2_a_guest_with_one_byte_changed_is_refused() {
    let _one = locked();
    let world = one_answer("r-05-8-e2", 2);
    let mut changed = world.guest.clone();
    *changed.last_mut().unwrap() ^= 1;
    let file = world.rig.path("changed.wasm");
    fs::write(&file, changed).unwrap();
    assert_eq!(world.audit_on(false, Some(file)).unwrap_err(), "the guest is not this record's");
}

#[test]
fn record_s_05_8_e3_the_records_guest_and_a_record_that_replays_are_not_refused() {
    let _one = locked();
    assert!(one_answer("r-05-8-e3", 2).audit().is_ok());
}

#[test]
fn record_s_05_7_e6_extra_sessions_written_into_the_record_diverge() {
    let _one = locked();
    let world = World::asking("r-05-7-e6", 2, 1);
    let intent = world.declare();
    let before = now() - 1;
    // Two real attested sessions for a guest that asks one question.
    let answers = world.honest(&intent, &QUESTIONS[..2]);
    world.close(&intent, answers, before, now() + 1);
    assert_eq!(world.refused(), "the record does not replay on its guest");
}

#[test]
fn record_s_05_7_e7_a_reworded_question_diverges() {
    let _one = locked();
    let world = World::asking("r-05-7-e7", 2, 1);
    let intent = world.declare();
    let before = now() - 1;
    let answers = world.honest(&intent, &["Question zero, reworded"]);
    world.close(&intent, answers, before, now() + 1);
    assert_eq!(world.refused(), "the record does not replay on its guest");
}

#[test]
fn record_2_1_a_version_1_record_needs_no_guest() {
    let _one = locked();
    let report = one_answer("r-2-1", 1).audit_on(true, None).unwrap();
    assert_eq!(report.lines().next(), Some("legacy audit passed: no attempt budget"));
}

#[test]
fn record_s_01_6_e1_a_ticketed_attestation_in_a_version_1_record_is_refused() {
    let _one = locked();
    let mut world = one_answer("r-01-6-e1", 1);
    world.old_verifier = false;
    assert_eq!(world.audit_with(true).unwrap_err(), "answer 0: a version 1 record cannot carry a ticket");
}

#[test]
fn record_s_01_6_e2_a_version_1_record_with_no_ticket_passes_as_legacy() {
    let _one = locked();
    let report = one_answer("r-01-6-e2", 1).audit_with(true).unwrap();
    assert_eq!(report.lines().next(), Some("legacy audit passed: no attempt budget"));
}

/// A version 2 run of `asks` answers against a notary that signs the ticket `forge` names. A
/// presentation the tool's own check refuses is still kept and named by the record.
fn forged(label: &str, asks: usize, forge: &str) -> World {
    forged_world(World::signing(label, 2, asks, Some(forge)), asks)
}

/// One record of `asks` answers from this world's notary, kept even when the tool refuses them.
fn forged_world(world: World, asks: usize) -> World {
    let intent = world.declare();
    let before = now() - 1;
    fs::create_dir_all(format!("{}.attest", world.record())).unwrap();
    let answers = QUESTIONS[..asks].iter().enumerate().map(|(position, question)| {
        let file = world.presentation(position);
        let output = world.try_ask(&file, &intent, position, question);
        if output.status.success() {
            answer_from(&serde_json::from_slice(&output.stdout).unwrap(), question)
        } else {
            Answer { schema: 0, question_sha256: sha(question.as_bytes()), label: 0, model: String::new(), response_id: String::new(), attestation: sha(&fs::read(&file).unwrap()), provider_proofs: Vec::new() }
        }
    }).collect();
    world.close(&intent, answers, before, now() + 1);
    world
}

#[test]
fn record_s_05_1_e1_no_ticket_extension_is_refused() {
    let _one = locked();
    assert_eq!(forged("r-05-1-e1", 1, "missing").refused(), "answer 0: the attestation carries no ticket");
}

#[test]
fn record_s_05_1_e2_two_ticket_extensions_are_refused() {
    let _one = locked();
    assert_eq!(forged("r-05-1-e2", 1, "doubled").refused(), "answer 0: the attestation does not verify");
}

#[test]
fn record_s_05_1_e3_a_ticket_one_byte_short_is_refused() {
    let _one = locked();
    assert_eq!(forged("r-05-1-e3", 1, "short").refused(), "answer 0: the attestation does not verify");
}

#[test]
fn record_s_05_2_e1_another_ticket_on_a_later_answer_is_refused() {
    let _one = locked();
    assert_eq!(forged("r-05-2-e1", 2, "later:ticket=1").refused(), "answer 1: the ticket is not the one answer 0 carries");
}

#[test]
fn record_s_05_2_e2_another_authorisation_on_a_later_answer_is_refused() {
    let _one = locked();
    let forge = format!("later:authorisation={}", "c".repeat(64));
    assert_eq!(forged("r-05-2-e2", 2, &forge).refused(), "answer 1: the ticket is not the one answer 0 carries");
}

#[test]
fn record_s_05_2_e3_answers_with_the_same_values_are_not_refused() {
    let _one = locked();
    let world = World::asking("r-05-2-e3", 2, 2);
    let intent = world.declare();
    let before = now() - 1;
    let answers = world.honest(&intent, &QUESTIONS[..2]);
    world.close(&intent, answers, before, now() + 1);
    assert!(world.audit().is_ok());
}

#[test]
fn record_s_05_3_e1_a_ticket_for_another_task_is_refused() {
    let _one = locked();
    let forge = format!("task={}", "d".repeat(64));
    assert_eq!(forged("r-05-3-e1", 1, &forge).refused(), "the ticket's task is not this record's");
}

#[test]
fn record_s_05_3_e2_a_ticket_for_the_records_task_is_not_refused() {
    let _one = locked();
    assert!(one_answer("r-05-3-e2", 2).audit().is_ok());
}

#[test]
fn record_s_05_4_e1_ticket_2_of_budget_2_is_refused() {
    let _one = locked();
    assert_eq!(forged("r-05-4-e1", 1, "budget=2,ticket=2").refused(), "answer 0: the ticket is not inside its budget");
}

#[test]
fn record_s_05_4_e2_budget_0_is_refused() {
    let _one = locked();
    assert_eq!(forged("r-05-4-e2", 1, "budget=0").refused(), "answer 0: the ticket is not inside its budget");
}

#[test]
fn record_s_05_4_e3_budget_101_is_refused() {
    let _one = locked();
    assert_eq!(forged("r-05-4-e3", 1, "budget=101").refused(), "answer 0: the ticket is not inside its budget");
}

#[test]
fn record_s_05_5_e1_the_report_states_the_first_attempt() {
    let _one = locked();
    let report = forged("r-05-5-e1", 1, "budget=1").audit().unwrap();
    assert!(report.contains("attempt 1 of at most 1 under this authorisation, 0 authorised earlier, for this exact task on this notary"), "{report}");
}

#[test]
fn record_s_05_5_e2_the_report_states_a_later_attempt_and_earlier_budget() {
    let _one = locked();
    let report = forged("r-05-5-e2", 1, "budget=5,round=1,prior=3,ticket=2").audit().unwrap();
    assert!(report.contains("attempt 3 of at most 5 under this authorisation, 3 authorised earlier, for this exact task on this notary"), "{report}");
}

#[test]
fn record_s_05_6_e1_a_pass_names_the_seven_limits() {
    let _one = locked();
    let report = one_answer("r-05-6-e1", 2).audit().unwrap();
    let limits: Vec<&str> = report.lines().filter(|line| line.starts_with("not proven")).collect();
    assert_eq!(limits.len(), 8, "{report}");
    assert_eq!(limits[7], "not proven: this record has no provider canary");
    assert_eq!(limits[5], "not proven: the operator may have asked the provider directly, away from the notary");
    assert_eq!(limits[6], "not proven: the customer did not authorise this task again after this attestation");
}

#[test]
fn record_s_05_6_e2_a_failing_record_states_no_proof() {
    let _one = locked();
    let refused = forged("r-05-6-e2", 1, "missing").refused();
    assert!(!refused.contains("proven") && !refused.contains("audit passed"), "{refused}");
}

#[test]
fn record_s_05_7_e1_an_attestation_from_another_run_is_refused() {
    let _one = locked();
    let world = World::asking("r-05-7-e1", 2, 1);
    let intent = world.declare();
    let before = now() - 1;
    fs::create_dir_all(format!("{}.attest", world.record())).unwrap();
    let said = world.ask(&world.presentation(0), &"a".repeat(64), 0, QUESTIONS[0], None);
    world.close(&intent, vec![answer_from(&said, QUESTIONS[0])], before, now() + 1);
    assert_eq!(world.refused(), "answer 0: the binding names another run");
}

#[test]
fn record_s_05_7_e2_a_varied_input_under_the_original_authorisation_is_refused() {
    let _one = locked();
    let mut world = World::asking("r-05-7-e2", 2, 1);
    // The notary authorised the original input. The record and its links name another.
    world.input = b"a varied input".to_vec();
    let intent = world.declare();
    let before = now() - 1;
    let answers = world.honest(&intent, &QUESTIONS[..1]);
    world.close(&intent, answers, before, now() + 1);
    assert_eq!(world.refused(), "the ticket's task is not this record's");
}

#[test]
fn record_s_05_7_e3_presenting_work_as_version_1_avoids_no_budget() {
    let _one = locked();
    let mut world = one_answer("r-05-7-e3", 1);
    assert!(world.audit().unwrap_err().contains("--legacy-v1"));
    // With the option, what is truly ticketless reports that it has no budget.
    let report = world.audit_with(true).unwrap();
    assert_eq!(report.lines().next(), Some("legacy audit passed: no attempt budget"));
    // The real verifier sees this notary's ticket, so the same record is refused.
    world.old_verifier = false;
    assert_eq!(world.audit_with(true).unwrap_err(), "answer 0: a version 1 record cannot carry a ticket");
}

#[test]
fn record_s_05_7_e4_a_second_log_key_gets_no_more_attempts() {
    let _one = locked();
    let mut world = World::asking("r-05-7-e4", 2, 1);
    let made = Command::new(built("notary"))
        .args(["authorize", "--dir"])
        .arg(world.rig.root.join("notary"))
        .args(["--task", &"e".repeat(64), "--attempts", "1"])
        .output()
        .unwrap();
    assert!(made.status.success());
    let identifier = String::from_utf8(made.stdout).unwrap().trim().to_string();
    world.settings.retain(|(name, _)| name != "HYALOS_NOTARY_AUTHORISATION");
    world.settings.push(("HYALOS_NOTARY_AUTHORISATION".to_string(), identifier));
    // The notary never sees a log key: a run under a second key is one more new run to it.
    let first = world.try_ask(&world.scratch_file("first"), &"1".repeat(64), 0, QUESTIONS[0]);
    assert!(first.status.success());
    let second = world.try_ask(&world.scratch_file("second"), &"2".repeat(64), 0, QUESTIONS[0]);
    assert!(!second.status.success());
    assert_eq!(String::from_utf8_lossy(&second.stdout).trim(), "spent");
}

#[test]
fn record_s_05_7_e5_an_edited_budget_in_a_stored_attestation_is_refused() {
    let _one = locked();
    let world = World::asking("r-05-7-e5", 2, 1);
    let intent = world.declare();
    let before = now() - 1;
    let mut answers = world.honest(&intent, &QUESTIONS[..1]);
    world.close(&intent, answers.clone(), before, now() + 1);
    let mut bytes = fs::read(world.presentation(0)).unwrap();
    let at = bytes.windows(64).position(|w| w == world.authorisation.as_bytes()).expect("the ticket's bytes are in the presentation");
    bytes[at + 128 + 3] = 101;
    fs::write(world.presentation(0), &bytes).unwrap();
    answers[0].attestation = sha(&bytes);
    rewrite(&world, &intent, answers);
    assert_eq!(world.refused(), "answer 0: the attestation does not verify");
}

#[test]
fn record_s_05_7_e8_a_run_under_another_log_key_cannot_back_this_chain() {
    let _one = locked();
    let world = World::asking("r-05-7-e8", 2, 1);
    let intent = world.declare();
    let before = now() - 1;
    // The hash this run would have had in a chain under a second key.
    let link = fs::read_to_string(world.rig.root.join("ledger/00000.json")).unwrap();
    let key = public_der(&fs::read_to_string(world.rig.path("operator.pub")).unwrap()).unwrap();
    assert!(link.contains(&sha(&key)));
    let other = sha(link.replace(&sha(&key), &sha(b"a second operator key")).as_bytes());
    fs::create_dir_all(format!("{}.attest", world.record())).unwrap();
    let said = world.ask(&world.presentation(0), &other, 0, QUESTIONS[0], None);
    world.close(&intent, vec![answer_from(&said, QUESTIONS[0])], before, now() + 1);
    assert_eq!(world.refused(), "answer 0: the binding names another run");
}

const AN_UNKNOWN_KIND: &str = "example.provider/proof-9";

fn a_proof() -> crate::ProviderProof {
    crate::ProviderProof { kind: AN_UNKNOWN_KIND.to_string(), sha256: "ab".repeat(32) }
}

/// A world holding one attested answer that names a provider proof.
fn answer_with_a_proof(label: &str) -> (World, String, Vec<Answer>) {
    let world = World::asking(label, 2, 1);
    let intent = world.declare();
    let before = now() - 1;
    let mut answers = world.honest(&intent, &QUESTIONS[..1]);
    answers[0].provider_proofs = vec![a_proof()];
    world.close(&intent, answers.clone(), before, now() + 1);
    (world, intent, answers)
}

/// A record's bytes, otherwise well formed, whose one answer holds these proof entries.
fn record_with_proofs(version: u32, proofs: Value) -> Vec<u8> {
    let guest = sha(b"a guest");
    let mut record = json!({"version": version, "guest_sha256": guest, "inputs_hex": [], "effects": [], "result_hex": "", "fuel_used": 1,
        "answers": [{"schema": 0, "question_sha256": "q", "label": 0, "model": "m", "response_id": "r", "provider_proofs": proofs}]});
    if version >= 2 {
        record["task"] = json!(crate::task_v2(&guest, &[]).unwrap());
    }
    serde_json::to_vec(&record).unwrap()
}

#[test]
fn record_s_06_1_e1_an_answer_with_no_proof_has_no_slot_in_its_bytes() {
    let _one = locked();
    let world = one_answer("r-06-1-e1", 2);
    let written = fs::read_to_string(world.record()).unwrap();
    assert!(written.contains("\"attestation\""));
    assert!(!written.contains("provider_proofs"), "{written}");
}

#[test]
fn record_s_06_1_e2_a_version_1_record_replays_byte_identically() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let bytes = fs::read(root.join("evidence/out/live3.json")).unwrap();
    let t = crate::read_transcript(&bytes).unwrap();
    assert_eq!(t.version, 1);
    assert!(!serde_json::to_string(&t).unwrap().contains("provider_proofs"));
    crate::replay(&Backend::Process, &fs::read(root.join("guest/digest.wasm")).unwrap(), &t).unwrap();
}

#[test]
fn record_s_06_2_e1_an_unknown_proof_passes_and_is_counted_without_its_kind() {
    let _one = locked();
    let (world, ..) = answer_with_a_proof("r-06-2-e1");
    let report = world.audit().unwrap();
    assert_eq!(report.lines().next(), Some("audit passed"));
    assert!(report.contains("unchecked provider proofs: 1, which prove nothing"), "{report}");
    assert!(!report.contains(AN_UNKNOWN_KIND) && !report.contains("example.provider"), "{report}");
}

#[test]
fn record_s_06_2_e2_a_proof_does_not_rescue_a_record_that_fails_another_check() {
    let _one = locked();
    let (world, intent, mut answers) = answer_with_a_proof("r-06-2-e2");
    answers[0].label = (answers[0].label + 1) % 4;
    rewrite(&world, &intent, answers);
    assert_eq!(world.refused(), "answer 0: the notary saw a different question or answer than the record claims");
}

#[test]
fn record_s_06_3_e1_a_proof_with_a_bad_kind_or_shape_is_refused() {
    let digest = "ab".repeat(32);
    assert!(crate::read_transcript(&record_with_proofs(2, json!([{"kind": "a.kind", "sha256": digest}]))).is_ok(), "the control");
    for kind in [String::new(), "x".repeat(65), "Upper".to_string(), "a kind".to_string(), "kind\n".to_string()] {
        let refused = crate::read_transcript(&record_with_proofs(2, json!([{"kind": kind, "sha256": digest}]))).unwrap_err();
        assert_eq!(refused, "a provider proof is malformed");
    }
    assert_eq!(crate::read_transcript(&record_with_proofs(2, json!([{"kind": "a.kind", "sha256": digest, "extra": 1}]))).unwrap_err(), "cannot parse");
    assert_eq!(crate::read_transcript(&record_with_proofs(1, json!([{"kind": "a.kind", "sha256": digest}]))).unwrap_err(), "a version 1 record cannot carry a provider proof");
}

#[test]
fn record_s_06_3_e2_a_proof_whose_digest_is_not_64_lowercase_hex_is_refused() {
    for digest in ["ab".repeat(31), "ab".repeat(33), "AB".repeat(32), "zz".repeat(32)] {
        let refused = crate::read_transcript(&record_with_proofs(2, json!([{"kind": "a.kind", "sha256": digest}]))).unwrap_err();
        assert_eq!(refused, "a provider proof is malformed");
    }
}

#[test]
fn record_s_06_4_e1_a_proof_does_not_remove_the_model_name_limit() {
    let _one = locked();
    let (world, ..) = answer_with_a_proof("r-06-4-e1");
    assert!(world.audit().unwrap().contains("not proven: the model's name is the provider's claim"));
}

#[test]
fn record_s_06_4_e2_with_no_proof_the_model_name_limit_is_stated() {
    let _one = locked();
    let report = one_answer("r-06-4-e2", 2).audit().unwrap();
    assert!(report.contains("not proven: the model's name is the provider's claim"), "{report}");
    assert!(report.contains("unchecked provider proofs: 0, which prove nothing"), "{report}");
}

/// A version 3 record of one answer whose notary signs the canary `canary` names.
fn canary_record(label: &str, canary: &str) -> World {
    forged_world(World::signing_canary(label, 3, 1, None, Some(canary)), 1)
}

const NO_FRESH_PASS: &str = "answer 0: the canary is not fresh at its session";

#[test]
fn canary_s_03_1_e1_a_pass_an_hour_before_its_session_passes() {
    let _one = locked();
    assert!(canary_record("canary-3-1-e1", "before=3600").audit().unwrap().starts_with("audit passed"));
}

#[test]
fn canary_s_03_1_e2_no_canary_is_refused_naming_the_answer() {
    let _one = locked();
    assert_eq!(canary_record("canary-3-1-e2", "missing").refused(), "answer 0: the attestation carries no canary");
}

#[test]
fn canary_s_03_1_e3_two_canaries_are_refused() {
    let _one = locked();
    assert_eq!(canary_record("canary-3-1-e3", "doubled").refused(), "answer 0: the attestation does not verify");
}

#[test]
fn canary_s_03_1_e4_a_canary_of_the_wrong_length_is_refused() {
    let _one = locked();
    assert_eq!(canary_record("canary-3-1-e4", "malformed").refused(), "answer 0: the attestation does not verify");
}

#[test]
fn canary_s_03_1_e5_a_pass_after_its_session_is_refused() {
    let _one = locked();
    assert_eq!(canary_record("canary-3-1-e5", &format!("at={}", now() + 3600)).refused(), NO_FRESH_PASS);
}

#[test]
fn canary_s_03_1_e6_a_pass_a_second_past_the_window_is_refused() {
    let _one = locked();
    assert_eq!(canary_record("canary-3-1-e6", "before=86401").refused(), NO_FRESH_PASS);
}

#[test]
fn canary_s_03_1_e7_a_pass_exactly_at_the_window_passes() {
    let _one = locked();
    assert!(canary_record("canary-3-1-e7", "before=86400").audit().unwrap().starts_with("audit passed"));
}

/// Writes the state a real notary reads for its next session.
fn set_pass(world: &World, checked: u64) {
    let path = world.rig.root.join("notary/canary");
    fs::write(path, format!("hyalos.canary/1\nstate passed\nchecked {checked}\nend\n")).unwrap();
}

#[test]
fn canary_s_03_2_e1_the_report_names_the_oldest_pass() {
    let _one = locked();
    let world = World::asking("canary-3-2-e1", 3, 2);
    let intent = world.declare();
    let before = now() - 1;
    let (older, newer) = (now() - 7200, now() - 60);
    fs::create_dir_all(format!("{}.attest", world.record())).unwrap();
    let answers = [older, newer].iter().enumerate().map(|(position, pass)| {
        set_pass(&world, *pass);
        answer_from(&world.ask(&world.presentation(position), &intent, position, QUESTIONS[position], None), QUESTIONS[position])
    }).collect();
    world.close(&intent, answers, before, now() + 1);
    let report = world.audit().unwrap();
    let line = format!("provider canary: forged body refused at {} UTC, the oldest pass in this record, checked by the customer on this notary", crate::utc(older));
    assert!(report.lines().any(|l| l == line), "{report}");
    let (attempt, canary) = (report.lines().position(|l| l.starts_with("attempt ")).unwrap(), report.lines().position(|l| l == line).unwrap());
    assert_eq!(canary, attempt + 1, "{report}");
}

#[test]
fn canary_s_03_2_e2_a_pass_keeps_the_hidden_bytes_limit_and_adds_its_own() {
    let _one = locked();
    let report = one_answer("canary-3-2-e2", 3).audit().unwrap();
    assert!(report.lines().any(|l| l == crate::HIDDEN_BYTES_LIMIT), "{report}");
    assert!(report.lines().any(|l| l == "not proven: the canary tested one forged shape, with the customer's credential, at the time stated"), "{report}");
    assert!(!report.contains("no provider canary"), "{report}");
}

#[test]
fn canary_s_03_3_e1_a_version_2_record_states_it_has_no_canary() {
    let _one = locked();
    let committed = include_bytes!("../../evidence/out/two-machines-record/record.json");
    assert_eq!(crate::read_transcript(committed).unwrap().version, 2, "the committed record");
    let report = one_answer("canary-3-3-e1", 2).audit().unwrap();
    assert!(report.starts_with("audit passed"), "{report}");
    assert!(report.lines().any(|l| l == "not proven: this record has no provider canary"), "{report}");
    assert!(!report.contains("provider canary: forged"), "{report}");
}

#[test]
fn canary_s_03_3_e2_a_version_2_record_cannot_carry_a_canary() {
    let _one = locked();
    let world = forged_world(World::signing_canary("canary-3-3-e2", 2, 1, None, Some("before=60")), 1);
    assert_eq!(world.refused(), "answer 0: a version 2 record cannot carry a canary");
}

#[test]
fn canary_s_03_3_e3_a_version_1_record_keeps_its_legacy_audit() {
    let _one = locked();
    let report = one_answer("canary-3-3-e3", 1).audit_with(true).unwrap();
    assert!(report.starts_with("legacy audit passed: no attempt budget"), "{report}");
    assert!(!report.contains("canary"), "{report}");
}

#[test]
fn canary_a_version_1_record_cannot_carry_a_canary() {
    let _one = locked();
    let world = forged_world(World::signing_canary("canary-v1", 1, 1, None, Some("before=60")), 1);
    assert_eq!(world.audit_with(true).unwrap_err(), "answer 0: a version 1 record cannot carry a canary");
}

#[test]
fn canary_3_1_the_monitors_window_is_the_notarys() {
    let line = format!("pub const CANARY_WINDOW_SECONDS: u64 = {};", "86_400");
    assert!(include_str!("../../attest/src/lib.rs").contains(&line));
    assert_eq!(crate::CANARY_WINDOW_SECONDS, 86_400);
}

#[test]
fn canary_3_2_provider_proofs_are_checked_on_version_3() {
    let good = crate::read_transcript(&record_with_proofs(3, json!([{"kind": AN_UNKNOWN_KIND, "sha256": "ab".repeat(32)}])));
    assert_eq!(good.unwrap().version, 3);
    let bad = crate::read_transcript(&record_with_proofs(3, json!([{"kind": "NOT A KIND", "sha256": "ab".repeat(32)}])));
    assert_eq!(bad.unwrap_err(), "a provider proof is malformed");
}

#[test]
fn canary_utc_dates_are_the_calendars() {
    assert_eq!(crate::utc(0), "1970-01-01 00:00:00");
    assert_eq!(crate::utc(951_782_400), "2000-02-29 00:00:00");
    assert_eq!(crate::utc(1_795_305_600), "2026-11-22 00:00:00");
    assert_eq!(crate::utc(1_795_305_600 + 86_399), "2026-11-22 23:59:59");
}
