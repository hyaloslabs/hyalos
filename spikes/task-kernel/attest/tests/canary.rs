//! The customer's provider check, run as a program against the stand-in. No test contacts a real
//! provider. Every credential here is made up.

mod common;

use attest::CREDENTIAL_LENGTH;
use common::{NAME, Scratch, Standin, standin};
use std::{
    fs,
    net::TcpListener,
    os::{fd::AsRawFd, unix::fs::PermissionsExt},
    path::PathBuf,
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
        Self { scratch, dir, credential, value }
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
        command.arg("check").arg("--dir").arg(&self.dir).arg("--credential").arg(&self.credential);
        command
    }

    fn check(&self, provider: &Standin) -> Output {
        self.command().env("HYALOS_TRUST_SERVER_NAME", NAME).env("HYALOS_TRUST_ROOT", &provider.root).env("HYALOS_TRUST_PROVIDER_ADDRESS", &provider.address).output().unwrap()
    }

    /// Runs a check that must make no connection, and returns its output.
    fn check_with_no_connection(&self) -> Output {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let output = self.command().env("HYALOS_TRUST_SERVER_NAME", NAME).env("HYALOS_TRUST_PROVIDER_ADDRESS", address).output().unwrap();
        assert!(listener.accept().is_err(), "the check contacted the provider");
        output
    }

    fn reset(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_canary")).arg("reset").arg("--dir").arg(&self.dir).output().unwrap()
    }
}

fn said(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn complained(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

fn standing(rig: &Rig) -> String {
    rig.state().expect("no state").lines().nth(1).unwrap().to_string()
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
    let checked: u64 = state.lines().nth(2).unwrap().strip_prefix("checked ").unwrap().parse().unwrap();
    assert!((before..=after).contains(&checked));
    assert_eq!(state, state_text("passed", checked));
    assert!(rig.marked(), "a pass leaves the marker");
    for staged in ["canary.new", "canary.init.new", "canary.held.new", "canary.latched.new"] {
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
    assert!(!said(&output).contains("small body followed"), "the check went on after the control failed");
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
    let output = rig.command().env("HYALOS_TEST_CANARY_FAIL_RENAME", "1").env("HYALOS_TRUST_SERVER_NAME", NAME).env("HYALOS_TRUST_ROOT", &provider.root).env("HYALOS_TRUST_PROVIDER_ADDRESS", &provider.address).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(rig.state(), None, "a pass survived");
    assert!(rig.marked());
    let later = rig.check_with_no_connection();
    assert_eq!(later.status.code(), Some(2));
    assert_eq!(rig.state(), None);
}

#[test]
fn canary_s_01_3_e1_the_credential_is_never_echoed() {
    for (index, statuses) in [REFUSES, ACCEPTS, "500", "200,500,200"].into_iter().enumerate() {
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
    assert_eq!(complained(&output).trim(), "refused: the credential is not the length verify is pinned to");
    assert_eq!(rig.state(), None);
}

#[test]
fn canary_s_01_3_e3_an_open_credential_file_is_refused() {
    let rig = Rig::new("canary-3-e3");
    fs::set_permissions(&rig.credential, fs::Permissions::from_mode(0o644)).unwrap();
    let output = rig.check_with_no_connection();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(complained(&output).trim(), "refused: cannot read the credential file, or its access is too open");
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
    assert_eq!(complained(&output).trim(), "refused: the canary state is unreadable or malformed");
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
    let output = rig.command().env("HYALOS_TEST_CANARY_FAIL_RENAME", "1").env("HYALOS_TRUST_SERVER_NAME", NAME).env("HYALOS_TRUST_ROOT", &provider.root).env("HYALOS_TRUST_PROVIDER_ADDRESS", &provider.address).output().unwrap();
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
    let lock = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(rig.dir.join("canary.lock")).unwrap();
    // SAFETY: the descriptor belongs to `lock`, which is alive for this call.
    assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }, 0);
    let output = rig.check_with_no_connection();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(complained(&output).trim(), "refused: another canary check or reset holds the lock");
    assert_eq!(rig.state(), None);
    drop(lock);
}

#[cfg(not(feature = "canary-test-hooks"))]
#[test]
fn canary_1_3_a_default_build_ignores_the_rename_failure_setting() {
    let rig = Rig::new("canary-1-3");
    let provider = rig.provider(ACCEPTS);
    let output = rig.command().env("HYALOS_TEST_CANARY_FAIL_RENAME", "1").env("HYALOS_TRUST_SERVER_NAME", NAME).env("HYALOS_TRUST_ROOT", &provider.root).env("HYALOS_TRUST_PROVIDER_ADDRESS", &provider.address).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(standing(&rig), "state latched");
}

#[test]
fn canary_the_reset_needs_a_state_or_a_marker() {
    let rig = Rig::new("canary-reset-none");
    let output = rig.reset();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(rig.state(), None);
}
