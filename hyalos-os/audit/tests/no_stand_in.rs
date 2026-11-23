//! In a build without `test-trust`, no stand-in provider setting reaches the child `attest verify`.

#![cfg(not(feature = "test-trust"))]

use hyalos_audit::{Answer, PlainFiles, Transcript, Verifier, sha, witnessed};
use std::{env, fs, os::unix::fs::PermissionsExt, process::Command};

const SETTINGS: [&str; 3] = [
    "HYALOS_TRUST_ROOT",
    "HYALOS_TRUST_SERVER_NAME",
    "HYALOS_TRUST_PROVIDER_ADDRESS",
];
const NAME: &str = "s_03_7_e2_provider_settings_never_reach_the_child";

#[test]
fn s_03_7_e2_provider_settings_never_reach_the_child() {
    if env::var_os(SETTINGS[0]).is_none() {
        let mut again = Command::new(env::current_exe().unwrap());
        again.args(["--exact", NAME, "--nocapture"]);
        for name in SETTINGS {
            again.env(name, "a-stand-in");
        }
        let output = again.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        return;
    }
    let dir = env::temp_dir().join(format!("hyalos-no-stand-in-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("record.attest")).unwrap();
    fs::write(dir.join("record.attest/000.presentation"), b"presented").unwrap();
    let seen = dir.join("seen-env");
    let program = dir.join("child");
    fs::write(
        &program,
        format!("#!/bin/sh\n/usr/bin/env > '{}'\nexit 1\n", seen.display()),
    )
    .unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
    let verifier = Verifier::from_env(program.to_str().unwrap());
    assert!(verifier.trust.is_empty());
    let record = Transcript {
        version: 1,
        profile: None,
        task: String::new(),
        guest_sha256: String::new(),
        inputs_hex: Vec::new(),
        answers: vec![Answer {
            schema: 0,
            question_sha256: String::new(),
            label: 0,
            model: String::new(),
            response_id: String::new(),
            attestation: sha(b"presented"),
            provider_proofs: Vec::new(),
        }],
        effects: Vec::new(),
        result_hex: String::new(),
        fuel_used: 0,
    };
    let path = dir.join("record");
    let refused = witnessed(
        &record,
        path.to_str().unwrap(),
        "02",
        &verifier,
        None,
        &PlainFiles,
    );
    assert!(refused.is_err());
    let environment = fs::read_to_string(&seen).expect("the child ran");
    fs::remove_dir_all(&dir).unwrap();
    assert!(!environment.contains("HYALOS_TRUST"), "{environment}");
}
