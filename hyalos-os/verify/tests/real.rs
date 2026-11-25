//! S-06: a real committed record against the public log, and the documents held to the code. A test
//! that needs the log skips when it cannot be reached, unless `HYALOS_REQUIRE_PUBLIC_LOG` is set.

// Not yet on the `unsafe` list of design.md §2 (docs/plans/hyalos-kernel-core), which predates
// this test; Slice 10's list takes it in or this test drops its `unsafe` (progress/slice-09.md).
#![allow(unsafe_code)]

mod common;

use common::root;
use std::{
    env, fs,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

const CANARY: &str = "evidence/out/two-machines-canary";
const OPERATOR: &str = "evidence/out/two-machines-slice5/operator.pub";

fn at(path: &str) -> PathBuf {
    root().join(path)
}

fn verify(record: &Path, links: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hyalos-verify"))
        .arg(record)
        .arg(links)
        .arg("--operator-public")
        .arg(at(OPERATOR))
        .arg("--notary-public")
        .arg(at(&format!("{CANARY}/notary.pub")))
        .arg("--guest")
        .arg(at("guest/digest.wasm"))
        .env_clear()
        .env("PATH", "")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

/// The real record's verdict, or `None` when the log could not be asked and that is allowed.
fn judged(test: &str, record: &Path, links: &Path) -> Option<Output> {
    let output = verify(record, links);
    let said = String::from_utf8_lossy(&output.stderr);
    if output.status.code() == Some(2) && said.contains("public log") {
        assert!(
            env::var_os("HYALOS_REQUIRE_PUBLIC_LOG").is_none(),
            "{test}: {said}"
        );
        eprintln!("skipped: {test} cannot reach the public log");
        return None;
    }
    Some(output)
}

struct Copy(PathBuf);

impl Copy {
    fn new(label: &str) -> Self {
        let dir = env::temp_dir().join(format!("hyalos-real-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("record.json.attest")).unwrap();
        fs::create_dir_all(dir.join("links")).unwrap();
        let source = at(CANARY);
        for name in ["record.json", "record.json.salt"] {
            fs::copy(source.join(name), dir.join(name)).unwrap();
        }
        for (from, to) in [
            (
                source.join("record.json.attest"),
                dir.join("record.json.attest"),
            ),
            (at("evidence/ledger"), dir.join("links")),
        ] {
            for file in fs::read_dir(from).unwrap() {
                let file = file.unwrap();
                fs::copy(file.path(), to.join(file.file_name())).unwrap();
            }
        }
        Self(dir)
    }

    fn refused(&self, test: &str) {
        let Some(output) = judged(test, &self.0.join("record.json"), &self.0.join("links")) else {
            return;
        };
        assert_eq!(
            output.status.code(),
            Some(1),
            "{test}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty(), "{test}: a report was printed");
    }
}

impl Drop for Copy {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn s_06_1_e1_a_real_record_passes_against_the_public_log() {
    let test = "s_06_1_e1";
    let record = at(&format!("{CANARY}/record.json"));
    let Some(output) = judged(test, &record, &at("evidence/ledger")) else {
        return;
    };
    let report = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(report.starts_with("audit passed\n"), "{report}");
    assert!(report.contains("under the public log's key"), "{report}");
    assert!(!report.contains("test trust"), "{report}");
}

/// Makes every `connect` fail in the child, so it has no network, without any privilege.
fn no_network() -> std::io::Result<()> {
    const LOAD: u16 = 0x20;
    const JUMP_IF_EQUAL: u16 = 0x15;
    const RETURN: u16 = 0x06;
    const X86_64: u32 = 0xc000_003e;
    const FAIL: u32 = 0x0005_0000 | libc::ENETUNREACH as u32;
    const ALLOW: u32 = 0x7fff_0000;
    let step = |code, jt, jf, k| libc::sock_filter { code, jt, jf, k };
    let mut filter = [
        step(LOAD, 0, 0, 4),
        step(JUMP_IF_EQUAL, 0, 3, X86_64),
        step(LOAD, 0, 0, 0),
        step(JUMP_IF_EQUAL, 0, 1, libc::SYS_connect as u32),
        step(RETURN, 0, 0, FAIL),
        step(RETURN, 0, 0, ALLOW),
    ];
    let program = libc::sock_fprog {
        len: filter.len() as u16,
        filter: filter.as_mut_ptr(),
    };
    // SAFETY: the filter outlives both calls, and the kernel copies it.
    unsafe {
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
            || libc::prctl(
                libc::PR_SET_SECCOMP,
                libc::SECCOMP_MODE_FILTER,
                &program as *const libc::sock_fprog,
            ) != 0
        {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

#[test]
fn s_06_1_e2_with_no_network_it_is_inconclusive() {
    let mut command = Command::new(env!("CARGO_BIN_EXE_hyalos-verify"));
    command
        .arg(at(&format!("{CANARY}/record.json")))
        .arg(at("evidence/ledger"))
        .arg("--operator-public")
        .arg(at(OPERATOR))
        .arg("--notary-public")
        .arg(at(&format!("{CANARY}/notary.pub")))
        .arg("--guest")
        .arg(at("guest/digest.wasm"))
        .env_clear()
        .env("PATH", "")
        .stdin(Stdio::null());
    // SAFETY: the closure makes only async-signal-safe system calls.
    unsafe { command.pre_exec(no_network) };
    let output = command.output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty(), "a report was printed");
    assert!(String::from_utf8_lossy(&output.stderr).contains("public log"));
}

#[test]
fn s_06_2_e1_one_changed_byte_in_the_transcript_does_not_pass() {
    let copy = Copy::new("transcript");
    let file = copy.0.join("record.json");
    let mut bytes = fs::read(&file).unwrap();
    let field = bytes
        .windows(11)
        .position(|w| w == b"\"fuel_used\"")
        .unwrap();
    let digit = field + bytes[field..].iter().position(u8::is_ascii_digit).unwrap();
    bytes[digit] = if bytes[digit] == b'9' {
        b'8'
    } else {
        bytes[digit] + 1
    };
    fs::write(&file, bytes).unwrap();
    copy.refused("s_06_2_e1");
}

#[test]
fn s_06_2_e2_one_changed_byte_in_a_presentation_does_not_pass() {
    let copy = Copy::new("presentation");
    let file = copy.0.join("record.json.attest/000.presentation");
    let mut bytes = fs::read(&file).unwrap();
    let middle = bytes.len() / 2;
    bytes[middle] ^= 1;
    fs::write(&file, bytes).unwrap();
    copy.refused("s_06_2_e2 presentation");
}

#[test]
fn s_06_2_e2_one_changed_byte_in_a_link_does_not_pass() {
    let copy = Copy::new("link");
    let file = copy.0.join("links/00011.json");
    let mut bytes = fs::read(&file).unwrap();
    let middle = bytes.len() / 2;
    bytes[middle] ^= 1;
    fs::write(&file, bytes).unwrap();
    copy.refused("s_06_2_e2 link");
}

fn guide() -> String {
    fs::read_to_string(root().join("../docs/operations/verify-a-hyalos-record.md")).unwrap()
}

fn record_doc() -> String {
    fs::read_to_string(at("RECORD.md")).unwrap()
}

#[test]
fn s_06_3_e2_local_7_3_the_guide_lists_every_limit_the_report_can_print() {
    let guide = guide();
    let mut found = Vec::new();
    for file in fs::read_dir(at("audit/src")).unwrap() {
        let text = fs::read_to_string(file.unwrap().path()).unwrap();
        for (start, _) in text.match_indices("\"not proven: ") {
            let body = &text[start + 1..];
            let end = body.find('"').unwrap();
            found.push(body[..end].to_string());
        }
    }
    assert_eq!(found.len(), 9, "{found:?}");
    for line in found {
        assert!(
            guide.contains(&format!("`{line}`")),
            "the guide lacks: {line}"
        );
    }
}

#[test]
fn local_7_1_the_guide_says_whom_not_to_trust() {
    let guide = guide();
    for needed in [
        "**Do not take the verifier from the operator.**",
        "**You.** Use the key of the notary you chose",
        "**A separate channel**",
        "A pass holds only under the keys you give it.",
        "release-verifier.yml --source-ref \"refs/tags/$tag\"",
        ".github/scripts/build-verifier.sh dist",
    ] {
        assert!(guide.contains(needed), "{needed}");
    }
}

#[test]
fn s_06_4_e1_the_exit_codes_in_the_record_format_are_the_verifiers() {
    let doc = record_doc();
    for row in ["| 0 | Passed |", "| 1 | Refused |", "| 2 | Inconclusive |"] {
        assert!(doc.contains(row), "{row}");
    }
    let unknown = Command::new(env!("CARGO_BIN_EXE_hyalos-verify"))
        .arg("--not-an-option")
        .env_clear()
        .output()
        .unwrap();
    assert_eq!(unknown.status.code(), Some(2));
    let dir = env::temp_dir().join(format!("hyalos-real-exit-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let record = dir.join("record.json");
    fs::write(&record, b"{}").unwrap();
    let refused = verify(&record, &at("evidence/ledger"));
    fs::remove_dir_all(&dir).unwrap();
    assert_eq!(
        refused.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
}

#[test]
fn s_06_4_e2_the_size_limits_in_the_record_format_are_the_verifiers() {
    let doc = record_doc();
    for row in [
        "| Transcript | 16 MiB |",
        "| One presentation | 1 MiB |",
        "| One link file | 64 KiB |",
        "| Guest | 16 MiB |",
        "| Key file | 4 KiB |",
        "| Salt | 4 KiB |",
        "| One public log reply | 1 MiB |",
        "| Entries listed under one key | 10,000 |",
    ] {
        assert!(doc.contains(row), "{row}");
    }
    let main = fs::read_to_string(at("verify/src/main.rs")).unwrap();
    for code in [
        "FileKind::Transcript | FileKind::Guest => 16 * MIB",
        "FileKind::Presentation => MIB",
        "FileKind::Link => 64 * 1024",
        "FileKind::Key | FileKind::Salt => 4 * 1024",
    ] {
        assert!(main.contains(code), "{code}");
    }
    let chain = fs::read_to_string(at("audit/src/chain.rs")).unwrap();
    assert!(chain.contains("MAX_LOG_REPLY: u64 = 1 << 20"));
    assert!(chain.contains("MAX_LOG_ENTRIES: usize = 10_000"));
}

#[test]
fn s_06_4_the_record_format_describes_version_4_the_profile_and_the_options() {
    let doc = record_doc();
    for needed in [
        "Version 4",
        "hyalos.profile/1",
        "--operator-public",
        "--notary-public",
        "--guest",
        "--legacy-v1",
        "--version",
    ] {
        assert!(doc.contains(needed), "{needed}");
    }
}

#[test]
fn local_7_4_the_public_log_requirement_is_listed() {
    let rules = fs::read_to_string(root().join("../CLAUDE.md")).unwrap();
    assert!(rules.contains("`_PUBLIC_LOG`"));
}
