//! The customer's authorisations and the move of the notary's state to version 2, through the built
//! program. Every key here is made up, in a directory of the test's own.

mod common;

use attest::{RELEASE, link};
use common::{INTENT, Scratch};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const TASK: &str = "1f1d0c7a9b3e4f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8";
const OTHER_TASK: &str = "2f1d0c7a9b3e4f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8";
const RUN: &str = "3f1d0c7a9b3e4f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8";

/// A notary directory whose key is not a usable key, so a command that read it could not pass.
fn directory(scratch: &Scratch) -> PathBuf {
    let dir = scratch.0.join("notary");
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(dir.join("signing.key"), "not a key").unwrap();
    fs::set_permissions(dir.join("signing.key"), fs::Permissions::from_mode(0)).unwrap();
    fs::write(dir.join("sequences"), "hyalos.sequences/2\nend 0\n").unwrap();
    dir
}

fn authorize(dir: &Path, task: &str, attempts: &str, again: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_notary"));
    command
        .args(["authorize", "--dir"])
        .arg(dir)
        .args(["--task", task, "--attempts", attempts]);
    if again {
        command.arg("--again");
    }
    command.output().unwrap()
}

fn kept(dir: &Path, task: &str, attempts: &str, again: bool) -> String {
    let output = authorize(dir, task, attempts, again);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let identifier = String::from_utf8(output.stdout).unwrap().trim().to_string();
    assert_eq!(identifier.len(), 64);
    assert!(
        identifier
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    );
    identifier
}

fn file(dir: &Path, identifier: &str) -> String {
    let path = dir.join("authorisations").join(identifier);
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(dir.join("authorisations"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    fs::read_to_string(path).unwrap()
}

fn expected(task: &str, attempts: u32, round: u32, prior: u32) -> String {
    format!(
        "hyalos.authorisation/1\ntask {task}\nattempts {attempts}\nround {round}\nprior {prior}\nend\n"
    )
}

fn count(dir: &Path) -> usize {
    fs::read_dir(dir.join("authorisations")).map_or(0, |entries| entries.count())
}

fn tickets(dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_notary"))
        .args(["tickets", "--dir"])
        .arg(dir)
        .output()
        .unwrap()
}

fn plant(dir: &Path, identifier: &str, text: &str) {
    let held = dir.join("authorisations");
    fs::create_dir_all(&held).unwrap();
    fs::set_permissions(&held, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(held.join(identifier), text).unwrap();
    fs::set_permissions(held.join(identifier), fs::Permissions::from_mode(0o600)).unwrap();
}

/// Starts a real notary on `dir`, or gives what it said when it refused.
fn start(scratch: &Scratch, dir: &Path) -> Result<common::Notary, String> {
    let home = scratch.0.join("home");
    let who = common::operator(&home);
    let root = scratch.0.join("root.der");
    fs::write(&root, b"").unwrap();
    common::try_notary(dir, &who.public, &root)
}

/// A directory as an earlier notary left it, with no key yet read by this program.
fn old_state(scratch: &Scratch, sequences: &str) -> PathBuf {
    let dir = scratch.0.join("old");
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(dir.join("signing.key"), [7u8; 32]).unwrap();
    fs::set_permissions(dir.join("signing.key"), fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(dir.join("sequences"), sequences).unwrap();
    dir
}

#[test]
fn record_s_03_1_e1_one_attempt_is_kept_at_round_0() {
    let scratch = Scratch::new("r-03-1-e1");
    let dir = directory(&scratch);
    let identifier = kept(&dir, TASK, "1", false);
    assert_eq!(file(&dir, &identifier), expected(TASK, 1, 0, 0));
}

#[test]
fn record_s_03_1_e2_a_hundred_attempts_are_kept() {
    let scratch = Scratch::new("r-03-1-e2");
    let dir = directory(&scratch);
    let identifier = kept(&dir, TASK, "100", false);
    assert_eq!(file(&dir, &identifier), expected(TASK, 100, 0, 0));
}

#[test]
fn record_s_03_2_e1_no_attempts_is_refused_and_nothing_is_kept() {
    let scratch = Scratch::new("r-03-2-e1");
    let dir = directory(&scratch);
    assert!(!authorize(&dir, TASK, "0", false).status.success());
    assert_eq!(count(&dir), 0);
}

#[test]
fn record_s_03_2_e2_too_many_attempts_is_refused_and_nothing_is_kept() {
    let scratch = Scratch::new("r-03-2-e2");
    let dir = directory(&scratch);
    assert!(!authorize(&dir, TASK, "101", false).status.success());
    assert_eq!(count(&dir), 0);
}

#[test]
fn record_s_03_2_e3_an_uppercase_task_is_refused_and_nothing_is_kept() {
    let scratch = Scratch::new("r-03-2-e3");
    let dir = directory(&scratch);
    assert!(
        !authorize(&dir, &TASK.to_uppercase(), "1", false)
            .status
            .success()
    );
    assert_eq!(count(&dir), 0);
}

#[test]
fn record_s_03_3_e1_a_task_already_authorised_needs_again() {
    let scratch = Scratch::new("r-03-3-e1");
    let dir = directory(&scratch);
    kept(&dir, TASK, "1", false);
    assert!(!authorize(&dir, TASK, "1", false).status.success());
    assert_eq!(count(&dir), 1);
}

#[test]
fn record_s_03_3_e2_another_task_starts_at_round_0() {
    let scratch = Scratch::new("r-03-3-e2");
    let dir = directory(&scratch);
    kept(&dir, OTHER_TASK, "4", false);
    let identifier = kept(&dir, TASK, "1", false);
    assert_eq!(file(&dir, &identifier), expected(TASK, 1, 0, 0));
}

#[test]
fn record_s_03_4_e1_again_makes_round_1() {
    let scratch = Scratch::new("r-03-4-e1");
    let dir = directory(&scratch);
    kept(&dir, TASK, "2", false);
    let identifier = kept(&dir, TASK, "1", true);
    assert_eq!(file(&dir, &identifier), expected(TASK, 1, 1, 2));
}

#[test]
fn record_s_03_4_e2_again_after_two_rounds_makes_round_2() {
    let scratch = Scratch::new("r-03-4-e2");
    let dir = directory(&scratch);
    kept(&dir, TASK, "2", false);
    kept(&dir, TASK, "2", true);
    let identifier = kept(&dir, TASK, "1", true);
    assert_eq!(file(&dir, &identifier), expected(TASK, 1, 2, 4));
}

#[test]
fn record_s_03_6_e1_a_first_round_has_a_prior_total_of_0() {
    let scratch = Scratch::new("r-03-6-e1");
    let dir = directory(&scratch);
    let identifier = kept(&dir, TASK, "3", false);
    assert_eq!(file(&dir, &identifier), expected(TASK, 3, 0, 0));
}

#[test]
fn record_s_03_6_e2_budgets_of_3_and_5_give_a_prior_total_of_8() {
    let scratch = Scratch::new("r-03-6-e2");
    let dir = directory(&scratch);
    kept(&dir, TASK, "3", false);
    kept(&dir, TASK, "5", true);
    let identifier = kept(&dir, TASK, "1", true);
    assert_eq!(file(&dir, &identifier), expected(TASK, 1, 2, 8));
}

#[test]
fn record_s_03_7_e1_one_ticket_of_three_shows_as_spent() {
    let scratch = Scratch::new("r-03-7-e1");
    let dir = directory(&scratch);
    let identifier = kept(&dir, TASK, "3", false);
    fs::write(
        dir.join("sequences"),
        format!("hyalos.sequences/2\n{RUN} 1 {identifier} 0\nend 1\n"),
    )
    .unwrap();
    let output = tickets(&dir);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{identifier} task {TASK} round 0 budget 3 prior 0 spent 1 of 3\n")
    );
}

#[test]
fn record_s_03_7_e2_an_unreadable_authorisation_prints_no_partial_total() {
    let scratch = Scratch::new("r-03-7-e2");
    let dir = directory(&scratch);
    kept(&dir, TASK, "3", false);
    plant(&dir, &"a".repeat(64), "hyalos.authorisation/1\ntask");
    let output = tickets(&dir);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "no partial total");
}

#[test]
fn record_s_04_7_e1_a_version_1_file_is_rewritten_keeping_each_number() {
    let scratch = Scratch::new("r-04-7-e1");
    let dir = old_state(&scratch, &format!("hyalos.sequences/1\n{RUN} 5\nend 1\n"));
    let _notary = start(&scratch, &dir).unwrap();
    assert_eq!(
        fs::read_to_string(dir.join("sequences")).unwrap(),
        format!("hyalos.sequences/2\n{RUN} 5 - -\nend 1\n")
    );
}

#[test]
fn record_s_04_7_e3_a_version_1_file_cut_short_refuses_the_start() {
    let scratch = Scratch::new("r-04-7-e3");
    let cut = format!("hyalos.sequences/1\n{RUN} 5\n");
    let dir = old_state(&scratch, &cut);
    let refusal = start(&scratch, &dir).err().expect("the notary must refuse");
    assert!(refusal.contains("run sequences"), "{refusal}");
    assert_eq!(
        fs::read_to_string(dir.join("sequences")).unwrap(),
        cut,
        "the file is left as it was"
    );
}

#[test]
fn record_s_04_10_e1_a_version_1_file_and_an_authorisation_refuse_the_start() {
    let scratch = Scratch::new("r-04-10-e1");
    let old = format!("hyalos.sequences/1\n{RUN} 5\nend 1\n");
    let dir = old_state(&scratch, &old);
    plant(&dir, &"a".repeat(64), &expected(TASK, 1, 0, 0));
    let refusal = start(&scratch, &dir).err().expect("the notary must refuse");
    assert!(refusal.contains("version 1"), "{refusal}");
    assert_eq!(
        fs::read_to_string(dir.join("sequences")).unwrap(),
        old,
        "the file is left as it was"
    );
}

#[test]
fn record_s_04_10_e2_authorize_is_refused_on_a_version_1_file() {
    let scratch = Scratch::new("r-04-10-e2");
    let dir = old_state(&scratch, &format!("hyalos.sequences/1\n{RUN} 5\nend 1\n"));
    let output = authorize(&dir, TASK, "1", false);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("version 1"));
    assert_eq!(count(&dir), 0, "nothing is kept");
}

#[test]
fn record_s_04_10_e3_a_version_2_file_and_authorisations_start() {
    let scratch = Scratch::new("r-04-10-e3");
    let dir = old_state(&scratch, "hyalos.sequences/2\nend 0\n");
    plant(&dir, &"a".repeat(64), &expected(TASK, 1, 0, 0));
    assert!(start(&scratch, &dir).is_ok());
}

#[test]
fn record_3_1_two_authorisations_at_once_take_different_rounds() {
    let scratch = Scratch::new("r-3-1");
    let dir = directory(&scratch);
    kept(&dir, TASK, "1", false);
    let spawn = || {
        let dir = dir.clone();
        std::thread::spawn(move || kept(&dir, TASK, "1", true))
    };
    let (one, two) = (spawn(), spawn());
    let (one, two) = (one.join().unwrap(), two.join().unwrap());
    let round = |identifier: &str| {
        file(&dir, identifier)
            .lines()
            .find_map(|line| line.strip_prefix("round ").map(str::to_string))
            .unwrap()
    };
    let mut rounds = [round(&one), round(&two)];
    rounds.sort();
    assert_eq!(rounds, ["1".to_string(), "2".to_string()]);
}

#[test]
fn record_s_03_7_tickets_reads_a_directory_the_notary_is_serving() {
    let scratch = Scratch::new("r-03-7-serving");
    let dir = old_state(&scratch, "hyalos.sequences/2\nend 0\n");
    let identifier = "a".repeat(64);
    plant(&dir, &identifier, &expected(TASK, 3, 0, 0));
    let _serving = start(&scratch, &dir).unwrap();
    let output = tickets(&dir);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{identifier} task {TASK} round 0 budget 3 prior 0 spent 0 of 3\n")
    );
}

/// A serving notary with no authorisation of its own, and the operator enrolled on it.
struct Serving {
    scratch: Scratch,
    who: common::Operator,
    root: PathBuf,
    dir: PathBuf,
    notary: Option<common::Notary>,
}

impl Serving {
    fn new(label: &str) -> Self {
        let scratch = Scratch::new(label);
        let who = common::operator(&scratch.0.join("home"));
        let root = scratch.0.join("root.der");
        fs::write(&root, b"").unwrap();
        let dir = scratch.0.join("notary");
        let notary = Some(common::try_notary(&dir, &who.public, &root).unwrap());
        Self {
            scratch,
            who,
            root,
            dir,
            notary,
        }
    }

    fn restart(&mut self) {
        drop(self.notary.take());
        self.notary = Some(common::try_notary(&self.dir, &self.who.public, &self.root).unwrap());
    }

    /// The one word the notary answers a session's first line with. The link is then dropped.
    async fn word(&self, run: &str, authorisation: &str) -> String {
        let notary = self.notary.as_ref().unwrap();
        let seen = notary.lines().len();
        let me = link::Identity::load(&self.who.key).unwrap();
        let mut stream = link::connect(&notary.address, &me, &notary.transport)
            .await
            .unwrap();
        link::write_line(&mut stream, &format!("{RELEASE} {run} {authorisation}"))
            .await
            .unwrap();
        let said = link::read_line(&mut stream, 16).await.unwrap();
        drop(stream);
        notary.line_after(seen);
        said
    }

    /// What the state file holds for a run: its count, authorisation and ticket.
    fn held(&self, run: &str) -> Option<(u64, String, String)> {
        let text = fs::read_to_string(self.dir.join("sequences")).unwrap();
        text.lines().find_map(|line| {
            let fields: Vec<&str> = line.split(' ').collect();
            (fields.len() == 4 && fields[0] == run).then(|| {
                (
                    fields[1].parse().unwrap(),
                    fields[2].to_string(),
                    fields[3].to_string(),
                )
            })
        })
    }
}

fn run(number: u64) -> String {
    format!("{number:064x}")
}

#[tokio::test(flavor = "multi_thread")]
async fn record_s_03_5_e1_an_authorisation_made_while_serving_is_used_at_once() {
    let serving = Serving::new("r-03-5-e1");
    let identifier = kept(&serving.dir, TASK, "1", false);
    assert_eq!(serving.word(&run(1), &identifier).await, "ok");
}

#[tokio::test(flavor = "multi_thread")]
async fn record_s_03_5_e2_a_half_written_authorisation_opens_no_session() {
    let serving = Serving::new("r-03-5-e2");
    let identifier = "a".repeat(64);
    plant(&serving.dir, &identifier, "hyalos.authorisation/1\ntask");
    assert_eq!(serving.word(&run(1), &identifier).await, "authorisation");
    assert_eq!(serving.held(&run(1)), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn record_s_04_1_e1_a_new_run_takes_ticket_0() {
    let serving = Serving::new("r-04-1-e1");
    let identifier = kept(&serving.dir, TASK, "2", false);
    assert_eq!(serving.word(&run(1), &identifier).await, "ok");
    assert_eq!(
        serving.held(&run(1)),
        Some((1, identifier, "0".to_string()))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn record_s_04_1_e2_another_new_run_takes_ticket_1() {
    let serving = Serving::new("r-04-1-e2");
    let identifier = kept(&serving.dir, TASK, "2", false);
    serving.word(&run(1), &identifier).await;
    assert_eq!(serving.word(&run(2), &identifier).await, "ok");
    assert_eq!(
        serving.held(&run(2)),
        Some((1, identifier, "1".to_string()))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn record_s_04_2_e1_a_run_holding_a_ticket_takes_its_next_number() {
    let serving = Serving::new("r-04-2-e1");
    let identifier = kept(&serving.dir, TASK, "2", false);
    serving.word(&run(1), &identifier).await;
    assert_eq!(serving.word(&run(1), &identifier).await, "ok");
    assert_eq!(
        serving.held(&run(1)),
        Some((2, identifier, "0".to_string()))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn record_s_04_2_e2_a_known_run_needs_no_new_ticket_at_the_budget() {
    let serving = Serving::new("r-04-2-e2");
    let identifier = kept(&serving.dir, TASK, "1", false);
    serving.word(&run(1), &identifier).await;
    assert_eq!(serving.word(&run(1), &identifier).await, "ok");
    assert_eq!(
        serving.held(&run(1)),
        Some((2, identifier, "0".to_string()))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn record_s_04_3_e1_a_new_run_past_the_budget_hears_spent() {
    let serving = Serving::new("r-04-3-e1");
    let identifier = kept(&serving.dir, TASK, "1", false);
    serving.word(&run(1), &identifier).await;
    assert_eq!(serving.word(&run(2), &identifier).await, "spent");
    assert_eq!(serving.held(&run(2)), None, "no sequence number is used");
}

#[tokio::test(flavor = "multi_thread")]
async fn record_s_04_3_e2_a_run_under_another_log_key_is_counted_the_same() {
    // The notary never sees a log key, so a run under a second key is one more new run to it.
    let serving = Serving::new("r-04-3-e2");
    let identifier = kept(&serving.dir, TASK, "2", false);
    serving.word(&run(1), &identifier).await;
    serving.word(&run(2), &identifier).await;
    assert_eq!(serving.word(&run(3), &identifier).await, "spent");
}

#[tokio::test(flavor = "multi_thread")]
async fn record_s_04_4_e1_an_identifier_never_issued_is_refused() {
    let serving = Serving::new("r-04-4-e1");
    kept(&serving.dir, TASK, "1", false);
    assert_eq!(
        serving.word(&run(1), &"c".repeat(64)).await,
        "authorisation"
    );
    assert_eq!(serving.held(&run(1)), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn record_s_04_4_e2_a_malformed_identifier_is_refused() {
    let serving = Serving::new("r-04-4-e2");
    let identifier = kept(&serving.dir, TASK, "1", false);
    assert_eq!(
        serving.word(&run(1), &identifier.to_uppercase()).await,
        "authorisation"
    );
    assert_eq!(serving.word(&run(1), "").await, "authorisation");
    assert_eq!(serving.held(&run(1)), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn record_s_04_5_e1_a_run_naming_another_authorisation_is_refused() {
    let serving = Serving::new("r-04-5-e1");
    let first = kept(&serving.dir, TASK, "2", false);
    let second = kept(&serving.dir, OTHER_TASK, "2", false);
    serving.word(&run(1), &first).await;
    assert_eq!(serving.word(&run(1), &second).await, "authorisation");
    assert_eq!(serving.held(&run(1)), Some((1, first, "0".to_string())));
    assert_eq!(serving.word(&run(2), &second).await, "ok");
    assert_eq!(
        serving.held(&run(2)),
        Some((1, second, "0".to_string())),
        "the other's first ticket was still free"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn record_s_04_5_e2_a_run_naming_its_own_authorisation_is_served() {
    let serving = Serving::new("r-04-5-e2");
    let identifier = kept(&serving.dir, TASK, "2", false);
    serving.word(&run(1), &identifier).await;
    assert_eq!(serving.word(&run(1), &identifier).await, "ok");
}

#[tokio::test(flavor = "multi_thread")]
async fn record_s_04_6_e1_a_session_left_after_its_reply_does_not_return_the_ticket() {
    let _one = common::one_at_a_time();
    let scratch = Scratch::new("r-04-6-e1");
    let who = common::operator(&scratch.0.join("home"));
    let standin = common::standin(&scratch.0, &[]);
    let dir = scratch.0.join("notary");
    let notary = common::try_notary(&dir, &who.public, &standin.root).unwrap();
    let identifier = kept(&dir, TASK, "1", false);
    let mut leaving = common::tool(
        &who,
        &standin.root,
        &standin.address,
        &notary.address,
        &notary.transport,
        &notary.signing,
        &identifier,
    );
    leaving.env("HYALOS_TEST_LEAVE_AFTER_REPLY", "1");
    let seen = notary.lines().len();
    assert!(
        !common::run_ask(leaving, &scratch.0.join("left.presentation"), INTENT, 0)
            .status
            .success()
    );
    notary.line_after(seen);
    let other = common::tool(
        &who,
        &standin.root,
        &standin.address,
        &notary.address,
        &notary.transport,
        &notary.signing,
        &identifier,
    );
    let said = common::run_ask(other, &scratch.0.join("again.presentation"), &run(9), 0);
    assert_eq!(common::word(&said), "spent");
}

#[tokio::test(flavor = "multi_thread")]
async fn record_s_04_6_e2_a_restart_keeps_the_tickets() {
    let mut serving = Serving::new("r-04-6-e2");
    let identifier = kept(&serving.dir, TASK, "2", false);
    serving.word(&run(1), &identifier).await;
    serving.restart();
    assert_eq!(serving.word(&run(2), &identifier).await, "ok");
    assert_eq!(
        serving.held(&run(2)),
        Some((1, identifier, "1".to_string()))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn record_s_04_6_e3_a_save_that_fails_spends_nothing() {
    let serving = Serving::new("r-04-6-e3");
    let identifier = kept(&serving.dir, TASK, "1", false);
    let blocked = serving.dir.join("sequences.new");
    fs::create_dir(&blocked).unwrap();
    assert_eq!(serving.word(&run(1), &identifier).await, "sequence");
    assert_eq!(serving.held(&run(1)), None);
    fs::remove_dir(&blocked).unwrap();
    assert_eq!(
        serving.word(&run(1), &identifier).await,
        "ok",
        "the ticket was not spent"
    );
    assert_eq!(
        serving.held(&run(1)),
        Some((1, identifier, "0".to_string()))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn record_s_04_7_e2_a_run_from_old_state_is_refused() {
    let scratch = Scratch::new("r-04-7-e2");
    let who = common::operator(&scratch.0.join("home"));
    let root = scratch.0.join("root.der");
    fs::write(&root, b"").unwrap();
    let dir = old_state(&scratch, &format!("hyalos.sequences/1\n{RUN} 5\nend 1\n"));
    let notary = common::try_notary(&dir, &who.public, &root).unwrap();
    let identifier = kept(&dir, TASK, "3", false);
    let serving = Serving {
        scratch,
        who,
        root,
        dir,
        notary: Some(notary),
    };
    assert_eq!(serving.word(RUN, &identifier).await, "authorisation");
    assert_eq!(
        fs::read_to_string(serving.dir.join("sequences")).unwrap(),
        format!("hyalos.sequences/2\n{RUN} 5 - -\nend 1\n"),
        "its number is unchanged"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn record_s_04_9_e1_a_new_run_on_an_older_round_is_refused() {
    let serving = Serving::new("r-04-9-e1");
    let older = kept(&serving.dir, TASK, "2", false);
    kept(&serving.dir, TASK, "2", true);
    assert_eq!(serving.word(&run(1), &older).await, "authorisation");
    assert_eq!(serving.held(&run(1)), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn record_s_04_9_e2_a_new_run_on_the_newest_round_is_served() {
    let serving = Serving::new("r-04-9-e2");
    kept(&serving.dir, TASK, "2", false);
    let newest = kept(&serving.dir, TASK, "2", true);
    assert_eq!(serving.word(&run(1), &newest).await, "ok");
}

#[tokio::test(flavor = "multi_thread")]
async fn record_s_04_9_e3_a_run_already_on_an_older_round_is_served_with_no_new_ticket() {
    let serving = Serving::new("r-04-9-e3");
    let older = kept(&serving.dir, TASK, "2", false);
    serving.word(&run(1), &older).await;
    kept(&serving.dir, TASK, "2", true);
    assert_eq!(serving.word(&run(1), &older).await, "ok");
    assert_eq!(serving.held(&run(1)), Some((2, older, "0".to_string())));
}

#[test]
fn record_4_1_both_extensions_are_read() {
    let _one = common::one_at_a_time();
    let scratch = Scratch::new("r-4-1");
    let who = common::operator(&scratch.0.join("home"));
    let standin = common::standin(&scratch.0, &[]);
    let notary = common::notary(&scratch.0.join("notary"), &who.public, &standin.root);
    let file = scratch.0.join("one.presentation");
    let command = common::tool(
        &who,
        &standin.root,
        &standin.address,
        &notary.address,
        &notary.transport,
        &notary.signing,
        &notary.authorisation,
    );
    common::said(&common::run_ask(command, &file, INTENT, 0));
    let seen = common::verify(&who, &standin.root, &file, &notary.signing).unwrap();
    assert_eq!(
        (seen["run"].as_str(), seen["sequence"].as_u64()),
        (Some(INTENT), Some(0))
    );
    let ticket = &seen["ticket"];
    assert_eq!(ticket["authorisation"], notary.authorisation.as_str());
    assert_eq!(ticket["task"], common::TASK);
    assert_eq!(
        (
            ticket["budget"].as_u64(),
            ticket["round"].as_u64(),
            ticket["prior"].as_u64(),
            ticket["ticket"].as_u64()
        ),
        (Some(100), Some(0), Some(0), Some(0))
    );
}

#[test]
fn record_4_2_a_requester_extension_is_refused() {
    let _one = common::one_at_a_time();
    let scratch = Scratch::new("r-4-2");
    let who = common::operator(&scratch.0.join("home"));
    let standin = common::standin(&scratch.0, &[]);
    let notary = common::notary(&scratch.0.join("notary"), &who.public, &standin.root);
    let file = scratch.0.join("never.presentation");
    let mut command = common::tool(
        &who,
        &standin.root,
        &standin.address,
        &notary.address,
        &notary.transport,
        &notary.signing,
        &notary.authorisation,
    );
    command.env("HYALOS_TEST_REQUEST_EXTENSION", "1");
    let output = common::run_ask(command, &file, INTENT, 0);
    assert_eq!(common::word(&output), "provider");
    assert!(!file.exists(), "no presentation was written");
    assert!(
        notary.line_after(0).ends_with("refused dropped"),
        "the notary signed nothing"
    );
}
