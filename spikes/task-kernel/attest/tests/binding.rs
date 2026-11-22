//! The built programs, run as an operator would run them, against the stand-in provider.
//!
//! One attested call is made and shared by every test here: a session costs seconds and several
//! hundred megabytes. The credential these tests use is a made-up value in a temporary home.

use std::{
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::OnceLock,
};

const INTENT: &str = "5f1d0c7a9b3e4f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8";
const POSITION: &str = "3";
const NAME: &str = "provider.test";

struct Call {
    home: PathBuf,
    root: PathBuf,
    presentation: PathBuf,
    said: serde_json::Value,
    notary: String,
    /// Held so the stand-in lives as long as the tests, and ends when they do.
    _standin: Child,
}

fn tool(call_home: &PathBuf, root: &PathBuf, address: Option<&str>) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_attest"));
    command.env("HOME", call_home).env("HYALOS_TRUST_SERVER_NAME", NAME).env("HYALOS_TRUST_ROOT", root);
    if let Some(address) = address {
        command.env("HYALOS_TRUST_PROVIDER_ADDRESS", address);
    }
    command
}

fn call() -> &'static Call {
    static CALL: OnceLock<Call> = OnceLock::new();
    CALL.get_or_init(|| {
        // Under the build's own scratch directory, so nothing is left in the system's, and named
        // for this process, so two runs on one build directory do not share it.
        let home = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("binding-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let config = home.join(".config/hyalos-spike");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("anthropic.key"), "made-up-credential-for-tests\n").unwrap();
        let root = home.join("root.der");
        let mut standin = Command::new(env!("CARGO_BIN_EXE_standin"))
            .arg(&root)
            .arg(NAME)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(standin.stdout.take().unwrap()).read_line(&mut line).unwrap();
        let address = line.trim().strip_prefix("listening ").expect("the stand-in did not start").to_string();

        let presentation = home.join("one.presentation");
        let mut ask = tool(&home, &root, Some(&address))
            .args(["ask", presentation.to_str().unwrap(), INTENT, POSITION])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        ask.stdin.take().unwrap().write_all(b"The build fails on a clean checkout").unwrap();
        // A stuck session fails the tests. It does not hang them.
        let started = std::time::Instant::now();
        while ask.try_wait().unwrap().is_none() {
            if started.elapsed() > std::time::Duration::from_secs(120) {
                let _ = ask.kill();
                panic!("the attested call did not finish in 120 seconds");
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let output = ask.wait_with_output().unwrap();
        assert!(output.status.success(), "ask failed: {}", String::from_utf8_lossy(&output.stderr));
        let said = serde_json::from_slice(&output.stdout).unwrap();
        let notary = tool(&home, &root, None).arg("notary").output().unwrap();
        Call { home, root, presentation, said, notary: String::from_utf8(notary.stdout).unwrap().trim().to_string(), _standin: standin }
    })
}

fn verify(call: &Call, file: &PathBuf) -> Result<serde_json::Value, String> {
    let output = tool(&call.home, &call.root, None).args(["verify", file.to_str().unwrap(), &call.notary]).output().unwrap();
    if output.status.success() {
        Ok(serde_json::from_slice(&output.stdout).unwrap())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}

/// Where the notary's run and sequence number sit in the presentation's bytes.
fn run_at(bytes: &[u8]) -> usize {
    let mut found = bytes.windows(INTENT.len()).enumerate().filter(|(at, w)| *w == INTENT.as_bytes() && bytes.get(at + 64..at + 72) == Some(&[0u8; 8]));
    let (at, _) = found.next().expect("the presentation holds no run followed by a sequence number");
    assert!(found.next().is_none(), "more than one run and sequence number");
    at
}

#[test]
fn s_03_1_a_request_with_a_binding_is_answered_and_verify_reports_the_binding() {
    let call = call();
    assert_eq!(call.said["label"], 0, "the stand-in's typed label");
    assert_eq!(call.said["trust"], "test trust settings", "ask's own line must say so too");
    let report = verify(call, &call.presentation).unwrap();
    assert_eq!(report["intent_hash"], INTENT);
    assert_eq!(report["position"], 3);
    assert_eq!(report["label"], 0);
    assert_eq!(report["trust"], "test trust settings", "a report made against a stand-in must say so");
}

#[test]
fn t_1_2_verify_reports_the_run_and_sequence_the_notary_signed() {
    let call = call();
    let report = verify(call, &call.presentation).unwrap();
    assert_eq!(report["run"], INTENT);
    assert_eq!(report["sequence"], 0);
    assert_eq!(call.said["sequence"], 0, "ask prints the sequence number too");
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
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let time = report["time"].as_u64().expect("a session time in whole seconds");
    assert!(time <= now && now - time < 600, "session time {time} is not this test's, now {now}");
}

/// Verifies the shared presentation with only the trust settings given, and returns the outcome.
fn verify_with(call: &Call, settings: &[(&str, &std::ffi::OsStr)]) -> Result<serde_json::Value, String> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_attest"));
    command.env("HOME", &call.home);
    for name in ["HYALOS_TRUST_SERVER_NAME", "HYALOS_TRUST_ROOT", "HYALOS_TRUST_PROVIDER_ADDRESS"] {
        command.env_remove(name);
    }
    let output = command.envs(settings.iter().copied()).args(["verify", call.presentation.to_str().unwrap(), &call.notary]).output().unwrap();
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
    assert!(refusal.contains("the session was with another server"), "{refusal}");
}

#[test]
fn s_03_1_any_one_test_setting_marks_the_report() {
    // An address is not used by verify at all, and still marks what it reports or refuses.
    let call = call();
    let both = [("HYALOS_TRUST_ROOT", call.root.as_os_str()), ("HYALOS_TRUST_SERVER_NAME", std::ffi::OsStr::new(NAME))];
    assert_eq!(verify_with(call, &both).unwrap()["trust"], "test trust settings");
}

#[test]
fn a_real_shaped_credential_is_never_sent_under_test_settings() {
    let call = call();
    let home = call.home.join("real-shaped");
    std::fs::create_dir_all(home.join(".config/hyalos-spike")).unwrap();
    std::fs::write(home.join(".config/hyalos-spike/anthropic.key"), "sk-ant-made-up-for-tests\n").unwrap();
    let mut ask = tool(&home, &call.root, Some("127.0.0.1:9"))
        .args(["ask", home.join("never.presentation").to_str().unwrap(), INTENT, POSITION])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    ask.stdin.take().unwrap().write_all(b"A title").unwrap();
    let output = ask.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("refusing to send a real credential under test trust settings"));
    assert!(!home.join("never.presentation").exists());
}
