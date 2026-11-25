//! The notary as its own program, against the stand-in provider. No test contacts the real one.

mod common;

use attest::{DEADLINE, MAX_RECV, MAX_SENT, link};
use common::*;
use std::{
    os::unix::fs::PermissionsExt,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

struct Rig {
    scratch: Scratch,
    who: Operator,
    standin: Standin,
    notary: Notary,
}

impl Rig {
    fn new(label: &str, controls: &[&str]) -> Self {
        let scratch = Scratch::new(label);
        let who = operator(&scratch.0.join("home"));
        let standin = standin(&scratch.0, controls);
        let notary = notary(&scratch.0.join("notary"), &who.public, &standin.root);
        Self {
            scratch,
            who,
            standin,
            notary,
        }
    }

    fn command(&self) -> Command {
        tool(
            &self.who,
            &self.standin.root,
            &self.standin.address,
            &self.notary.address,
            &self.notary.transport,
            &self.notary.signing,
            &self.notary.authorisation,
        )
    }

    fn file(&self, name: &str) -> std::path::PathBuf {
        self.scratch.0.join(name)
    }

    fn ask(&self, name: &str, intent: &str, position: u64) -> std::process::Output {
        run_ask(self.command(), &self.file(name), intent, position)
    }
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// Whether the notary's run sequences name the test's run.
fn recorded(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join("sequences"))
        .unwrap()
        .contains(INTENT)
}

fn fake_operator_public(dir: &Path) -> String {
    operator(&dir.join("enrolled")).public
}

#[test]
fn s_01_1_e1_no_key_makes_an_owner_only_key_prints_it_and_listens() {
    let scratch = Scratch::new("s-01-1-e1");
    let operator = fake_operator_public(&scratch.0);
    let root = scratch.0.join("no-root.der");
    std::fs::write(&root, b"").unwrap();
    let dir = scratch.0.join("notary");
    let notary = notary(&dir, &operator, &root);
    assert_eq!(notary.signing.len(), 66, "a compressed public key");
    assert_eq!(mode(&dir.join("signing.key")), 0o600);
    assert_eq!(mode(&dir.join("transport.key")), 0o600);
    assert!(
        std::net::TcpStream::connect(&notary.address).is_ok(),
        "it listens"
    );
}

#[test]
fn s_01_1_e2_a_key_from_an_earlier_start_is_kept() {
    let scratch = Scratch::new("s-01-1-e2");
    let operator = fake_operator_public(&scratch.0);
    let root = scratch.0.join("no-root.der");
    std::fs::write(&root, b"").unwrap();
    let dir = scratch.0.join("notary");
    let first = notary(&dir, &operator, &root);
    let (signing, transport) = (first.signing.clone(), first.transport.clone());
    let bytes = std::fs::read(dir.join("signing.key")).unwrap();
    drop(first);
    let second = notary(&dir, &operator, &root);
    assert_eq!(
        (second.signing.as_str(), second.transport.as_str()),
        (signing.as_str(), transport.as_str())
    );
    assert_eq!(std::fs::read(dir.join("signing.key")).unwrap(), bytes);
}

#[test]
fn s_01_1_e3_a_key_others_can_read_refuses_the_start() {
    let scratch = Scratch::new("s-01-1-e3");
    let operator = fake_operator_public(&scratch.0);
    let root = scratch.0.join("no-root.der");
    std::fs::write(&root, b"").unwrap();
    let dir = scratch.0.join("notary");
    drop(notary(&dir, &operator, &root));
    for (file, loose) in [
        ("signing.key", 0o640),
        ("signing.key", 0o604),
        ("transport.key", 0o660),
    ] {
        let path = dir.join(file);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(loose)).unwrap();
        let refusal = try_notary(&dir, &operator, &root)
            .err()
            .expect("it must refuse to start");
        assert!(refusal.contains("access is too open"), "{refusal}");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

#[test]
fn s_01_2_e1_s_01_5_e1_a_session_is_attested_under_the_printed_key_with_one_line() {
    let _one = one_at_a_time();
    let rig = Rig::new("s-01-2-e1", &[]);
    let said = said(&rig.ask("one.presentation", INTENT, 0));
    assert_eq!(said["sequence"], 0);
    assert!(
        said.get("notary").is_none(),
        "ask no longer prints a notary key"
    );
    let report = verify(
        &rig.who,
        &rig.standin.root,
        &rig.file("one.presentation"),
        &rig.notary.signing,
    )
    .unwrap();
    assert_eq!(
        (report["run"].as_str(), report["sequence"].as_u64()),
        (Some(INTENT), Some(0))
    );
    let line = rig.notary.line_after(0);
    let fields: Vec<&str> = line.split(' ').collect();
    assert_eq!(fields.len(), 7, "{line}");
    assert!(fields[0].parse::<u64>().is_ok(), "{line}");
    assert_eq!(fields[1], NAME);
    assert!(
        fields[2].parse::<u64>().unwrap() > 0 && fields[3].parse::<u64>().unwrap() > 0,
        "{line}"
    );
    assert_eq!(&fields[4..], ["attested", INTENT, "0"]);
    assert_eq!(rig.notary.lines().len(), 1);
}

#[test]
fn s_01_3_e1_a_certificate_for_another_name_is_refused() {
    let _one = one_at_a_time();
    let rig = Rig::new("s-01-3-e1", &["--present", "other.test"]);
    let output = rig.ask("never.presentation", INTENT, 0);
    assert_eq!(word(&output), "provider");
    assert!(!rig.file("never.presentation").exists());
    let line = rig.notary.line_after(0);
    assert!(line.contains(" refused "), "{line}");
    assert!(!line.contains("other.test"), "{line}");
}

#[test]
fn s_01_3_a_session_with_a_server_the_notary_does_not_approve_signs_nothing() {
    // An operator that tells its own tool another name: the session completes, and the notary,
    // which verified the certificate for that name, refuses it. Its number was spent when it opened.
    let _one = one_at_a_time();
    let rig = Rig::new("s-01-3-cheat", &["--present", "other.test"]);
    let mut command = rig.command();
    command.env("HYALOS_TRUST_SERVER_NAME", "other.test");
    let output = run_ask(command, &rig.file("never.presentation"), INTENT, 0);
    assert_eq!(word(&output), "provider");
    assert!(!rig.file("never.presentation").exists());
    let line = rig.notary.line_after(0);
    assert!(
        line.contains(" other.test ") && line.ends_with("refused server"),
        "{line}"
    );
    assert!(
        recorded(&rig.notary.dir),
        "a session spends its number when it opens (ADR-014)"
    );
}

#[test]
fn s_01_3_a_certificate_under_a_root_the_notary_does_not_trust_is_a_provider_refusal() {
    let _one = one_at_a_time();
    let scratch = Scratch::new("s-01-3-root");
    let who = operator(&scratch.0.join("home"));
    let provider = standin(&scratch.0, &[]);
    let other_root = scratch.0.join("no-root.der");
    std::fs::write(&other_root, b"").unwrap();
    let notary = notary(&scratch.0.join("notary"), &who.public, &other_root);
    let file = scratch.0.join("never.presentation");
    let output = run_ask(
        tool(
            &who,
            &provider.root,
            &provider.address,
            &notary.address,
            &notary.transport,
            &notary.signing,
            &notary.authorisation,
        ),
        &file,
        INTENT,
        0,
    );
    assert_eq!(word(&output), "provider");
    assert!(!file.exists());
    let line = notary.line_after(0);
    assert!(line.ends_with("refused server"), "{line}");
    assert!(
        recorded(&notary.dir),
        "a session spends its number when it opens (ADR-014)"
    );
}

#[test]
fn s_01_3_e2_s_01_5_e2_a_dropped_session_signs_nothing_and_the_next_succeeds() {
    let _one = one_at_a_time();
    let dropping = Rig::new("s-01-3-e2", &["--drop"]);
    assert_eq!(
        word(&dropping.ask("never.presentation", INTENT, 0)),
        "provider"
    );
    assert!(!dropping.file("never.presentation").exists());
    let line = dropping.notary.line_after(0);
    assert!(line.ends_with("refused dropped"), "{line}");
    let fields: Vec<&str> = line.split(' ').collect();
    assert_eq!(
        (fields.len(), fields[1], fields[2], fields[3]),
        (6, "none", "0", "0"),
        "no exchange data: {line}"
    );
    assert!(
        !line.contains("build fails") && !line.contains(&"x".repeat(16)),
        "{line}"
    );
}

#[test]
fn s_01_3_e2_a_link_cut_at_any_point_is_never_logged_as_a_refused_server() {
    use std::io::Write;
    let _one = one_at_a_time();
    let rig = Rig::new("s-01-3-cut", &[]);
    for (seen, wait) in (100..=1500).step_by(100).enumerate() {
        let mut command = rig.command();
        let mut child = command
            .arg("ask")
            .arg(rig.file("cut.presentation"))
            .args([INTENT, "0"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(b"A title").unwrap();
        std::thread::sleep(Duration::from_millis(wait));
        let _ = child.kill();
        let _ = child.wait();
        let line = rig.notary.line_after(seen);
        assert!(
            line.ends_with("refused dropped") || line.contains(" attested "),
            "cut after {wait} ms: {line}"
        );
    }
}

#[test]
fn s_01_3_e2_the_notary_serves_the_next_session_after_a_drop() {
    let _one = one_at_a_time();
    let scratch = Scratch::new("s-01-3-e2-next");
    let who = operator(&scratch.0.join("home"));
    let good = standin(&scratch.0, &[]);
    let notary = notary(&scratch.0.join("notary"), &who.public, &good.root);
    let command = || {
        tool(
            &who,
            &good.root,
            &good.address,
            &notary.address,
            &notary.transport,
            &notary.signing,
            &notary.authorisation,
        )
    };
    // A provider that takes the connection and closes it: the session breaks halfway.
    let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = silent.local_addr().unwrap().to_string();
    let closer = std::thread::spawn(move || drop(silent.accept()));
    let broken = run_ask(
        tool(
            &who,
            &good.root,
            &address,
            &notary.address,
            &notary.transport,
            &notary.signing,
            &notary.authorisation,
        ),
        &scratch.0.join("never.presentation"),
        INTENT,
        0,
    );
    closer.join().unwrap();
    assert_eq!(word(&broken), "provider");
    assert!(notary.line_after(0).contains(" refused "));
    let next = run_ask(command(), &scratch.0.join("next.presentation"), INTENT, 0);
    assert_eq!(
        said(&next)["sequence"],
        1,
        "the broken session spent its number (ADR-014)"
    );
    assert!(notary.line_after(1).contains(" attested "));
}

#[test]
fn s_01_4_e1_t_2_3_the_operators_program_has_no_notary_key_and_the_layout_holds() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    for file in [
        "src/lib.rs",
        "src/main.rs",
        "src/bin/notary.rs",
        "src/bin/standin.rs",
    ] {
        assert!(crate_dir.join(file).is_file(), "{file}");
    }
    for file in ["src/main.rs", "src/lib.rs"] {
        let text = std::fs::read_to_string(crate_dir.join(file)).unwrap();
        for banned in [
            "Secp256k1Signer",
            "set_signer",
            "notary.key",
            "notary_key(",
            "signing key",
        ] {
            assert!(!text.contains(banned), "{file} names {banned}");
        }
    }
    let refused = Command::new(env!("CARGO_BIN_EXE_attest"))
        .arg("notary")
        .output()
        .unwrap();
    assert!(
        !refused.status.success(),
        "the operator's tool has no notary command"
    );
}

#[test]
fn s_01_4_e2_with_no_notary_a_call_is_refused_and_a_key_file_there_is_not_opened() {
    let scratch = Scratch::new("s-01-4-e2");
    let who = operator(&scratch.0.join("home"));
    let root = scratch.0.join("root.der");
    std::fs::write(&root, b"").unwrap();
    // A program that opened this pipe for reading would block for ever.
    let pipe = who.home.join(".config/hyalos-spike/notary.key");
    let made = Command::new("mkfifo").arg(&pipe).status().unwrap();
    assert!(made.success());
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = closed.local_addr().unwrap().to_string();
    drop(closed);
    let started = Instant::now();
    let output = run_ask(
        tool(
            &who,
            &root,
            "127.0.0.1:9",
            &address,
            &"00".repeat(91),
            &"00".repeat(33),
            &"ab".repeat(32),
        ),
        &scratch.0.join("never.presentation"),
        INTENT,
        0,
    );
    assert_eq!(word(&output), "unreachable");
    assert!(started.elapsed() < DEADLINE, "refused inside its deadline");
    assert!(!scratch.0.join("never.presentation").exists());
}

#[test]
fn s_03_5_e3_sequence_numbers_survive_a_restart_and_each_run_starts_at_zero() {
    let _one = one_at_a_time();
    let scratch = Scratch::new("s-03-5-e3");
    let who = operator(&scratch.0.join("home"));
    let provider = standin(&scratch.0, &[]);
    let dir = scratch.0.join("notary");
    let ask = |notary: &Notary, name: &str, run: &str, position: u64| {
        let file = scratch.0.join(name);
        let output = run_ask(
            tool(
                &who,
                &provider.root,
                &provider.address,
                &notary.address,
                &notary.transport,
                &notary.signing,
                &notary.authorisation,
            ),
            &file,
            run,
            position,
        );
        let number = said(&output)["sequence"].as_u64().unwrap();
        let report = verify(&who, &provider.root, &file, &notary.signing).unwrap();
        assert_eq!(
            (report["run"].as_str(), report["sequence"].as_u64()),
            (Some(run), Some(number))
        );
        number
    };
    let first = notary(&dir, &who.public, &provider.root);
    assert_eq!(ask(&first, "a.presentation", INTENT, 0), 0);
    assert_eq!(ask(&first, "b.presentation", INTENT, 1), 1);
    drop(first);
    let second = notary(&dir, &who.public, &provider.root);
    assert_eq!(ask(&second, "c.presentation", INTENT, 2), 2);
    assert_eq!(ask(&second, "d.presentation", OTHER_RUN, 0), 0);
}

#[test]
fn s_03_5_unreadable_run_sequences_refuse_the_start() {
    let scratch = Scratch::new("s-03-5-file");
    let operator = fake_operator_public(&scratch.0);
    let root = scratch.0.join("no-root.der");
    std::fs::write(&root, b"").unwrap();
    let dir = scratch.0.join("notary");
    drop(notary(&dir, &operator, &root));
    let header = "hyalos.sequences/1";
    for bad in [
        String::new(),
        "not a sequence\n".to_string(),
        format!("{INTENT} 1\n"),
        format!("{header}\n"),
        format!("{header}\n{INTENT} 1\n"),
        format!("{header}\n{INTENT} 7\nend 1"),
        format!("{header}\n{INTENT} 1\nend 2\n"),
        format!("{header}\n{INTENT} 0\nend 1\n"),
        format!("{header}\n{INTENT} +1\nend 1\n"),
        format!("{header}\n{INTENT} 1\n{INTENT} 2\nend 2\n"),
    ] {
        std::fs::write(dir.join("sequences"), bad).unwrap();
        let refusal = try_notary(&dir, &operator, &root)
            .err()
            .expect("it must refuse to start");
        assert!(refusal.contains("run sequences"), "{refusal}");
    }
}

#[test]
fn s_03_5_a_lost_sequence_file_refuses_the_start_rather_than_reuse_numbers() {
    let scratch = Scratch::new("s-03-5-lost");
    let operator = fake_operator_public(&scratch.0);
    let root = scratch.0.join("no-root.der");
    std::fs::write(&root, b"").unwrap();
    let dir = scratch.0.join("notary");
    drop(notary(&dir, &operator, &root));
    std::fs::remove_file(dir.join("sequences")).unwrap();
    let refusal = try_notary(&dir, &operator, &root)
        .err()
        .expect("it must refuse to start");
    assert!(refusal.contains("run sequences"), "{refusal}");
    assert!(
        !dir.join("sequences").exists(),
        "and it does not make a new one"
    );
}

#[test]
fn s_03_5_a_second_notary_on_one_directory_refuses_to_start() {
    let scratch = Scratch::new("s-03-5-lock");
    let operator = fake_operator_public(&scratch.0);
    let root = scratch.0.join("no-root.der");
    std::fs::write(&root, b"").unwrap();
    let dir = scratch.0.join("notary");
    let first = notary(&dir, &operator, &root);
    let refusal = try_notary(&dir, &operator, &root)
        .err()
        .expect("it must refuse to start");
    assert!(refusal.contains("another notary"), "{refusal}");
    drop(first);
    drop(notary(&dir, &operator, &root));
}

#[test]
fn s_03_5_a_number_that_cannot_be_saved_is_not_signed() {
    let _one = one_at_a_time();
    let rig = Rig::new("s-03-5-save", &[]);
    let dir = &rig.notary.dir;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    let output = rig.ask("never.presentation", INTENT, 0);
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(word(&output), "provider");
    assert!(!rig.file("never.presentation").exists());
    assert!(rig.notary.line_after(0).ends_with("refused sequence"));
    assert_eq!(
        said(&rig.ask("next.presentation", INTENT, 0))["sequence"],
        0,
        "the unsaved number was not used"
    );
}

#[test]
fn adr_014_a_session_left_after_reading_the_reply_spends_its_number() {
    let _one = one_at_a_time();
    let rig = Rig::new("adr-014", &[]);
    let mut leaving = rig.command();
    leaving.env("HYALOS_TEST_LEAVE_AFTER_REPLY", "1");
    assert_eq!(
        word(&run_ask(leaving, &rig.file("left.presentation"), INTENT, 0)),
        "provider"
    );
    assert!(!rig.file("left.presentation").exists());
    assert!(rig.notary.line_after(0).ends_with("refused dropped"));
    assert_eq!(
        said(&rig.ask("next.presentation", INTENT, 0))["sequence"],
        1
    );
}

#[test]
fn t_2_1_an_unenrolled_transport_key_is_closed_before_the_protocol_starts() {
    let _one = one_at_a_time();
    let rig = Rig::new("t-2-1", &[]);
    let stranger = operator(&rig.scratch.0.join("stranger"));
    let command = tool(
        &stranger,
        &rig.standin.root,
        &rig.standin.address,
        &rig.notary.address,
        &rig.notary.transport,
        &rig.notary.signing,
        &rig.notary.authorisation,
    );
    let output = run_ask(command, &rig.file("never.presentation"), INTENT, 0);
    assert_eq!(word(&output), "notary-key");
    assert!(!rig.file("never.presentation").exists());
    let line = rig.notary.line_after(0);
    assert!(line.ends_with("none 0 0 refused unenrolled"), "{line}");
    assert!(!recorded(&rig.notary.dir));
}

#[test]
fn t_2_1_a_notary_with_another_transport_key_is_refused_by_ask() {
    let _one = one_at_a_time();
    let rig = Rig::new("t-2-1-pin", &[]);
    let other = operator(&rig.scratch.0.join("other")).public;
    let command = tool(
        &rig.who,
        &rig.standin.root,
        &rig.standin.address,
        &rig.notary.address,
        &other,
        &rig.notary.signing,
        &rig.notary.authorisation,
    );
    assert_eq!(
        word(&run_ask(
            command,
            &rig.file("never.presentation"),
            INTENT,
            0
        )),
        "notary-key"
    );
    assert!(!rig.file("never.presentation").exists());
}

#[test]
fn t_2_1_an_attestation_under_another_signing_key_is_refused_by_ask() {
    let _one = one_at_a_time();
    let rig = Rig::new("t-2-1-signer", &[]);
    let wrong = format!("02{}", "11".repeat(32));
    let command = tool(
        &rig.who,
        &rig.standin.root,
        &rig.standin.address,
        &rig.notary.address,
        &rig.notary.transport,
        &wrong,
        &rig.notary.authorisation,
    );
    assert_eq!(
        word(&run_ask(
            command,
            &rig.file("never.presentation"),
            INTENT,
            0
        )),
        "notary-key"
    );
    assert!(!rig.file("never.presentation").exists());
}

#[test]
fn t_2_1_idle_strangers_cannot_keep_the_enrolled_operator_out() {
    let _one = one_at_a_time();
    let rig = Rig::new("t-2-1-idle", &[]);
    let idle: Vec<_> = (0..32)
        .map(|_| std::net::TcpStream::connect(&rig.notary.address).unwrap())
        .collect();
    assert_eq!(said(&rig.ask("one.presentation", INTENT, 0))["sequence"], 0);
    assert!(
        !rig.notary
            .lines()
            .iter()
            .any(|line| line.ends_with("refused busy")),
        "no session was open while they waited"
    );
    drop(idle);
}

#[test]
fn t_2_2_the_stand_in_replies_chunked() {
    let _one = one_at_a_time();
    let rig = Rig::new("t-2-2-chunked", &["--reply", "chunked"]);
    assert_eq!(said(&rig.ask("one.presentation", INTENT, 0))["label"], 0);
}

#[test]
fn t_2_2_an_error_status_is_a_provider_refusal_and_spends_the_number() {
    let _one = one_at_a_time();
    let rig = Rig::new("t-2-2-status", &["--status", "500"]);
    assert_eq!(word(&rig.ask("never.presentation", INTENT, 0)), "provider");
    assert!(!rig.file("never.presentation").exists());
    assert!(
        rig.notary
            .line_after(0)
            .contains(&format!("attested {INTENT} 0")),
        "the table's `attested` row"
    );
}

#[test]
fn t_2_2_a_quiet_provider_ends_at_the_deadline_on_both_sides() {
    let _one = one_at_a_time();
    let rig = Rig::new("t-2-2-quiet", &["--quiet"]);
    let started = Instant::now();
    assert_eq!(word(&rig.ask("never.presentation", INTENT, 0)), "deadline");
    assert!(started.elapsed() < DEADLINE + Duration::from_secs(4));
    assert!(rig.notary.line_after(0).ends_with("refused deadline"));
}

/// An enrolled connection, for the tests of the link's own rules.
async fn enrolled(rig: &Rig) -> tokio_rustls::client::TlsStream<tokio::net::TcpStream> {
    let me = link::Identity::load(&rig.who.key).unwrap();
    link::connect(&rig.notary.address, &me, &rig.notary.transport)
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn t_link_another_release_is_answered_release_and_closed() {
    let rig = Rig::new("t-link-release", &[]);
    let mut stream = enrolled(&rig).await;
    link::write_line(&mut stream, &format!("v0.0.0/other {INTENT}"))
        .await
        .unwrap();
    assert_eq!(link::read_line(&mut stream, 16).await.unwrap(), "release");
    assert!(
        link::read_line(&mut stream, 16).await.is_err(),
        "and it closes"
    );
    assert!(rig.notary.line_after(0).ends_with("refused release"));
}

#[tokio::test(flavor = "multi_thread")]
async fn t_link_a_second_connection_during_a_session_is_refused_busy() {
    let rig = Rig::new("t-link-busy", &[]);
    let mut open = enrolled(&rig).await;
    assert_eq!(
        link::open(&mut open, INTENT, &rig.notary.authorisation).await,
        Ok(true)
    );
    let me = link::Identity::load(&rig.who.key).unwrap();
    let second = link::connect(&rig.notary.address, &me, &rig.notary.transport).await;
    if let Ok(mut second) = second {
        assert!(
            link::open(&mut second, INTENT, &rig.notary.authorisation)
                .await
                .is_err(),
            "the second connection is not served"
        );
    }
    assert!(rig.notary.line_after(0).ends_with("none 0 0 refused busy"));
}

#[tokio::test(flavor = "multi_thread")]
async fn t_link_a_session_above_the_size_limits_is_refused() {
    use tlsn::{
        Session,
        config::{prover::ProverConfig, tls_commit::mpc::MpcTlsConfig},
    };
    use tokio_util::compat::TokioAsyncReadCompatExt;
    let rig = Rig::new("t-link-limits", &[]);
    let mut stream = enrolled(&rig).await;
    assert_eq!(
        link::open(&mut stream, INTENT, &rig.notary.authorisation).await,
        Ok(true)
    );
    let session = Session::new(stream.compat());
    let (driver, mut handle) = session.split();
    let driver = tokio::spawn(driver);
    let asked = MpcTlsConfig::builder()
        .max_sent_data(MAX_SENT + 1)
        .max_recv_data(MAX_RECV)
        .build()
        .unwrap();
    let prover = handle
        .new_prover(ProverConfig::builder().build().unwrap())
        .unwrap();
    assert!(prover.commit(asked).await.is_err(), "the notary rejects it");
    driver.abort();
    assert!(rig.notary.line_after(0).ends_with("refused limits"));
}

/// A session that must be refused by the canary, spending nothing.
fn assert_held(rig: &Rig) {
    let seen = rig.notary.lines().len();
    assert_eq!(word(&rig.ask("held.presentation", INTENT, 0)), "canary");
    assert!(!rig.file("held.presentation").exists());
    assert!(rig.notary.line_after(seen).ends_with("refused canary"));
    assert!(!recorded(&rig.notary.dir), "a number was spent");
}

#[test]
fn canary_s_02_1_e1_a_pass_an_hour_old_is_served() {
    let _one = one_at_a_time();
    let rig = Rig::new("canary-2-1-e1", &[]);
    write_canary(&rig.notary.dir, "passed", unix_now() - 3600);
    assert_eq!(said(&rig.ask("one.presentation", INTENT, 0))["label"], 0);
}

#[test]
fn canary_s_02_1_e3_a_pass_past_the_window_is_refused() {
    let _one = one_at_a_time();
    let rig = Rig::new("canary-2-1-e3", &[]);
    write_canary(
        &rig.notary.dir,
        "passed",
        unix_now() - attest::canary::CANARY_WINDOW_SECONDS - 1,
    );
    assert_held(&rig);
}

#[test]
fn canary_s_02_1_e4_a_held_state_is_refused() {
    let _one = one_at_a_time();
    let rig = Rig::new("canary-2-1-e4", &[]);
    write_canary(&rig.notary.dir, "held", unix_now());
    assert_held(&rig);
}

#[test]
fn canary_s_02_1_e5_a_latched_state_is_refused() {
    let _one = one_at_a_time();
    let rig = Rig::new("canary-2-1-e5", &[]);
    write_canary(&rig.notary.dir, "latched", unix_now());
    assert_held(&rig);
}

#[test]
fn canary_s_02_1_e6_a_malformed_or_future_state_is_refused() {
    let _one = one_at_a_time();
    let rig = Rig::new("canary-2-1-e6", &[]);
    write_canary_text(&rig.notary.dir, "hyalos.canary/1\nstate passed\n");
    assert_held(&rig);
    write_canary(&rig.notary.dir, "passed", unix_now() + 3600);
    assert_held(&rig);
}

#[test]
fn canary_s_02_1_e7_a_pass_gone_stale_mid_run_ends_the_run() {
    let _one = one_at_a_time();
    let rig = Rig::new("canary-2-1-e7", &[]);
    assert_eq!(said(&rig.ask("one.presentation", INTENT, 0))["sequence"], 0);
    let seen = rig.notary.lines().len();
    write_canary(
        &rig.notary.dir,
        "passed",
        unix_now() - attest::canary::CANARY_WINDOW_SECONDS - 1,
    );
    assert_eq!(word(&rig.ask("two.presentation", INTENT, 1)), "canary");
    assert!(rig.notary.line_after(seen).ends_with("refused canary"));
    let sequences = std::fs::read_to_string(rig.notary.dir.join("sequences")).unwrap();
    assert!(
        sequences.contains(&format!("{INTENT} 1 ")),
        "the refused session spent a number"
    );
}

#[test]
fn canary_s_02_1_e8_a_pass_stale_at_signing_gets_no_attestation() {
    let _one = one_at_a_time();
    let scratch = Scratch::new("canary-2-1-e8");
    let who = operator(&scratch.0.join("home"));
    let standin = standin(&scratch.0, &[]);
    let later = (attest::canary::CANARY_WINDOW_SECONDS + 1).to_string();
    let notary = forging_notary(
        &scratch.0.join("notary"),
        &who.public,
        &standin.root,
        &[("HYALOS_FORGE_SESSION_LATER", &later)],
    );
    let rig = Rig {
        scratch,
        who,
        standin,
        notary,
    };
    assert!(!rig.ask("never.presentation", INTENT, 0).status.success());
    assert!(!rig.file("never.presentation").exists());
    assert!(rig.notary.line_after(0).ends_with("refused canary"));
    assert!(
        recorded(&rig.notary.dir),
        "the session's number stays spent"
    );
}

#[test]
fn canary_s_02_2_e1_verify_reports_the_pass_time() {
    let _one = one_at_a_time();
    let rig = Rig::new("canary-2-2-e1", &[]);
    let pass = unix_now() - 60;
    write_canary(&rig.notary.dir, "passed", pass);
    said(&rig.ask("one.presentation", INTENT, 0));
    let report = verify(
        &rig.who,
        &rig.standin.root,
        &rig.file("one.presentation"),
        &rig.notary.signing,
    )
    .unwrap();
    assert_eq!(report["canary"]["checked"], pass);
}

#[test]
fn canary_s_02_2_e2_a_requester_canary_is_refused() {
    let _one = one_at_a_time();
    let rig = Rig::new("canary-2-2-e2", &[]);
    let mut command = rig.command();
    command.env("HYALOS_TEST_REQUEST_EXTENSION", "canary");
    assert_eq!(
        word(&run_ask(
            command,
            &rig.file("never.presentation"),
            INTENT,
            0
        )),
        "provider"
    );
    assert!(!rig.file("never.presentation").exists());
    assert!(
        rig.notary.line_after(0).ends_with("refused dropped"),
        "the notary signed nothing"
    );
}

#[test]
fn canary_s_02_3_e1_a_credential_beside_the_notary_is_not_needed() {
    let _one = one_at_a_time();
    let rig = Rig::new("canary-2-3-e1", &[]);
    let beside = rig.notary.dir.join("anthropic.key");
    std::fs::write(&beside, format!("test-{}", "c".repeat(101))).unwrap();
    std::fs::set_permissions(&beside, std::fs::Permissions::from_mode(0o0)).unwrap();
    assert_eq!(said(&rig.ask("one.presentation", INTENT, 0))["label"], 0);
    assert_eq!(mode(&beside), 0);
}

#[test]
fn canary_s_02_3_e2_the_notarys_source_names_no_credential() {
    let source = include_str!("../src/bin/notary.rs");
    for banned in ["pinned_credential", "anthropic.key"] {
        assert!(!source.contains(banned), "the notary names {banned}");
    }
}

#[test]
fn canary_2_2_an_older_tool_hears_release() {
    let _one = one_at_a_time();
    assert_eq!(env!("CARGO_PKG_VERSION"), "0.2.0");
    let rig = Rig::new("canary-2-2-release", &[]);
    let answer = tokio::runtime::Runtime::new().unwrap().block_on(async {
        let mut stream = enrolled(&rig).await;
        link::write_line(
            &mut stream,
            &format!("v0.1.0-beta.1/0.1.0 {INTENT} {}", rig.notary.authorisation),
        )
        .await
        .unwrap();
        link::read_line(&mut stream, 16).await.unwrap()
    });
    assert_eq!(answer, "release");
    assert!(!recorded(&rig.notary.dir));
}
