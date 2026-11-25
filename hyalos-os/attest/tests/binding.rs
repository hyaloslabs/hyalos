//! The built programs, run as an operator would run them, against the stand-in provider.
//!
//! One attested call, through a real notary, is shared by every test here: a session costs seconds
//! and several hundred megabytes. The credential these tests use is a made-up value in a temporary
//! home.

mod common;

use common::{INTENT, NAME, Notary, Operator, Scratch, Standin};
use std::{path::PathBuf, process::Command, sync::OnceLock};

const POSITION: u64 = 3;

struct Call {
    home: PathBuf,
    root: PathBuf,
    presentation: PathBuf,
    said: serde_json::Value,
    notary: String,
    authorisation: String,
    who: Operator,
    /// Held so the programs live as long as the tests, and end when they do.
    _programs: (Standin, Notary, Scratch),
}

fn tool(call: &Call, home: &PathBuf, provider: &str) -> Command {
    let who = Operator {
        home: home.clone(),
        key: call.who.key.clone(),
        public: call.who.public.clone(),
    };
    common::tool(
        &who,
        &call.root,
        provider,
        &call._programs.1.address,
        &call._programs.1.transport,
        &call.notary,
        &call.authorisation,
    )
}

fn call() -> &'static Call {
    static CALL: OnceLock<Call> = OnceLock::new();
    CALL.get_or_init(|| {
        let scratch = Scratch::new("binding-home");
        let home = scratch.0.join("home");
        let who = common::operator(&home);
        let standin = common::standin(&scratch.0, &[]);
        let root = standin.root.clone();
        let notary = common::notary(&scratch.0.join("notary"), &who.public, &root);
        let presentation = home.join("one.presentation");
        let command = common::tool(
            &who,
            &root,
            &standin.address,
            &notary.address,
            &notary.transport,
            &notary.signing,
            &notary.authorisation,
        );
        let said = common::said(&common::run_ask(command, &presentation, INTENT, POSITION));
        Call {
            home,
            root,
            presentation,
            said,
            notary: notary.signing.clone(),
            authorisation: notary.authorisation.clone(),
            who,
            _programs: (standin, notary, scratch),
        }
    })
}

fn verify(call: &Call, file: &PathBuf) -> Result<serde_json::Value, String> {
    let output = tool(call, &call.home, "127.0.0.1:9")
        .args(["verify", file.to_str().unwrap(), &call.notary])
        .output()
        .unwrap();
    if output.status.success() {
        Ok(serde_json::from_slice(&output.stdout).unwrap())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}

/// Where the notary's run and sequence number sit in the presentation's bytes.
fn run_at(bytes: &[u8]) -> usize {
    let mut found = bytes.windows(INTENT.len()).enumerate().filter(|(at, w)| {
        *w == INTENT.as_bytes() && bytes.get(at + 64..at + 72) == Some(&[0u8; 8])
    });
    let (at, _) = found
        .next()
        .expect("the presentation holds no run followed by a sequence number");
    assert!(
        found.next().is_none(),
        "more than one run and sequence number"
    );
    at
}

#[test]
fn s_03_1_a_request_with_a_binding_is_answered_and_verify_reports_the_binding() {
    let call = call();
    assert_eq!(call.said["label"], 0, "the stand-in's typed label");
    assert_eq!(
        call.said["trust"], "test trust settings",
        "ask's own line must say so too"
    );
    let report = verify(call, &call.presentation).unwrap();
    assert_eq!(report["intent_hash"], INTENT);
    assert_eq!(report["position"], 3);
    assert_eq!(report["label"], 0);
    assert_eq!(
        report["trust"], "test trust settings",
        "a report made against a stand-in must say so"
    );
}

#[test]
fn t_1_2_verify_reports_the_run_and_sequence_the_notary_signed() {
    let call = call();
    let report = verify(call, &call.presentation).unwrap();
    assert_eq!(report["run"], INTENT);
    assert_eq!(report["sequence"], 0);
    assert_eq!(
        call.said["sequence"], 0,
        "ask prints the sequence number too"
    );
}

#[test]
fn t_1_2_an_altered_sequence_number_does_not_verify() {
    let call = call();
    let mut bytes = std::fs::read(&call.presentation).unwrap();
    let at = run_at(&bytes);
    bytes[at + 71] = 1;
    let altered = call.home.join("altered-sequence.presentation");
    std::fs::write(&altered, &bytes).unwrap();
    let refusal = verify(call, &altered).unwrap_err();
    assert!(refusal.contains("does not verify"), "{refusal}");
}

#[test]
fn t_1_2_an_altered_run_does_not_verify() {
    let call = call();
    let mut bytes = std::fs::read(&call.presentation).unwrap();
    let at = run_at(&bytes);
    bytes[at] = if bytes[at] == b'0' { b'1' } else { b'0' };
    let altered = call.home.join("altered-run.presentation");
    std::fs::write(&altered, &bytes).unwrap();
    let refusal = verify(call, &altered).unwrap_err();
    assert!(refusal.contains("does not verify"), "{refusal}");
}

#[test]
fn t_1_3_verify_reports_the_session_time_the_notary_recorded() {
    let call = call();
    let report = verify(call, &call.presentation).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let time = report["time"]
        .as_u64()
        .expect("a session time in whole seconds");
    assert!(
        time <= now && now - time < 600,
        "session time {time} is not this test's, now {now}"
    );
}

/// Verifies the shared presentation with only the trust settings given, and returns the outcome.
fn verify_with(
    call: &Call,
    settings: &[(&str, &std::ffi::OsStr)],
) -> Result<serde_json::Value, String> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_attest"));
    command.env("HOME", &call.home);
    for name in [
        "HYALOS_TRUST_SERVER_NAME",
        "HYALOS_TRUST_ROOT",
        "HYALOS_TRUST_PROVIDER_ADDRESS",
    ] {
        command.env_remove(name);
    }
    let output = command
        .envs(settings.iter().copied())
        .args(["verify", call.presentation.to_str().unwrap(), &call.notary])
        .output()
        .unwrap();
    if output.status.success() {
        Ok(serde_json::from_slice(&output.stdout).unwrap())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}

#[test]
fn s_03_1_the_default_trust_settings_refuse_the_stand_in_by_its_certificate() {
    // With no test root, the stand-in's certificate chain is not one this tool trusts.
    let refusal = verify_with(call(), &[]).unwrap_err();
    assert!(refusal.contains("does not verify"), "{refusal}");
}

#[test]
fn s_03_1_the_test_root_alone_refuses_the_stand_in_by_its_name() {
    // The chain now verifies, so this reaches the comparison with the approved server name.
    let call = call();
    let refusal = verify_with(call, &[("HYALOS_TRUST_ROOT", call.root.as_os_str())]).unwrap_err();
    assert!(
        refusal.contains("the session was with another server"),
        "{refusal}"
    );
}

#[test]
fn s_03_1_any_one_test_setting_marks_the_report() {
    // An address is not used by verify at all, and still marks what it reports or refuses.
    let call = call();
    let both = [
        ("HYALOS_TRUST_ROOT", call.root.as_os_str()),
        ("HYALOS_TRUST_SERVER_NAME", std::ffi::OsStr::new(NAME)),
    ];
    assert_eq!(
        verify_with(call, &both).unwrap()["trust"],
        "test trust settings"
    );
}

#[test]
fn issue_394_a_credential_of_another_length_is_refused_before_anything_is_sent() {
    let call = call();
    let home = call.home.join("other-length");
    std::fs::create_dir_all(home.join(".config/hyalos-spike")).unwrap();
    std::fs::write(
        home.join(".config/hyalos-spike/anthropic.key"),
        format!("{}\n", "x".repeat(107)),
    )
    .unwrap();
    // Nothing listens at this address, so only a refusal before connecting gives this message.
    let output = common::run_ask(
        tool(call, &home, "127.0.0.1:9"),
        &home.join("never.presentation"),
        INTENT,
        POSITION,
    );
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("not the length this tool is pinned to")
    );
    assert!(!home.join("never.presentation").exists());
}

#[test]
fn a_real_shaped_credential_is_never_sent_under_test_settings() {
    let call = call();
    let home = call.home.join("real-shaped");
    std::fs::create_dir_all(home.join(".config/hyalos-spike")).unwrap();
    std::fs::write(
        home.join(".config/hyalos-spike/anthropic.key"),
        "sk-ant-made-up-for-tests\n",
    )
    .unwrap();
    let output = common::run_ask(
        tool(call, &home, "127.0.0.1:9"),
        &home.join("never.presentation"),
        INTENT,
        POSITION,
    );
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("refusing to send a real credential under test trust settings")
    );
    assert!(!home.join("never.presentation").exists());
}
