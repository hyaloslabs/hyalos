//! The customer's provider check, run as a program against the stand-in. No test contacts a real
//! provider. Every credential here is made up.

// One of the files allowed `unsafe` (design.md §2 of docs/plans/hyalos-kernel-core).
#![allow(unsafe_code)]

mod common;

use attest::CREDENTIAL_LENGTH;
use common::{NAME, Scratch, Standin, standin};
use std::{
    fs,
    net::TcpListener,
    os::{fd::AsRawFd, unix::fs::PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

const EARLIER: u64 = 1_700_000_000;
const REFUSES: &str = "200,400,200";
const ACCEPTS: &str = "200,200,200";

fn state_text(word: &str, checked: u64) -> String {
    format!("hyalos.canary/1\nstate {word}\nchecked {checked}\nend\n")
}

/// A notary directory and a private test credential.
struct Rig {
    scratch: Scratch,
    dir: PathBuf,
    credential: PathBuf,
    value: String,
}

impl Rig {
    fn new(label: &str) -> Self {
        let scratch = Scratch::new(label);
        let dir = scratch.0.join("notary");
        fs::create_dir(&dir).unwrap();
        let value = format!("test-{}", "c".repeat(CREDENTIAL_LENGTH - 5));
        let credential = scratch.0.join("credential");
        fs::write(&credential, &value).unwrap();
        fs::set_permissions(&credential, fs::Permissions::from_mode(0o600)).unwrap();
        Self {
            scratch,
            dir,
            credential,
            value,
        }
    }

    fn put(&self, text: &str) {
        let path = self.dir.join("canary");
        fs::write(&path, text).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    fn state(&self) -> Option<String> {
        fs::read_to_string(self.dir.join("canary")).ok()
    }

    fn marked(&self) -> bool {
        self.dir.join("canary.init").exists()
    }

    fn provider(&self, statuses: &str) -> Standin {
        standin(&self.scratch.0, &["--statuses", statuses])
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_canary"));
        command.env_remove("HYALOS_TEST_CANARY_FAIL_RENAME");
        command
            .arg("check")
            .arg("--dir")
            .arg(&self.dir)
            .arg("--credential")
            .arg(&self.credential);
        command
    }

    fn check(&self, provider: &Standin) -> Output {
        self.command()
            .env("HYALOS_TRUST_SERVER_NAME", NAME)
            .env("HYALOS_TRUST_ROOT", &provider.root)
            .env("HYALOS_TRUST_PROVIDER_ADDRESS", &provider.address)
            .output()
            .unwrap()
    }

    /// Runs a check that must make no connection, and returns its output.
    fn check_with_no_connection(&self) -> Output {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let output = self
            .command()
            .env("HYALOS_TRUST_SERVER_NAME", NAME)
            .env("HYALOS_TRUST_PROVIDER_ADDRESS", address)
            .output()
            .unwrap();
        assert!(
            listener.accept().is_err(),
            "the check contacted the provider"
        );
        output
    }

    fn reset(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_canary"))
            .arg("reset")
            .arg("--dir")
            .arg(&self.dir)
            .output()
            .unwrap()
    }
}

fn said(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn complained(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn standing(rig: &Rig) -> String {
    rig.state()
        .expect("no state")
        .lines()
        .nth(1)
        .unwrap()
        .to_string()
}

#[test]
fn canary_s_01_1_e1_only_200_400_200_is_a_pass() {
    let rig = Rig::new("canary-1-1-e1");
    let before = now();
    let output = rig.check(&rig.provider(REFUSES));
    let after = now();
    assert_eq!(output.status.code(), Some(0), "{}", complained(&output));
    assert!(said(&output).contains("verdict: the provider refused the forged body"));
    let state = rig.state().unwrap();
    let checked: u64 = state
        .lines()
        .nth(2)
        .unwrap()
        .strip_prefix("checked ")
        .unwrap()
        .parse()
        .unwrap();
    assert!((before..=after).contains(&checked));
    assert_eq!(state, state_text("passed", checked));
    assert!(rig.marked(), "a pass leaves the marker");
    for staged in [
        "canary.new",
        "canary.init.new",
        "canary.held.new",
        "canary.latched.new",
    ] {
        assert!(!rig.dir.join(staged).exists(), "{staged} was left behind");
    }
}

#[test]
fn canary_s_01_1_e2_a_pass_that_cannot_be_written_changes_nothing() {
    let rig = Rig::new("canary-1-1-e2");
    let earlier = state_text("held", EARLIER);
    rig.put(&earlier);
    fs::create_dir(rig.dir.join("canary.new")).unwrap();
    let output = rig.check(&rig.provider(REFUSES));
    assert_ne!(output.status.code(), Some(0));
    assert_eq!(rig.state().unwrap(), earlier);
}

#[test]
fn canary_s_01_2_e1_an_accepted_forgery_latches() {
    let rig = Rig::new("canary-2-e1");
    let output = rig.check(&rig.provider(ACCEPTS));
    assert_eq!(output.status.code(), Some(1));
    assert!(said(&output).contains("verdict: THE PROVIDER ACCEPTS THE FORGED SHAPE"));
    assert_eq!(standing(&rig), "state latched");
}

#[test]
fn canary_a_forgery_latches_even_when_the_last_control_fails() {
    let rig = Rig::new("canary-2-late");
    let output = rig.check(&rig.provider("200,200,500"));
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(standing(&rig), "state latched");
}

#[test]
fn canary_s_01_2_e2_a_failed_control_removes_an_earlier_pass() {
    let rig = Rig::new("canary-2-e2");
    rig.put(&state_text("passed", EARLIER));
    let output = rig.check(&rig.provider("500"));
    assert_eq!(output.status.code(), Some(2));
    assert!(
        !said(&output).contains("small body followed"),
        "the check went on after the control failed"
    );
    assert_eq!(standing(&rig), "state held");
}

#[test]
fn canary_s_01_2_e3_an_unclear_forged_answer_holds() {
    let rig = Rig::new("canary-2-e3");
    let output = rig.check(&rig.provider("200,500,200"));
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(standing(&rig), "state held");
}

#[test]
fn canary_s_01_2_e4_a_failed_last_control_holds() {
    let rig = Rig::new("canary-2-e4");
    let output = rig.check(&rig.provider("200,400,500"));
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(standing(&rig), "state held");
}

#[test]
fn canary_s_01_2_e5_a_dropped_connection_holds() {
    let rig = Rig::new("canary-2-e5");
    let provider = standin(&rig.scratch.0, &["--drop"]);
    let output = rig.check(&provider);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(standing(&rig), "state held");
}

#[test]
fn canary_s_01_2_e6_results_that_cannot_be_staged_stop_the_check() {
    let rig = Rig::new("canary-2-e6");
    let earlier = state_text("passed", EARLIER);
    rig.put(&earlier);
    fs::create_dir(rig.dir.join("canary.latched.new")).unwrap();
    let output = rig.check_with_no_connection();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(rig.state().unwrap(), earlier);
}

#[cfg(feature = "canary-test-hooks")]
#[test]
fn canary_s_01_2_e7_a_failed_rename_removes_the_pass() {
    let rig = Rig::new("canary-2-e7");
    rig.put(&state_text("passed", EARLIER));
    let provider = rig.provider("500");
    let output = rig
        .command()
        .env("HYALOS_TEST_CANARY_FAIL_RENAME", "1")
        .env("HYALOS_TRUST_SERVER_NAME", NAME)
        .env("HYALOS_TRUST_ROOT", &provider.root)
        .env("HYALOS_TRUST_PROVIDER_ADDRESS", &provider.address)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(rig.state(), None, "a pass survived");
    assert!(rig.marked());
    let later = rig.check_with_no_connection();
    assert_eq!(later.status.code(), Some(2));
    assert_eq!(rig.state(), None);
}

#[test]
fn canary_s_01_3_e1_the_credential_is_never_echoed() {
    for (index, statuses) in [REFUSES, ACCEPTS, "500", "200,500,200"]
        .into_iter()
        .enumerate()
    {
        let rig = Rig::new(&format!("canary-3-e1-{index}"));
        let output = rig.check(&rig.provider(statuses));
        for text in [said(&output), complained(&output), rig.state().unwrap()] {
            assert!(!text.contains(&rig.value), "the credential was echoed");
        }
    }
}

#[test]
fn canary_s_01_3_e2_a_credential_of_another_length_is_refused() {
    let rig = Rig::new("canary-3-e2");
    fs::write(&rig.credential, &rig.value[..105]).unwrap();
    let output = rig.check_with_no_connection();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        complained(&output).trim(),
        "refused: the credential is not the length verify is pinned to"
    );
    assert_eq!(rig.state(), None);
}

#[test]
fn canary_s_01_3_e3_an_open_credential_file_is_refused() {
    let rig = Rig::new("canary-3-e3");
    fs::set_permissions(&rig.credential, fs::Permissions::from_mode(0o644)).unwrap();
    let output = rig.check_with_no_connection();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        complained(&output).trim(),
        "refused: cannot read the credential file, or its access is too open"
    );
    assert_eq!(rig.state(), None);
}

#[test]
fn canary_s_01_4_e1_a_pass_does_not_clear_a_latch() {
    let rig = Rig::new("canary-4-e1");
    let latched = state_text("latched", EARLIER);
    rig.put(&latched);
    let output = rig.check(&rig.provider(REFUSES));
    assert_eq!(output.status.code(), Some(1));
    assert!(said(&output).contains("stays latched"));
    assert_eq!(rig.state().unwrap(), latched);
}

#[test]
fn canary_a_latch_outlasts_an_inconclusive_check() {
    let rig = Rig::new("canary-4-inconclusive");
    let latched = state_text("latched", EARLIER);
    rig.put(&latched);
    let output = rig.check(&rig.provider("500"));
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(rig.state().unwrap(), latched);
}

#[test]
fn canary_s_01_4_e2_the_reset_clears_a_latch() {
    let rig = Rig::new("canary-4-e2");
    rig.put(&state_text("latched", EARLIER));
    assert_eq!(rig.reset().status.code(), Some(0));
    assert_eq!(standing(&rig), "state held");
    let output = rig.check(&rig.provider(REFUSES));
    assert_eq!(output.status.code(), Some(0), "{}", complained(&output));
    assert_eq!(standing(&rig), "state passed");
}

#[test]
fn canary_s_01_4_e3_a_malformed_state_is_refused_and_kept() {
    let rig = Rig::new("canary-4-e3");
    let damaged = "hyalos.canary/1\nstate latched\n";
    rig.put(damaged);
    let output = rig.check_with_no_connection();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        complained(&output).trim(),
        "refused: the canary state is unreadable or malformed"
    );
    assert_eq!(rig.state().unwrap(), damaged);
}

#[test]
fn canary_s_01_4_e4_the_reset_replaces_a_malformed_state() {
    let rig = Rig::new("canary-4-e4");
    rig.put("not a state");
    assert_eq!(rig.reset().status.code(), Some(0));
    assert_eq!(standing(&rig), "state held");
}

#[test]
fn canary_s_01_4_e5_a_lost_state_is_refused_until_the_reset() {
    let rig = Rig::new("canary-4-e5");
    assert_eq!(rig.check(&rig.provider(ACCEPTS)).status.code(), Some(1));
    fs::remove_file(rig.dir.join("canary")).unwrap();
    let output = rig.check_with_no_connection();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(rig.state(), None);
    assert_eq!(rig.reset().status.code(), Some(0));
    assert_eq!(standing(&rig), "state held");
}

#[cfg(feature = "canary-test-hooks")]
#[test]
fn canary_s_01_4_e6_a_lost_latch_is_not_cleared_by_a_pass() {
    let rig = Rig::new("canary-4-e6");
    rig.put(&state_text("passed", EARLIER));
    assert!(!rig.marked());
    let provider = rig.provider(ACCEPTS);
    let output = rig
        .command()
        .env("HYALOS_TEST_CANARY_FAIL_RENAME", "1")
        .env("HYALOS_TRUST_SERVER_NAME", NAME)
        .env("HYALOS_TRUST_ROOT", &provider.root)
        .env("HYALOS_TRUST_PROVIDER_ADDRESS", &provider.address)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(rig.state(), None);
    assert!(rig.marked());
    let later = rig.check_with_no_connection();
    assert_eq!(later.status.code(), Some(2));
    assert_eq!(rig.state(), None);
}

#[test]
fn canary_s_01_4_e7_a_marker_that_cannot_be_made_stops_the_check() {
    let rig = Rig::new("canary-4-e7");
    let earlier = state_text("held", EARLIER);
    rig.put(&earlier);
    fs::create_dir(rig.dir.join("canary.init.new")).unwrap();
    let output = rig.check_with_no_connection();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(rig.state().unwrap(), earlier);
    assert!(!rig.marked());
}

#[test]
fn canary_1_2_a_second_check_is_refused_by_the_lock() {
    let rig = Rig::new("canary-1-2");
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(rig.dir.join("canary.lock"))
        .unwrap();
    // SAFETY: the descriptor belongs to `lock`, which is alive for this call.
    assert_eq!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    let output = rig.check_with_no_connection();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        complained(&output).trim(),
        "refused: another canary check or reset holds the lock"
    );
    assert_eq!(rig.state(), None);
    drop(lock);
}

#[cfg(not(feature = "canary-test-hooks"))]
#[test]
fn canary_1_3_a_default_build_ignores_the_rename_failure_setting() {
    let rig = Rig::new("canary-1-3");
    let provider = rig.provider(ACCEPTS);
    let output = rig
        .command()
        .env("HYALOS_TEST_CANARY_FAIL_RENAME", "1")
        .env("HYALOS_TRUST_SERVER_NAME", NAME)
        .env("HYALOS_TRUST_ROOT", &provider.root)
        .env("HYALOS_TRUST_PROVIDER_ADDRESS", &provider.address)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(standing(&rig), "state latched");
}

#[test]
fn canary_a_missing_directory_is_not_called_a_held_lock() {
    let rig = Rig::new("canary-no-dir");
    fs::remove_dir(&rig.dir).unwrap();
    let output = rig.check_with_no_connection();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        complained(&output).trim(),
        "refused: the notary directory's lock file cannot be opened"
    );
    let output = rig.reset();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        complained(&output).trim(),
        "refused: the notary directory's lock file cannot be opened"
    );
}

#[test]
fn canary_the_reset_needs_a_state_or_a_marker() {
    let rig = Rig::new("canary-reset-none");
    let output = rig.reset();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(rig.state(), None);
}

fn status(rig: &Rig) -> Output {
    Command::new(env!("CARGO_BIN_EXE_canary"))
        .arg("status")
        .arg("--dir")
        .arg(&rig.dir)
        .env_remove("HYALOS_TRUST_SERVER_NAME")
        .output()
        .unwrap()
}

#[test]
fn canary_4_1_status_is_fresh_only_on_a_fresh_pass() {
    let rig = Rig::new("canary-4-1");
    rig.put(&state_text("passed", now() - 60));
    let output = status(&rig);
    assert_eq!(
        (output.status.code(), said(&output).trim()),
        (Some(0), "fresh")
    );
    assert!(
        !rig.dir.join("canary.lock").exists(),
        "status takes no lock"
    );
    let window = attest::canary::CANARY_WINDOW_SECONDS;
    for text in [
        state_text("passed", now() - window - 1),
        state_text("passed", now() + 3600),
        state_text("held", now()),
        state_text("latched", now()),
        "not a state".to_string(),
    ] {
        rig.put(&text);
        let output = status(&rig);
        assert_eq!(
            (output.status.code(), said(&output).trim()),
            (Some(1), "not fresh"),
            "{text}"
        );
    }
    fs::remove_file(rig.dir.join("canary")).unwrap();
    let output = status(&rig);
    assert_eq!(
        (output.status.code(), said(&output).trim()),
        (Some(1), "not fresh")
    );
}

#[test]
fn canary_4_1_status_names_no_credential() {
    let source = include_str!("../src/bin/canary.rs");
    let body = source
        .split("fn status(dir: &Path)")
        .nth(1)
        .unwrap()
        .split("#[tokio::main]")
        .next()
        .unwrap();
    assert!(!body.contains("credential"), "{body}");
}

const RECORD: &str = include_str!("../../RECORD.md");

#[test]
fn canary_s_04_1_e1_the_format_names_the_canary() {
    for named in ["hyalos.canary/1", "Version 3", "| `canary` |", "24 hours"] {
        assert!(RECORD.contains(named), "RECORD.md does not name {named}");
    }
}

#[test]
fn canary_s_04_1_e2_the_format_states_the_latch_and_the_limit() {
    for named in [
        "canary reset",
        "does not close the hidden API-key case",
        "the state was not updated",
    ] {
        assert!(RECORD.contains(named), "RECORD.md does not state {named}");
    }
}

/// The operator's script in a temporary directory, with every program it calls a stub on `PATH`
/// or at its `$PWD` path. Nothing real is contacted. `None` when this host cannot run it. The
/// third value is what the stub monitor's `live` saw as `HYALOS_KERNEL`, one line per run, and
/// is empty when `live` never ran. `kernel_builds` chooses whether the stub `cargo` leaves a
/// kernel image behind.
fn probe(
    label: &str,
    canary_exit: u8,
    kernel_builds: bool,
) -> Option<(Output, String, String, bool)> {
    if !std::path::Path::new("/usr/bin/time").exists() {
        eprintln!("skipped: this host has no /usr/bin/time");
        return None;
    }
    let scratch = Scratch::new(label);
    let (work, stubs, home) = (
        scratch.0.join("work"),
        scratch.0.join("stubs"),
        scratch.0.join("home"),
    );
    for dir in [
        work.join("target/release"),
        work.join("kernel-image"),
        stubs.clone(),
        home.clone(),
    ] {
        fs::create_dir_all(dir).unwrap();
    }
    let (log, live) = (scratch.0.join("ssh.log"), scratch.0.join("live.log"));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let write = |path: PathBuf, body: String| {
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    };
    write(
        stubs.join("ssh"),
        format!(
            "echo \"$*\" >> '{}'\ncase \"$*\" in\n-G*) printf 'hostname 127.0.0.1\\nport {port}\\n' ;;\n*'canary status'*) exit {canary_exit} ;;\n*'-L '*) printf 'signing aa\\ntransport bb\\nlistening 127.0.0.1:7047\\n'; exec 2>/dev/null; exec sleep 3 ;;\n*authorize*) echo an-authorisation ;;\nesac",
            log.display()
        ),
    );
    write(stubs.join("scp"), "exit 0".to_string());
    write(work.join("target/release/attest"), "case \"$1\" in\nlink-key) echo an-operator-key ;;\nask) cat > /dev/null; echo '{}' ;;\nesac".to_string());
    write(work.join("target/release/notary"), "exit 0".to_string());
    write(work.join("target/release/canary"), "exit 0".to_string());
    write(
        work.join("target/release/task-kernel"),
        format!(
            "case \"$1\" in\ntask) echo a-task ;;\nlive) echo \"$HYALOS_KERNEL\" >> '{}' ;;\nesac",
            live.display()
        ),
    );
    // The script builds the kernel image in `kernel-image`, where the real one lands.
    let built = if kernel_builds {
        "mkdir -p target/x86_64-unknown-none/release && echo a-kernel > target/x86_64-unknown-none/release/hyalos-kernel"
    } else {
        "exit 101"
    };
    write(stubs.join("cargo"), built.to_string());
    let path = format!("{}:{}", stubs.display(), std::env::var("PATH").unwrap());
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/probes/two_machines.sh");
    let output = Command::new("bash")
        .arg(script)
        .args(["a-host", "owner/repo", "1"])
        .current_dir(&work)
        .env("PATH", path)
        .env("HOME", &home)
        .env_remove("HYALOS_KERNEL")
        .output()
        .unwrap();
    drop(listener);
    Some((
        output,
        fs::read_to_string(&log).unwrap_or_default(),
        fs::read_to_string(&live).unwrap_or_default(),
        work.join("evidence/out/two-machines").exists(),
    ))
}

#[test]
fn canary_s_04_2_e1_with_a_fresh_pass_the_probe_reaches_the_run() {
    let Some((output, calls, ran, _)) = probe("canary-4-2-e1", 0, true) else {
        return;
    };
    assert!(output.status.success(), "{}", complained(&output));
    assert!(!ran.is_empty(), "the run was not reached");
    let (gate, notary) = (
        calls.find("canary status").unwrap(),
        calls.find("--listen").unwrap(),
    );
    assert!(
        gate < notary,
        "the gate comes before the notary starts: {calls}"
    );
}

#[test]
fn canary_s_04_2_e2_without_a_pass_the_probe_stops_before_anything_public() {
    let Some((output, calls, ran, made)) = probe("canary-4-2-e2", 1, true) else {
        return;
    };
    assert_eq!(output.status.code(), Some(2), "{}", complained(&output));
    assert!(said(&output).contains("no fresh customer canary pass"));
    assert!(ran.is_empty(), "the run went ahead");
    assert!(
        !calls.contains("--listen") && !calls.contains("authorize"),
        "{calls}"
    );
    assert!(
        !made,
        "a stopped run left its output directory, which would block the retry"
    );
}

/// S-04.5 of `docs/plans/hyalos-kernel-core`: the script's `live` run is on the KVM kernel it
/// built, so the monitor's guard never refuses it after an authorised attempt is spent.
#[test]
fn core_s_04_5_the_probe_script_runs_live_on_the_kernel_it_built() {
    let Some((output, _, ran, _)) = probe("core-4-5-kernel", 0, true) else {
        return;
    };
    assert!(output.status.success(), "{}", complained(&output));
    let kernel = ran.trim();
    assert!(
        kernel.ends_with("/kernel-image/target/x86_64-unknown-none/release/hyalos-kernel"),
        "live ran without the built kernel"
    );
    assert!(
        Path::new(kernel).is_absolute(),
        "the kernel path does not depend on the monitor's directory"
    );
}

/// S-04.5: without a kernel image the script stops before the notary starts, any authorised
/// attempt is spent or anything is published.
#[test]
fn core_s_04_5_the_probe_script_stops_before_anything_without_a_kernel() {
    let Some((output, calls, ran, made)) = probe("core-4-5-no-kernel", 0, false) else {
        return;
    };
    assert_eq!(output.status.code(), Some(2), "{}", complained(&output));
    assert!(said(&output).contains("the kernel image did not build"));
    assert!(calls.is_empty(), "the other machine was contacted: {calls}");
    assert!(ran.is_empty(), "the run went ahead");
    assert!(!made, "a stopped run left its output directory");
}
