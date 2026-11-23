//! The notary's cost report, against the stand-in provider. No test contacts the real one.

mod common;

use common::*;
use std::{fmt::Write as _, path::Path, process::{Command, Output}, time::{Duration, Instant}};

struct Rig {
    scratch: Scratch,
    who: Operator,
    standin: Standin,
    notary: Notary,
}

impl Rig {
    fn new(label: &str) -> Self {
        let scratch = Scratch::new(label);
        let who = operator(&scratch.0.join("home"));
        let standin = standin(&scratch.0, &[]);
        let notary = notary(&scratch.0.join("notary"), &who.public, &standin.root);
        Self { scratch, who, standin, notary }
    }

    /// Runs `count` attested calls and returns each call's wall time, one per line.
    fn sessions(&self, count: u64) -> String {
        let mut times = String::new();
        for position in 0..count {
            let command = tool(&self.who, &self.standin.root, &self.standin.address, &self.notary.address, &self.notary.transport, &self.notary.signing, &self.notary.authorisation);
            let started = Instant::now();
            let output = run_ask(command, &self.scratch.0.join("call.presentation"), INTENT, position);
            let elapsed = started.elapsed().as_secs_f64();
            assert_eq!(said(&output)["sequence"], position);
            writeln!(times, "{elapsed:.6}").unwrap();
        }
        self.wait_for(count);
        times
    }

    /// The usage file is rewritten just after the operator's call returns.
    fn wait_for(&self, attested: u64) {
        let wanted = format!("attested {attested}\n");
        let started = Instant::now();
        while !std::fs::read_to_string(self.notary.dir.join("usage")).unwrap_or_default().contains(&wanted) {
            assert!(started.elapsed() < Duration::from_secs(30), "the usage file did not reach {attested}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn file(&self, name: &str, text: &str) -> std::path::PathBuf {
        let path = self.scratch.0.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }
}

fn report(dir: &Path, extra: &[&Path], flags: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_notary"));
    command.arg("report").arg("--dir").arg(dir);
    for (flag, file) in flags.iter().zip(extra) {
        command.arg(flag).arg(file);
    }
    command.output().unwrap()
}

fn printed(output: &Output) -> String {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout.clone()).unwrap()
}

/// The number after a label, up to the next space.
fn figure(text: &str, label: &str) -> f64 {
    let at = text.find(label).unwrap_or_else(|| panic!("no {label:?} in {text}")) + label.len();
    text[at..].split_whitespace().next().unwrap().parse().unwrap()
}

#[test]
fn s_05_1_e1_s_05_2_t_6_1_forty_sessions_give_the_notarys_cost_and_the_operators_wall_time() {
    let _one = one_at_a_time();
    let rig = Rig::new("s-05-1-e1");
    let times = rig.sessions(40);
    let wall = rig.file("wall", &times);
    let real = rig.file("real-wall", "7.42\n");

    let full = printed(&report(&rig.notary.dir, &[&wall, &real], &["--wall", "--real-wall"]));
    println!("{full}");
    assert!(full.contains("sessions: 40 attested, 0 refused, in one life of the serving process"), "{full}");
    assert!(!full.contains("not yet"), "{full}");
    assert!(figure(&full, "notary CPU: ") > 0.0, "{full}");
    assert!(figure(&full, "notary peak memory: ") > 0.0, "{full}");
    assert!(figure(&full, "link bytes per attestation: ") > 0.0, "{full}");
    assert!(full.contains(" sent by the notary, "), "{full}");
    assert!(figure(&full, "without the real provider: ") > 0.0, "{full}");
    assert!(full.contains("s per call over 40 calls"), "{full}");
    assert!(full.contains("with the real provider: 7.420 s per call over 1 call(s)"), "{full}");

    // S-05.1.e1: without a file it says the wall time was not supplied.
    let bare = printed(&report(&rig.notary.dir, &[], &[]));
    assert!(bare.contains("without the real provider: not supplied"), "{bare}");
    assert!(bare.contains("with the real provider: not supplied"), "{bare}");

    // A file that does not cover these sessions gives no figure for them.
    let short = rig.file("short", "1.0\n");
    let refused = report(&rig.notary.dir, &[&short], &["--wall"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("does not cover"));

    // The usage file holds numbers under its own labels and nothing from the exchange.
    let usage = std::fs::read_to_string(rig.notary.dir.join("usage")).unwrap();
    let labels: Vec<&str> = usage.lines().map(|line| line.split(' ').next().unwrap()).collect();
    assert_eq!(labels, ["hyalos.usage/1", "attested", "refused", "cpu_micros", "peak_kb", "link_sent", "link_received", "end"]);
    assert!(usage.lines().skip(1).take(6).all(|line| line.split(' ').nth(1).unwrap().parse::<u64>().is_ok()), "{usage}");
    // The session line keeps its four fields: time, server, bytes, outcome.
    assert!(rig.notary.lines().iter().all(|line| line.split(' ').count() == 7), "attested lines are unchanged");
}

#[test]
fn s_05_1_e2_five_sessions_say_how_many_and_that_it_is_not_yet_a_measurement() {
    let _one = one_at_a_time();
    let rig = Rig::new("s-05-1-e2");
    rig.sessions(5);
    let text = printed(&report(&rig.notary.dir, &[], &[]));
    assert!(text.contains("sessions: 5 attested, 0 refused"), "{text}");
    assert!(text.contains("not yet a measurement"), "{text}");
    assert!(!text.contains("per attestation"), "{text}");
}

#[test]
fn s_05_1_a_restart_starts_a_new_life_for_the_figures() {
    let _one = one_at_a_time();
    let rig = Rig::new("s-05-1-life");
    rig.sessions(1);
    let dir = rig.notary.dir.clone();
    let root = rig.standin.root.clone();
    let public = rig.who.public.clone();
    drop(rig.notary);
    let again = notary(&dir, &public, &root);
    let text = printed(&report(&again.dir, &[], &[]));
    assert!(text.contains("sessions: 0 attested, 0 refused"), "{text}");
}

#[test]
fn s_05_1_a_report_it_cannot_read_or_reconcile_is_a_refusal() {
    let _one = one_at_a_time();
    let rig = Rig::new("s-05-1-refuse");
    rig.sessions(1);
    let dir = &rig.notary.dir;
    let usage = std::fs::read_to_string(dir.join("usage")).unwrap();

    std::fs::write(dir.join("usage"), usage.replace("attested 1", "attested 2")).unwrap();
    assert!(String::from_utf8_lossy(&report(dir, &[], &[]).stderr).contains("disagree"));

    std::fs::write(dir.join("usage"), usage.replace("end\n", "")).unwrap();
    let cut = report(dir, &[], &[]);
    assert!(!cut.status.success());
    assert!(String::from_utf8_lossy(&cut.stderr).contains("cannot be read"));

    std::fs::remove_file(dir.join("usage")).unwrap();
    assert!(!report(dir, &[], &[]).status.success());
}
