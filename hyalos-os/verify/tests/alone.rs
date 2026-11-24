//! S-03.5, S-04.1 and 5.2: the verifier runs no program, writes no file, reads no home directory,
//! and holds the public log's replies to their limits.

mod common;

use base64::{Engine, engine::general_purpose::STANDARD as B64};
use common::{fixtures, root, verify};
use hyalos_audit::{AuditInputs, LogSource, PlainFiles, STAND_IN_BUILD, Verdict, Verifier, check};
use serde_json::Value;
use std::{
    env, fs,
    io::{Read, Write},
    net::TcpListener,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

const CANARY: &str = "evidence/out/two-machines-canary";
const SLICE5: &str = "evidence/out/two-machines-slice5";

fn stand_in(test: &str) -> bool {
    if !STAND_IN_BUILD {
        eprintln!("skipped: {test} needs the test-trust feature");
    }
    STAND_IN_BUILD
}

fn at(path: &str) -> String {
    root().join(path).display().to_string()
}

struct Temp(PathBuf);

impl Temp {
    fn new(label: &str) -> Self {
        let dir = env::temp_dir().join(format!("hyalos-alone-{label}-{}", std::process::id()));
        Self::clear(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn clear(dir: &Path) {
        if dir.exists() {
            let _ = Command::new("chmod")
                .args(["-R", "u+rwx"])
                .arg(dir)
                .status();
            let _ = fs::remove_dir_all(dir);
        }
    }

    fn dir(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(&path).unwrap();
        path
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        Self::clear(&self.0);
    }
}

/// The real canary record, against a stand-in log of the committed replies.
fn real_record(links: &str, cwd: &Path, path: &str, home: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_hyalos-verify"));
    command
        .arg(at(&format!("{CANARY}/record.json")))
        .arg(links)
        .args(["--operator-public", &at(&format!("{SLICE5}/operator.pub"))])
        .args(["--notary-public", &at(&format!("{CANARY}/notary.pub"))])
        .args(["--guest", &at("guest/digest.wasm")])
        .current_dir(cwd)
        .env_clear()
        .env("PATH", path)
        .env("HOME", home)
        .env("TMPDIR", "/nonexistent")
        .env("HYALOS_TRUST_LOG_DIR", at("evidence/ledger"))
        .env("HYALOS_TRUST_LOG_KEY", at("audit/tests/golden/log.pub"))
        .stdin(Stdio::null());
    command
}

fn passed(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.starts_with(b"audit passed"));
}

#[test]
fn s_03_5_e1_an_unreadable_home_is_not_needed_and_no_file_appears() {
    if !stand_in("s_03_5_e1") {
        return;
    }
    let temp = Temp::new("s-03-5-e1");
    let (home, cwd) = (temp.dir("home"), temp.dir("cwd"));
    fs::set_permissions(&home, fs::Permissions::from_mode(0o000)).unwrap();
    let output = real_record(&at("evidence/ledger"), &cwd, "", &home)
        .output()
        .unwrap();
    passed(&output);
    assert_eq!(fs::read_dir(&cwd).unwrap().count(), 0, "a file appeared");
}

#[test]
fn s_03_5_e2_it_completes_where_it_cannot_write_a_byte() {
    if !stand_in("s_03_5_e2") {
        return;
    }
    let temp = Temp::new("s-03-5-e2");
    let (home, cwd) = (temp.dir("home"), temp.dir("cwd"));
    fs::set_permissions(&cwd, fs::Permissions::from_mode(0o500)).unwrap();
    let mut command = real_record(&at("evidence/ledger"), &cwd, "", &home);
    // Any write to a regular file, in `/tmp` or anywhere else, now fails and kills the process.
    unsafe {
        command.pre_exec(|| {
            let none = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            if libc::setrlimit(libc::RLIMIT_FSIZE, &none) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    passed(&command.output().unwrap());
    for dir in ["audit/src", "verify/src", "attest-check/src"] {
        for file in fs::read_dir(at(dir)).unwrap() {
            let path = file.unwrap().path();
            let text = fs::read_to_string(&path).unwrap();
            for token in [
                "/tmp",
                "temp_dir",
                "\"HOME\"",
                "home_dir",
                "process::Command",
            ] {
                assert!(!text.contains(token), "{}: {token}", path.display());
            }
        }
    }
}

#[test]
fn s_04_1_e1_it_passes_a_real_record_with_an_empty_path() {
    if !stand_in("s_04_1_e1") {
        return;
    }
    let temp = Temp::new("s-04-1-e1");
    let (home, cwd) = (temp.dir("home"), temp.dir("cwd"));
    passed(
        &real_record(&at("evidence/ledger"), &cwd, "", &home)
            .output()
            .unwrap(),
    );
}

#[test]
fn s_04_1_e2_a_decoy_openssl_never_runs_and_a_forged_signature_is_refused() {
    if !stand_in("s_04_1_e2") {
        return;
    }
    let temp = Temp::new("s-04-1-e2");
    let (home, cwd, tools, links) = (
        temp.dir("home"),
        temp.dir("cwd"),
        temp.dir("tools"),
        temp.dir("links"),
    );
    let ran = temp.0.join("the-decoy-ran");
    for name in ["openssl", "attest"] {
        let decoy = tools.join(name);
        fs::write(
            &decoy,
            format!("#!/bin/sh\n: > '{}'\nexit 0\n", ran.display()),
        )
        .unwrap();
        fs::set_permissions(&decoy, fs::Permissions::from_mode(0o700)).unwrap();
    }
    for file in fs::read_dir(at("evidence/ledger")).unwrap() {
        let file = file.unwrap();
        fs::copy(file.path(), links.join(file.file_name())).unwrap();
    }
    let reply = links.join("00000.rekor.json");
    let mut held: Value = serde_json::from_slice(&fs::read(&reply).unwrap()).unwrap();
    let id = held.as_object().unwrap().keys().next().unwrap().clone();
    let field = &mut held[&id]["verification"]["signedEntryTimestamp"];
    let mut signature = B64.decode(field.as_str().unwrap()).unwrap();
    *signature.last_mut().unwrap() ^= 1;
    *field = Value::String(B64.encode(signature));
    fs::write(&reply, serde_json::to_vec(&held).unwrap()).unwrap();
    let output = real_record(
        links.to_str().unwrap(),
        &cwd,
        tools.to_str().unwrap(),
        &home,
    )
    .output()
    .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim(),
        "refused: link 0: the signed time does not verify"
    );
    assert!(!ran.exists(), "the decoy ran");
}

#[test]
fn local_5_6_the_five_answer_record_verifies_inside_the_time_limit() {
    if !stand_in("local_5_6") {
        return;
    }
    let started = std::time::Instant::now();
    let output = verify(&fixtures().join("pass/five-answers"));
    let took = started.elapsed();
    assert_eq!(output.status.code(), Some(0));
    println!("local_5_6: five answers verified in {took:?}");
    assert!(took.as_secs() < 10, "{took:?}");
}

/// A log that gives `reply` to the entry list, and counts what is asked of it.
fn serve(reply: Vec<u8>, status: &'static str) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let fetched = Arc::new(AtomicUsize::new(0));
    let counted = fetched.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            let mut wanted = None;
            while let Ok(read) = stream.read(&mut chunk) {
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
                let end = request.windows(4).position(|w| w == b"\r\n\r\n");
                if let (None, Some(end)) = (wanted, end) {
                    let head = String::from_utf8_lossy(&request[..end]).to_lowercase();
                    let length = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length: "))
                        .and_then(|n| n.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    wanted = Some(end + 4 + length);
                }
                if wanted.is_some_and(|size| request.len() >= size) {
                    break;
                }
            }
            if request.starts_with(b"POST") {
                let head = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\nlocation: /elsewhere\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    reply.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&reply);
            } else {
                counted.fetch_add(1, Ordering::SeqCst);
                let _ = stream.write_all(
                    b"HTTP/1.1 500 No\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                );
            }
        }
    });
    (base, fetched)
}

fn against(base: String) -> Verdict {
    let inputs = AuditInputs {
        dir: Some(at("evidence/ledger")),
        operator: Some(at(&format!("{SLICE5}/operator.pub"))),
        notary: fs::read_to_string(at(&format!("{CANARY}/notary.pub")))
            .unwrap()
            .trim()
            .to_string(),
        legacy: false,
        guest: Some(at("guest/digest.wasm")),
    };
    check(
        &at(&format!("{CANARY}/record.json")),
        &inputs,
        LogSource::public_at(base),
        &Verifier::real(),
        &PlainFiles,
    )
}

fn inconclusive(verdict: &Verdict) {
    assert!(
        matches!(verdict, Verdict::Inconclusive(what) if what.contains("public log")),
        "{verdict:?}"
    );
}

#[test]
fn local_5_2_a_log_reply_over_its_limit_is_inconclusive() {
    if !stand_in("local_5_2") {
        return;
    }
    let mut over = b"[]".to_vec();
    over.resize((1 << 20) + 1, b' ');
    inconclusive(&against(serve(over, "200 OK").0));
    let mut exact = b"[]".to_vec();
    exact.resize(1 << 20, b' ');
    let verdict = against(serve(exact, "200 OK").0);
    assert!(matches!(verdict, Verdict::Refused(_)), "{verdict:?}");
}

#[test]
fn local_5_2_a_list_over_its_limit_is_inconclusive_before_any_entry_is_fetched() {
    if !stand_in("local_5_2") {
        return;
    }
    let list = |count: usize| serde_json::to_vec(&vec!["an-entry"; count]).unwrap();
    let (base, fetched) = serve(list(10_001), "200 OK");
    inconclusive(&against(base));
    assert_eq!(fetched.load(Ordering::SeqCst), 0);
    let (base, fetched) = serve(list(10_000), "200 OK");
    inconclusive(&against(base));
    assert_eq!(
        fetched.load(Ordering::SeqCst),
        1,
        "the limit itself is allowed"
    );
}

#[test]
fn local_5_2_a_redirect_is_not_followed() {
    if !stand_in("local_5_2") {
        return;
    }
    let (base, fetched) = serve(b"[]".to_vec(), "302 Found");
    inconclusive(&against(base));
    assert_eq!(
        fetched.load(Ordering::SeqCst),
        0,
        "the redirect was followed"
    );
}
