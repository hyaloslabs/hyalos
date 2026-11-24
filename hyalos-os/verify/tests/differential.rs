//! S-04.2 and S-04.3: the in-process checks against the independent judges they replaced, on every
//! stored presentation and signature, valid and altered.

mod common;

use attest_check::{Trust, check, named_notary};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use common::{built, root};
use hyalos_audit::{REKOR_KEY, public_der, sha, signature_verifies};
use serde_json::Value;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn presentations(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            presentations(&path, found);
        } else if path.extension().is_some_and(|e| e == "presentation") {
            found.push(path);
        }
    }
}

/// The committed key for a presentation's record, when its directory holds one.
fn committed_key(file: &Path) -> Option<String> {
    let record = file.parent()?;
    let dir = if record.extension().is_some_and(|e| e == "attest") {
        record.parent()?
    } else {
        record
    };
    let text = fs::read_to_string(dir.join("notary.pub")).ok()?;
    Some(text.trim().to_string())
}

/// `attest verify` under the real provider's default trust.
fn judged_by_attest(bytes: &[u8], key: &str, scratch: &Path) -> Option<Value> {
    let file = scratch.join("presentation");
    fs::write(&file, bytes).unwrap();
    let output = Command::new(built("attest"))
        .arg("verify")
        .arg(&file)
        .arg(key)
        .env_clear()
        .stdin(Stdio::null())
        .output()
        .unwrap();
    output
        .status
        .success()
        .then(|| serde_json::from_slice(&output.stdout).unwrap())
}

fn assert_same(file: &Path, program: &Option<Value>, inside: &Option<Value>) {
    match (program, inside) {
        (None, None) => {}
        (Some(a), Some(b)) => {
            let (a, b) = (a.as_object().unwrap(), b.as_object().unwrap());
            let keys = |map: &serde_json::Map<String, Value>| {
                let mut names: Vec<String> = map.keys().cloned().collect();
                names.sort();
                names
            };
            assert_eq!(keys(a), keys(b), "{}", file.display());
            for (name, value) in a {
                assert_eq!(Some(value), b.get(name), "{}: {name}", file.display());
            }
        }
        _ => panic!("{}: the two checks disagree on the verdict", file.display()),
    }
}

fn scratch(label: &str) -> PathBuf {
    let dir = env::temp_dir().join(format!("hyalos-diff-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn s_04_2_e1_e2_both_checks_agree_on_every_stored_presentation() {
    std::panic::set_hook(Box::new(|info| {
        if info.location().is_some_and(|l| l.file().contains("tests/")) {
            eprintln!("{info}");
        }
    }));
    let dir = scratch("presentations");
    let mut files = Vec::new();
    presentations(&root().join("evidence"), &mut files);
    files.sort();
    assert_eq!(files.len(), 171, "the committed presentations");
    let trust = Trust::real();
    let (mut accepted, mut refused, mut unkeyed, mut canary) = (0, 0, 0, 0);
    for file in &files {
        let bytes = fs::read(file).unwrap();
        let committed = committed_key(file);
        let key = committed
            .clone()
            .or_else(|| named_notary(&bytes))
            .unwrap_or_else(|| format!("02{}", "11".repeat(32)));
        let program = judged_by_attest(&bytes, &key, &dir);
        let inside = check(&bytes, &key, &trust).ok();
        assert_same(file, &program, &inside);
        if committed.is_none() {
            // Compares only the check: the key is the one the presentation names.
            unkeyed += 1;
        } else if program.is_some() {
            accepted += 1;
        } else {
            refused += 1;
        }
        if file
            .display()
            .to_string()
            .contains("two-machines-canary/record.json.attest")
        {
            assert!(
                program.is_some(),
                "{}: attest verify refused",
                file.display()
            );
            canary += 1;
        }
        let mut altered = bytes.clone();
        let middle = altered.len() / 2;
        altered[middle] ^= 1;
        assert!(
            judged_by_attest(&altered, &key, &dir).is_none(),
            "{}: altered",
            file.display()
        );
        assert!(
            check(&altered, &key, &trust).is_err(),
            "{}: altered",
            file.display()
        );
    }
    fs::remove_dir_all(&dir).unwrap();
    assert_eq!(canary, 40);
    println!("s_04_2: both accept {accepted}, both refuse {refused}, no committed key {unkeyed}");
}

fn openssl(key: &[u8], signature: &[u8], message: &[u8], dir: &Path) -> bool {
    let text = B64.encode(key);
    let lines: Vec<&str> = text
        .as_bytes()
        .chunks(64)
        .map(|chunk| std::str::from_utf8(chunk).unwrap())
        .collect();
    let pem = format!(
        "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
        lines.join("\n")
    );
    let (k, s, m) = (
        dir.join("key.pem"),
        dir.join("signature"),
        dir.join("message"),
    );
    fs::write(&k, pem).unwrap();
    fs::write(&s, signature).unwrap();
    fs::write(&m, message).unwrap();
    Command::new("openssl")
        .args(["dgst", "-sha256", "-verify"])
        .arg(&k)
        .arg("-signature")
        .arg(&s)
        .arg(&m)
        .output()
        .expect("openssl is the independent judge here")
        .status
        .success()
}

/// Every stored log reply under `dir`.
fn replies(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            replies(&path, found);
        } else if path.to_string_lossy().ends_with(".rekor.json") {
            found.push(path);
        }
    }
}

/// The log key for a reply: the `log.pub` of the nearest directory holding one, else the real log's.
fn log_key_for(file: &Path) -> Vec<u8> {
    let evidence = root().join("evidence");
    for dir in file.ancestors().skip(1) {
        if dir == evidence || dir == root() {
            break;
        }
        if let Ok(pem) = fs::read_to_string(dir.join("log.pub")) {
            return public_der(&pem).expect("a stored log key");
        }
    }
    B64.decode(REKOR_KEY).unwrap()
}

type Signed = (Vec<u8>, Vec<u8>, Vec<u8>);

/// Every signature a stored reply holds that can be read as one: the log's over the entry, its
/// over the tree, and the operator's over the link. A forged reply may lack any of them.
fn signatures(reply: &Value, link: Option<&[u8]>, log_key: &[u8]) -> Vec<Signed> {
    let mut held = Vec::new();
    let Some(entry) = reply.as_object().and_then(|o| o.values().next()) else {
        return held;
    };
    let Some(body_text) = entry["body"].as_str() else {
        return held;
    };
    if let (Some(time), Some(index), Some(Ok(signature))) = (
        entry["integratedTime"].as_u64(),
        entry["logIndex"].as_u64(),
        entry["verification"]["signedEntryTimestamp"]
            .as_str()
            .map(|text| B64.decode(text)),
    ) {
        let message = format!(
            "{{\"body\":{},\"integratedTime\":{time},\"logID\":{},\"logIndex\":{index}}}",
            serde_json::to_string(body_text).unwrap(),
            serde_json::to_string(&sha(log_key)).unwrap(),
        );
        held.push((log_key.to_vec(), signature, message.into_bytes()));
    }
    if let Some((note, lines)) = entry["verification"]["inclusionProof"]["checkpoint"]
        .as_str()
        .and_then(|text| text.split_once("\n\n"))
    {
        for line in lines.lines() {
            let signed = line
                .strip_prefix("— ")
                .and_then(|rest| rest.rsplit(' ').next())
                .and_then(|text| B64.decode(text).ok())
                .filter(|bytes| bytes.len() > 4);
            if let Some(signed) = signed {
                let message = format!("{note}\n").into_bytes();
                held.push((log_key.to_vec(), signed[4..].to_vec(), message));
            }
        }
    }
    let body: Option<Value> = B64
        .decode(body_text)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let on_link = body.as_ref().and_then(|body| {
        let pem = B64
            .decode(body["spec"]["signature"]["publicKey"]["content"].as_str()?)
            .ok()?;
        let key = public_der(std::str::from_utf8(&pem).ok()?)?;
        let signature = B64
            .decode(body["spec"]["signature"]["content"].as_str()?)
            .ok()?;
        Some((key, signature))
    });
    if let (Some((key, signature)), Some(link)) = (on_link, link) {
        held.push((key, signature, link.to_vec()));
    }
    held
}

#[test]
fn s_04_3_e1_e2_p256_and_openssl_agree_on_every_stored_signature() {
    let dir = scratch("signatures");
    let mut files = Vec::new();
    replies(&root().join("evidence"), &mut files);
    replies(&root().join("verify/tests/fixtures"), &mut files);
    files.sort();
    assert_eq!(files.len(), 396, "the committed log replies");
    let (mut accepted, mut refused, mut altered, mut unreadable) = (0, 0, 0, 0);
    let mut ledger = 0;
    for file in &files {
        let Ok(reply) = serde_json::from_slice::<Value>(&fs::read(file).unwrap()) else {
            unreadable += 1;
            continue;
        };
        let name = file.to_string_lossy();
        let link = fs::read(name.replace(".rekor.json", ".json")).ok();
        let held = signatures(&reply, link.as_deref(), &log_key_for(file));
        if held.is_empty() {
            unreadable += 1;
        }
        let main_ledger = file.parent() == Some(root().join("evidence/ledger").as_path());
        for (key, signature, message) in held {
            let judged = openssl(&key, &signature, &message, &dir);
            let inside = signature_verifies(&key, &signature, &message);
            assert_eq!(judged, inside, "{name}: openssl and p256 disagree");
            if judged {
                accepted += 1;
            } else {
                refused += 1;
            }
            if main_ledger {
                assert!(judged, "{name}: a signature of the real chain is refused");
                ledger += 1;
            }
            if signature.is_empty() {
                continue;
            }
            let mut flipped = signature.clone();
            *flipped.last_mut().unwrap() ^= 1;
            let judged = openssl(&key, &flipped, &message, &dir);
            assert_eq!(
                judged,
                signature_verifies(&key, &flipped, &message),
                "{name}: altered"
            );
            assert!(!judged, "{name}: an altered signature is accepted");
            altered += 1;
        }
    }
    fs::remove_dir_all(&dir).unwrap();
    assert_eq!(ledger, 36, "the real chain's signatures");
    println!(
        "s_04_3: {} replies, both accept {accepted}, both refuse {refused}, altered and both refuse {altered}, replies with no readable signature {unreadable}",
        files.len()
    );
}
