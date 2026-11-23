//! Holding each model answer to what the trusted notary saw.

use crate::failure::Failure;
use crate::files::{FileError, FileKind, Files};
use crate::record::*;
use std::path::Path;

macro_rules! refused {
    ($($arg:tt)*) => { Failure::refused(format!($($arg)*)) };
}

/// The settings that point verify at a stand-in. They are the only part of this process's
/// environment it is given, so it reads no home directory and no credential.
#[cfg(feature = "test-trust")]
const TRUST_SETTINGS: [&str; 3] = [
    "HYALOS_TRUST_SERVER_NAME",
    "HYALOS_TRUST_ROOT",
    "HYALOS_TRUST_PROVIDER_ADDRESS",
];

/// The verify program the auditor built, and the trust settings it runs under.
pub struct Verifier {
    pub program: String,
    pub trust: Vec<(String, std::ffi::OsString)>,
}

impl Verifier {
    /// Without `test-trust` this reads no setting, so the child runs with none of them.
    pub fn from_env(program: &str) -> Self {
        #[cfg(feature = "test-trust")]
        let trust = TRUST_SETTINGS
            .iter()
            .filter_map(|name| Some((name.to_string(), std::env::var_os(name)?)))
            .collect();
        #[cfg(not(feature = "test-trust"))]
        let trust = Vec::new();
        Self {
            program: program.to_string(),
            trust,
        }
    }
}

/// Holds one answer to what its attestation says. `window` is the run's two signed times, when a
/// chain was checked. Each refusal names its own check.
pub fn check_answer(
    seen: &serde_json::Value,
    answer: &Answer,
    index: usize,
    run: &str,
    window: Option<(u64, u64)>,
) -> Result<(), String> {
    let fail = |what: &str| Err(format!("answer {index}: {what}"));
    let position = index as u64;
    if seen["intent_hash"] != run {
        return fail("the binding names another run");
    }
    if seen["position"].as_u64() != Some(position) {
        return fail("the binding names another position");
    }
    if seen["run"] != run {
        return fail("the notary was told another run");
    }
    if seen["sequence"].as_u64() != Some(position) {
        return fail(
            "the sequence number is not the answer's position, so an attestation is unaccounted for",
        );
    }
    if let Some((intent_time, result_time)) = window {
        let Some(time) = seen["time"].as_u64() else {
            return fail("the session has no time");
        };
        // Whole seconds cannot order a session inside the intent's own second, so it is refused (ADR-008).
        if time <= intent_time {
            return fail("the session is not later than the run's intent");
        }
        if time > result_time {
            return fail("the session is later than the run's result");
        }
    }
    let same = seen["question_sha256"] == answer.question_sha256.as_str()
        && seen["label"] == answer.label
        && seen["model"] == answer.model.as_str()
        && seen["response_id"] == answer.response_id.as_str();
    if !same {
        return fail("the notary saw a different question or answer than the record claims");
    }
    Ok(())
}

/// The attempt the notary signed for a run: one exact value, carried by every answer.
#[derive(Clone, PartialEq, Eq)]
pub struct Attempt {
    pub authorisation: String,
    pub task: String,
    pub budget: u64,
    pub round: u64,
    pub prior: u64,
    pub ticket: u64,
}

impl Attempt {
    fn read(seen: &serde_json::Value) -> Option<Self> {
        let text = |key: &str| seen[key].as_str().map(str::to_string);
        let number = |key: &str| seen[key].as_u64();
        Some(Self {
            authorisation: text("authorisation")?,
            task: text("task")?,
            budget: number("budget")?,
            round: number("round")?,
            prior: number("prior")?,
            ticket: number("ticket")?,
        })
    }
}

/// What the per-answer checks found.
pub struct Witnessed {
    pub lines: Vec<String>,
    pub test_settings: bool,
    /// The run's one attempt. `None` only for a version 1 record.
    pub attempt: Option<Attempt>,
    /// The oldest signed pass. `None` for a record older than version 3.
    pub oldest_pass: Option<u64>,
}

/// Checks that every model answer in a record is the one the trusted notary saw the provider's
/// server give, for this run and position. It needs no network and no credential. With `chain`,
/// the run and its window come from the verified log. Without it, every answer must name one run.
pub fn witnessed(
    t: &Transcript,
    record: &str,
    notary: &str,
    verifier: &Verifier,
    chain: Option<(&str, (u64, u64))>,
    files: &dyn Files,
) -> Result<Witnessed, Failure> {
    if t.answers.is_empty() {
        return Err("this record has no attested answers".into());
    }
    let mut run = chain.map(|(intent, _)| intent.to_string());
    let (mut lines, mut test_settings, mut attempt, mut oldest_pass) =
        (Vec::new(), false, None::<Attempt>, None::<u64>);
    for (index, answer) in t.answers.iter().enumerate() {
        let file = format!("{record}.attest/{index:03}.presentation");
        let bytes = files
            .read(Path::new(&file), FileKind::Presentation)
            .map_err(|error| match error {
                FileError::Missing => Failure::refused(format!("answer {index}: no attestation")),
                FileError::TooLarge => Failure::refused(format!(
                    "answer {index}: the attestation is over its size limit"
                )),
                FileError::Unreadable => {
                    Failure::inconclusive(format!("answer {index}: the attestation cannot be read"))
                }
            })?;
        if answer.attestation.is_empty() || sha(&bytes) != answer.attestation {
            return Err(refused!(
                "answer {index}: the attestation is not the one the record names"
            ));
        }
        let output = std::process::Command::new(&verifier.program)
            .args(["verify", &file, notary])
            .env_clear()
            .envs(verifier.trust.iter().map(|(name, value)| (name, value)))
            .output()
            .map_err(|_| Failure::inconclusive("cannot run the verify program"))?;
        if !output.status.success() {
            // Only compared, never shown: the program's text stays out of the report.
            let untrusted = String::from_utf8_lossy(&output.stderr).contains("not the trusted one");
            return Err(refused!(
                "answer {index}: {}",
                if untrusted {
                    "not attested by the trusted notary"
                } else {
                    "the attestation does not verify"
                }
            ));
        }
        let seen: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|_| Failure::inconclusive("cannot read the verdict"))?;
        let named =
            run.get_or_insert_with(|| seen["intent_hash"].as_str().unwrap_or_default().to_string());
        check_answer(&seen, answer, index, named, chain.map(|(_, window)| window))?;
        // A ticketed attestation in a version 1 record would hide its budget (ADR-011).
        if t.version == 1 && !seen["ticket"].is_null() {
            return Err(refused!(
                "answer {index}: a version 1 record cannot carry a ticket"
            ));
        }
        // Every record but a version 1 record must carry the one signed attempt.
        if t.version != 1 {
            let held = Attempt::read(&seen["ticket"])
                .ok_or_else(|| refused!("answer {index}: the attestation carries no ticket"))?;
            if !(1..=100).contains(&held.budget) || held.ticket >= held.budget {
                return Err(refused!(
                    "answer {index}: the ticket is not inside its budget"
                ));
            }
            if attempt.as_ref().is_some_and(|first| *first != held) {
                return Err(refused!(
                    "answer {index}: the ticket is not the one answer 0 carries"
                ));
            }
            attempt = Some(held);
        }
        // Optional on an older version, a canary could be removed unseen, so only version 3 and later have one.
        if t.version < 3 {
            if !seen["canary"].is_null() {
                return Err(refused!(
                    "answer {index}: a version {} record cannot carry a canary",
                    t.version
                ));
            }
        } else {
            let pass = seen["canary"]["checked"]
                .as_u64()
                .ok_or_else(|| refused!("answer {index}: the attestation carries no canary"))?;
            let session = seen["time"]
                .as_u64()
                .ok_or_else(|| refused!("answer {index}: the attestation has no session time"))?;
            if pass > session || session - pass > CANARY_WINDOW_SECONDS {
                return Err(refused!(
                    "answer {index}: the canary is not fresh at its session"
                ));
            }
            oldest_pass = Some(oldest_pass.map_or(pass, |oldest| oldest.min(pass)));
        }
        test_settings |= !seen["trust"].is_null();
        lines.push(format!(
            "answer {index}: run {named} position {index} sequence {index} session {}",
            seen["time"]
        ));
    }
    Ok(Witnessed {
        lines,
        test_settings,
        attempt,
        oldest_pass,
    })
}
