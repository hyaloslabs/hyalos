//! Runs the built `hyalos-verify` on one committed fixture.

use serde_json::Value;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

pub fn fixtures() -> PathBuf {
    root().join("verify/tests/fixtures")
}

pub fn manifest() -> Value {
    serde_json::from_slice(&fs::read(fixtures().join("manifest.json")).unwrap()).unwrap()
}

/// Every case the manifest lists, with the check that refuses it.
pub fn cases(manifest: &Value) -> Vec<(String, String)> {
    manifest["corpus"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|test| test["cases"].as_array().unwrap())
        .map(|case| {
            (
                case["id"].as_str().unwrap().to_string(),
                case["check"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

/// A program the release build must already hold.
pub fn built(program: &str) -> PathBuf {
    let path = root().join("target/release").join(program);
    assert!(path.exists(), "{program} is not built in target/release");
    path
}

/// The built verifier on a fixture directory, with only that fixture's stand-in settings.
pub fn verify(dir: &Path) -> Output {
    let at = |name: &str| dir.join(name);
    let case: Value = serde_json::from_slice(&fs::read(at("case.json")).unwrap()).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_hyalos-verify"));
    command
        .arg(at("record"))
        .arg(at("links"))
        .arg("--operator-public")
        .arg(at("operator.pub"))
        .arg("--notary-public")
        .arg(at("notary.pub"))
        .arg("--verify")
        .arg(built("attest"))
        .env_clear()
        .env("PATH", env::var("PATH").unwrap())
        .env("HOME", "/nonexistent")
        .env("HYALOS_TRUST_LOG_DIR", at("log"))
        .env("HYALOS_TRUST_LOG_KEY", at("log.pub"))
        .stdin(Stdio::null());
    if case["guest"] == true {
        command.arg("--guest").arg(at("guest.wasm"));
    }
    if case["legacy"] == true {
        command.arg("--legacy-v1");
    }
    if case["trust"] == true {
        command
            .env("HYALOS_TRUST_ROOT", at("root.der"))
            .env("HYALOS_TRUST_SERVER_NAME", "provider.test")
            .env("HYALOS_TRUST_PROVIDER_ADDRESS", "127.0.0.1:1");
    }
    command.output().unwrap()
}

/// Asserts a refusal by this check, with no pass line and no report.
pub fn assert_refused(id: &str, output: &Output, check: &str) {
    let (out, err) = (
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(output.status.code(), Some(1), "{id}: {err}");
    assert!(
        !out.lines()
            .chain(err.lines())
            .any(|l| l.starts_with("audit passed") || l.starts_with("legacy audit passed")),
        "{id}: a pass line"
    );
    assert!(out.is_empty(), "{id}: a refusal prints no report");
    assert_eq!(err.trim(), format!("refused: {check}"), "{id}");
}
