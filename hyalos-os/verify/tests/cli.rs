//! `hyalos-verify` as a customer runs it. Tests that need the stand-in log say so and skip in a
//! build without `test-trust`. CI runs them with it.

use hyalos_audit::{AuditInputs, LogSource, PlainFiles, STAND_IN_BUILD, Verdict, Verifier, check};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

const SLICE5: &str = "evidence/out/two-machines-slice5";
const CANARY: &str = "evidence/out/two-machines-canary";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn at(path: &str) -> String {
    root().join(path).display().to_string()
}

fn stand_in(test: &str) -> bool {
    if !STAND_IN_BUILD {
        eprintln!("skipped: {test} needs the test-trust feature");
    }
    STAND_IN_BUILD
}

struct Temp(PathBuf);

impl Temp {
    fn new(label: &str) -> Self {
        let dir = env::temp_dir().join(format!("hyalos-verify-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).display().to_string()
    }

    /// A copy of the canary record and its attestations.
    fn canary(&self) -> String {
        let record = self.path("record.json");
        fs::copy(at(&format!("{CANARY}/record.json")), &record).unwrap();
        fs::copy(
            at(&format!("{CANARY}/record.json.salt")),
            self.path("record.json.salt"),
        )
        .unwrap();
        fs::create_dir_all(self.path("record.json.attest")).unwrap();
        for file in fs::read_dir(at(&format!("{CANARY}/record.json.attest"))).unwrap() {
            let file = file.unwrap();
            fs::copy(
                file.path(),
                self.0.join("record.json.attest").join(file.file_name()),
            )
            .unwrap();
        }
        record
    }

    /// A copy of the committed links and their stored replies.
    fn ledger(&self) -> String {
        let dir = self.path("ledger");
        fs::create_dir_all(&dir).unwrap();
        for file in fs::read_dir(at("evidence/ledger")).unwrap() {
            let file = file.unwrap();
            fs::copy(file.path(), self.0.join("ledger").join(file.file_name())).unwrap();
        }
        dir
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// One check of `record`, with the committed keys and a stand-in log of `log`.
struct Check {
    record: String,
    links: String,
    log: String,
    operator: String,
    notary: String,
    verify: String,
    guest: Option<String>,
    legacy: bool,
}

impl Check {
    fn of(record: String, links: String) -> Self {
        Self {
            record,
            log: links.clone(),
            links,
            operator: at(&format!("{SLICE5}/operator.pub")),
            notary: at(&format!("{CANARY}/notary.pub")),
            verify: at("target/release/attest"),
            guest: Some(at("guest/digest.wasm")),
            legacy: false,
        }
    }

    fn canary() -> Self {
        Self::of(at(&format!("{CANARY}/record.json")), at("evidence/ledger"))
    }

    fn run(&self) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_hyalos-verify"));
        command
            .arg(&self.record)
            .arg(&self.links)
            .args(["--operator-public", &self.operator])
            .args(["--notary-public", &self.notary])
            .args(["--verify", &self.verify])
            .env_clear()
            .env("PATH", env::var("PATH").unwrap())
            .env("HOME", "/nonexistent")
            .env("HYALOS_TRUST_LOG_DIR", &self.log)
            .env("HYALOS_TRUST_LOG_KEY", at("audit/tests/golden/log.pub"))
            .stdin(Stdio::null());
        if let Some(guest) = &self.guest {
            command.args(["--guest", guest]);
        }
        if self.legacy {
            command.arg("--legacy-v1");
        }
        command.output().unwrap()
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_string()
}

fn no_pass(output: &Output) {
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        !text
            .lines()
            .any(|l| l.starts_with("audit passed") || l.starts_with("legacy audit passed")),
        "{text}"
    );
    assert!(output.stdout.is_empty(), "a refusal prints no report");
}

fn expect(output: &Output, code: i32, says: &str) {
    assert_eq!(output.status.code(), Some(code), "{}", stderr(output));
    no_pass(output);
    let word = if code == 1 {
        "refused: "
    } else {
        "inconclusive: "
    };
    assert!(stderr(output).starts_with(word), "{}", stderr(output));
    assert!(stderr(output).contains(says), "{}", stderr(output));
}

#[test]
fn s_03_1_every_committed_record_gives_its_golden_report() {
    if !stand_in("s_03_1") {
        return;
    }
    for (dir, notary, legacy) in [
        ("two-machines", SLICE5, true),
        ("two-machines-slice5", SLICE5, true),
        (
            "two-machines-record",
            "evidence/out/two-machines-record",
            false,
        ),
        ("two-machines-canary", CANARY, false),
    ] {
        let mut one = Check::of(
            at(&format!("evidence/out/{dir}/record.json")),
            at("evidence/ledger"),
        );
        one.notary = at(&format!("{notary}/notary.pub"));
        one.legacy = legacy;
        let output = one.run();
        assert_eq!(output.status.code(), Some(0), "{dir}: {}", stderr(&output));
        let golden = fs::read(at(&format!(
            "audit/tests/golden/evidence-out-{dir}-record.json.report"
        )))
        .unwrap();
        assert_eq!(output.stdout, golden, "{dir}");
    }
}

#[test]
fn s_03_1_e1_the_canary_record_matches_the_monitors_audit() {
    if !stand_in("s_03_1_e1") {
        return;
    }
    let one = Check::canary();
    let verified = one.run();
    assert_eq!(verified.status.code(), Some(0), "{}", stderr(&verified));
    let audited = Command::new(at("target/release/task-kernel"))
        .arg("audit")
        .arg(&one.record)
        .arg(&one.links)
        .args(["--operator-public", &one.operator])
        .args(["--notary-public", &one.notary])
        .args(["--verify", &one.verify])
        .args(["--guest", one.guest.as_ref().unwrap()])
        .env_clear()
        .env("PATH", env::var("PATH").unwrap())
        .env("HYALOS_TRUST_LOG_DIR", &one.log)
        .env("HYALOS_TRUST_LOG_KEY", at("audit/tests/golden/log.pub"))
        .output()
        .unwrap();
    assert_eq!(audited.status.code(), Some(0), "{}", stderr(&audited));
    assert_eq!(verified.stdout, audited.stdout);
}

#[test]
fn s_03_1_e2_a_version_1_record_without_the_option_is_refused_naming_it() {
    let mut one = Check::of(at(&format!("{SLICE5}/record.json")), at("evidence/ledger"));
    one.notary = at(&format!("{SLICE5}/notary.pub"));
    expect(&one.run(), 1, "--legacy-v1");
}

#[test]
fn s_03_3_e1_a_public_log_that_cannot_be_reached_is_inconclusive() {
    if !stand_in("s_03_3_e1") {
        return;
    }
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    let inputs = AuditInputs {
        dir: Some(at("evidence/ledger")),
        operator: Some(at(&format!("{SLICE5}/operator.pub"))),
        notary: fs::read_to_string(at(&format!("{CANARY}/notary.pub")))
            .unwrap()
            .trim()
            .to_string(),
        verify: at("target/release/attest"),
        legacy: false,
        guest: Some(at("guest/digest.wasm")),
    };
    let verdict = check(
        &at(&format!("{CANARY}/record.json")),
        &inputs,
        log_at(base),
        &Verifier {
            program: inputs.verify.clone(),
            trust: Vec::new(),
        },
        &PlainFiles,
    );
    assert!(
        matches!(verdict, Verdict::Inconclusive(ref what) if what.contains("public log")),
        "{verdict:?}"
    );
}

fn log_at(base: String) -> LogSource {
    LogSource::public_at(base)
}

#[test]
fn s_03_3_e2_a_missing_key_file_is_inconclusive_naming_its_role() {
    let mut operator = Check::canary();
    operator.operator = "/nonexistent/operator.pub".to_string();
    expect(&operator.run(), 2, "operator's public key");
    let mut notary = Check::canary();
    notary.notary = "/nonexistent/notary.pub".to_string();
    expect(&notary.run(), 2, "notary's public key");
}

fn cargo_tree(args: &[&str]) -> String {
    let output = Command::new(env::var_os("CARGO").unwrap())
        .current_dir(root())
        .args(["tree", "--locked", "-p", "hyalos-verify"])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn s_03_4_e1_the_dependency_tree_holds_no_task_running_code() {
    let tree = cargo_tree(&["-e", "normal", "--prefix", "none"]);
    assert!(tree.starts_with("hyalos-verify "), "{tree}");
    for line in tree.lines() {
        let name = line.split(' ').next().unwrap();
        assert!(
            ![
                "kvm-ioctls",
                "kvm-bindings",
                "tlsn",
                "tokio-rustls",
                "rcgen",
                "task-kernel"
            ]
            .contains(&name),
            "{line}"
        );
    }
}

#[test]
fn s_03_4_e2_the_source_has_no_live_kvm_or_signing_code() {
    for dir in ["verify/src", "audit/src"] {
        for file in fs::read_dir(at(dir)).unwrap() {
            let path = file.unwrap().path();
            let text = fs::read_to_string(&path).unwrap();
            for token in [
                "kvm_ioctls",
                "kvm_bindings",
                "kvm::",
                "Live",
                "\"-sign\"",
                "genpkey",
            ] {
                assert!(!text.contains(token), "{}: {token}", path.display());
            }
        }
    }
}

#[test]
fn local_3_2_no_release_selection_enables_test_trust() {
    let tree = cargo_tree(&["-e", "features"]);
    assert!(!tree.contains("test-trust"), "{tree}");
}

#[test]
fn s_03_6_e1_a_presentation_of_exactly_the_limit_proceeds_to_the_check() {
    if !stand_in("s_03_6_e1") {
        return;
    }
    let temp = Temp::new("s-03-6-e1");
    let record = temp.canary();
    fs::write(
        temp.path("record.json.attest/000.presentation"),
        vec![0u8; 1024 * 1024],
    )
    .unwrap();
    let output = Check::of(record, at("evidence/ledger")).run();
    expect(
        &output,
        1,
        "answer 0: the attestation is not the one the record names",
    );
}

#[test]
fn s_03_6_e2_a_presentation_over_the_limit_is_refused_naming_it() {
    if !stand_in("s_03_6_e2") {
        return;
    }
    let temp = Temp::new("s-03-6-e2");
    let record = temp.canary();
    fs::write(
        temp.path("record.json.attest/000.presentation"),
        vec![0u8; 1024 * 1024 + 1],
    )
    .unwrap();
    let output = Check::of(record, at("evidence/ledger")).run();
    expect(
        &output,
        1,
        "answer 0: the attestation is over its size limit",
    );
}

#[test]
fn s_03_6_every_local_input_over_its_limit_is_refused_before_parsing() {
    let temp = Temp::new("s-03-6-all");
    let large = |name: &str, size: u64| {
        let path = temp.path(name);
        fs::File::create(&path).unwrap().set_len(size).unwrap();
        path
    };
    let mut record = Check::canary();
    record.record = large("big-record", 16 * 1024 * 1024 + 1);
    expect(&record.run(), 1, "the record is over its size limit");
    let mut key = Check::canary();
    key.operator = large("big-operator", 4097);
    if STAND_IN_BUILD {
        expect(
            &key.run(),
            1,
            "the operator's public key is over its size limit",
        );
    }
    let mut notary = Check::canary();
    notary.notary = large("big-notary", 4097);
    expect(
        &notary.run(),
        1,
        "the notary's public key is over its size limit",
    );
    let mut guest = Check::canary();
    guest.guest = Some(large("big-guest", 16 * 1024 * 1024 + 1));
    expect(&guest.run(), 1, "the guest is over its size limit");
    if STAND_IN_BUILD {
        let copy = Temp::new("s-03-6-salt");
        let saved = copy.canary();
        fs::File::create(copy.path("record.json.salt"))
            .unwrap()
            .set_len(4097)
            .unwrap();
        expect(
            &Check::of(saved, at("evidence/ledger")).run(),
            1,
            "the salt is over its size limit",
        );
        let links = Temp::new("s-03-6-link");
        let dir = links.ledger();
        fs::File::create(format!("{dir}/00003.json"))
            .unwrap()
            .set_len(64 * 1024 + 1)
            .unwrap();
        let mut link = Check::canary();
        link.links = dir;
        expect(&link.run(), 1, "link 3: it is over its size limit");
    }
}

/// Every way the audit can stop, with the exit code the design gives it.
#[test]
fn local_3_3_every_audit_failure_is_refused_or_inconclusive() {
    let temp = Temp::new("local-3-3");
    let plain = |file: &str| temp.path(file);
    fs::write(plain("not-a-key"), "not a key\n").unwrap();
    fs::write(plain("not-a-record"), "not json").unwrap();
    fs::write(plain("not-a-program"), "").unwrap();
    let other_guest = plain("other-guest");
    fs::write(&other_guest, b"another guest").unwrap();
    type Change = fn(&mut Check, &Temp);
    let rows: [(&str, Change, i32, &str); 11] = [
        (
            "record missing",
            |c, _| c.record = "/nonexistent/record".into(),
            2,
            "cannot read",
        ),
        (
            "record unparsable",
            |c, t| c.record = t.path("not-a-record"),
            1,
            "cannot parse",
        ),
        (
            "record is a directory",
            |c, _| c.record = "/".into(),
            2,
            "cannot read",
        ),
        (
            "operator key missing",
            |c, _| c.operator = "/nonexistent/key".into(),
            2,
            "operator's public key",
        ),
        (
            "operator key not a key",
            |c, t| c.operator = t.path("not-a-key"),
            1,
            "operator's public key",
        ),
        (
            "notary key missing",
            |c, _| c.notary = "/nonexistent/key".into(),
            2,
            "notary's public key",
        ),
        (
            "notary key not a key",
            |c, t| c.notary = t.path("not-a-key"),
            1,
            "not a signing public key",
        ),
        (
            "guest not given",
            |c, _| c.guest = None,
            2,
            "the guest is not given",
        ),
        (
            "guest missing",
            |c, _| c.guest = Some("/nonexistent/guest".into()),
            2,
            "not a readable file",
        ),
        (
            "guest is another's",
            |c, t| c.guest = Some(t.path("other-guest")),
            1,
            "the guest is not this record's",
        ),
        (
            "verify program missing",
            |c, _| c.verify = "/nonexistent/attest".into(),
            2,
            "cannot be launched",
        ),
    ];
    for (name, change, code, says) in rows {
        let mut one = Check::canary();
        change(&mut one, &temp);
        let output = one.run();
        assert_eq!(
            output.status.code(),
            Some(code),
            "{name}: {}",
            stderr(&output)
        );
        expect(&output, code, says);
    }
    if !stand_in("local_3_3 evidence rows") {
        return;
    }
    let salt = Temp::new("local-3-3-salt");
    let record = salt.canary();
    fs::remove_file(salt.path("record.json.salt")).unwrap();
    expect(
        &Check::of(record, at("evidence/ledger")).run(),
        1,
        "no salt",
    );
    let answer = Temp::new("local-3-3-answer");
    let record = answer.canary();
    fs::remove_file(answer.path("record.json.attest/000.presentation")).unwrap();
    expect(
        &Check::of(record, at("evidence/ledger")).run(),
        1,
        "answer 0: no attestation",
    );
    let unlaunchable = Temp::new("local-3-3-launch");
    let program = unlaunchable.path("not-executable");
    fs::write(&program, "").unwrap();
    let mut one = Check::canary();
    one.verify = program;
    expect(&one.run(), 2, "cannot run the verify program");
}

#[test]
fn local_3_4_the_verify_program_is_required_until_slice_5() {
    let one = Check::canary();
    let output = Command::new(env!("CARGO_BIN_EXE_hyalos-verify"))
        .arg(&one.record)
        .arg(&one.links)
        .args([
            "--operator-public",
            &one.operator,
            "--notary-public",
            &one.notary,
        ])
        .env_clear()
        .output()
        .unwrap();
    expect(&output, 2, "the verify program is not given");
}

#[test]
fn local_3_cli_unknown_options_and_missing_arguments_are_inconclusive() {
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_hyalos-verify"))
            .args(args)
            .env_clear()
            .output()
            .unwrap()
    };
    expect(&run(&[]), 2, "usage");
    expect(&run(&["only-a-record"]), 2, "usage");
    expect(&run(&["a", "b", "--unknown"]), 2, "unknown option");
    expect(&run(&["a", "b", "--guest"]), 2, "--guest needs a value");
}

#[test]
fn local_3_cli_version_names_what_this_build_reads() {
    let output = Command::new(env!("CARGO_BIN_EXE_hyalos-verify"))
        .arg("--version")
        .env_clear()
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[0],
        concat!("hyalos-verify ", env!("CARGO_PKG_VERSION"))
    );
    assert!(
        lines[1] == "commit unknown" || (lines[1].len() == 47 && lines[1].starts_with("commit "))
    );
    assert_eq!(&lines[2..], ["records 1-4", "profiles hyalos.profile/1"]);
}

#[test]
fn local_3_7_an_altered_chain_or_answer_is_refused() {
    if !stand_in("local_3_7") {
        return;
    }
    let chain = Temp::new("local-3-7-chain");
    let dir = chain.ledger();
    let path = format!("{dir}/00002.json");
    let mut link = fs::read(&path).unwrap();
    link[10] ^= 1;
    fs::write(&path, link).unwrap();
    let mut altered = Check::canary();
    (altered.links, altered.log) = (dir.clone(), dir);
    let output = altered.run();
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    no_pass(&output);
    let answer = Temp::new("local-3-7-answer");
    let record = answer.canary();
    let text = fs::read_to_string(&record).unwrap();
    let swapped = text.replacen("\"label\": 0", "\"label\": 1", 1);
    assert_ne!(text, swapped);
    fs::write(&record, swapped).unwrap();
    let output = Check::of(record, at("evidence/ledger")).run();
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    no_pass(&output);
}

/// A build without `test-trust`, made in its own target directory.
fn release_without_test_trust() -> PathBuf {
    let target = root().join("target/no-test-trust");
    let mut build = Command::new(env::var_os("CARGO").unwrap());
    build
        .current_dir(root())
        .args([
            "build",
            "--release",
            "--locked",
            "-p",
            "hyalos-verify",
            "--target-dir",
        ])
        .arg(&target);
    for (name, _) in env::vars_os() {
        if name
            .to_str()
            .is_some_and(|n| n.starts_with("CARGO") && n != "CARGO_HOME" || n == "RUSTFLAGS")
        {
            build.env_remove(name);
        }
    }
    assert!(build.status().unwrap().success());
    target.join("release/hyalos-verify")
}

/// All five stand-in settings aim at a log key that would refuse every real link. A build that
/// read them would stop at `the reply is not from the log's key`.
#[test]
fn s_03_7_e1_a_release_build_ignores_every_stand_in_setting() {
    let program = release_without_test_trust();
    let one = Check::canary();
    let output = Command::new(program)
        .arg(&one.record)
        .arg(&one.links)
        .args(["--operator-public", &one.operator])
        .args(["--notary-public", &one.notary])
        .args(["--verify", &one.verify])
        .args(["--guest", one.guest.as_ref().unwrap()])
        .env_clear()
        .env("PATH", env::var("PATH").unwrap())
        .env("HYALOS_TRUST_LOG_DIR", "/nonexistent/log")
        .env("HYALOS_TRUST_LOG_KEY", &one.operator)
        .env("HYALOS_TRUST_ROOT", "/nonexistent/root")
        .env("HYALOS_TRUST_SERVER_NAME", "stand-in.test")
        .env("HYALOS_TRUST_PROVIDER_ADDRESS", "127.0.0.1:9")
        .output()
        .unwrap();
    let said = stderr(&output);
    assert!(!said.contains("not from the log's key"), "{said}");
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(
        !report.contains("stand-in") && !report.contains("test trust"),
        "{report}"
    );
    // With the network this is the real log's pass, and without it the check cannot finish.
    assert!(
        output.status.code() == Some(0)
            || (output.status.code() == Some(2) && said.contains("public log")),
        "{:?}: {said}",
        output.status.code()
    );
}

/// The library's own proof, in a build without the feature, that the child gets none of them.
#[test]
fn s_03_7_e2_provider_settings_never_reach_the_child_in_a_release_build() {
    let target = root().join("target/no-test-trust");
    let mut test = Command::new(env::var_os("CARGO").unwrap());
    test.current_dir(root())
        .args([
            "test",
            "--release",
            "--locked",
            "-p",
            "hyalos-audit",
            "--test",
            "no_stand_in",
            "--target-dir",
        ])
        .arg(&target);
    for (name, _) in env::vars_os() {
        if name
            .to_str()
            .is_some_and(|n| n.starts_with("CARGO") && n != "CARGO_HOME" || n == "RUSTFLAGS")
        {
            test.env_remove(name);
        }
    }
    let output = test.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}
