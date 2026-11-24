//! Holding each model answer to what the trusted notary saw.

use crate::failure::Failure;
use crate::files::{FileError, FileKind, Files};
use crate::record::*;
use attest_check::{Refusal, Trust};
use std::path::Path;

macro_rules! refused {
    ($($arg:tt)*) => { Failure::refused(format!($($arg)*)) };
}

/// The settings that point the check at a stand-in provider, in a test build only.
#[cfg(feature = "test-trust")]
const TRUST_SETTINGS: [&str; 3] = [
    "HYALOS_TRUST_SERVER_NAME",
    "HYALOS_TRUST_ROOT",
    "HYALOS_TRUST_PROVIDER_ADDRESS",
];

/// Whom the presentation check trusts to be the provider.
pub struct Verifier {
    pub trust: Trust,
}

impl Verifier {
    /// The real provider's trust.
    pub fn real() -> Self {
        Self {
            trust: Trust::real(),
        }
    }

    /// The trust this process is configured for. Without `test-trust` this reads no setting.
    #[cfg(not(feature = "test-trust"))]
    pub fn from_env() -> Result<Self, Failure> {
        Ok(Self::real())
    }

    /// The trust this process is configured for, from its stand-in settings.
    #[cfg(feature = "test-trust")]
    pub fn from_env() -> Result<Self, Failure> {
        let settings: Vec<(String, std::ffi::OsString)> = TRUST_SETTINGS
            .iter()
            .filter_map(|name| Some((name.to_string(), std::env::var_os(name)?)))
            .collect();
        Self::from_settings(&settings)
    }

    /// A stand-in's trust. A setting that is present but unusable never falls back to the real one.
    #[cfg(feature = "test-trust")]
    pub fn from_settings(settings: &[(String, std::ffi::OsString)]) -> Result<Self, Failure> {
        let set = |name: &str| -> Result<Option<String>, Failure> {
            match settings.iter().find(|(held, _)| held == name) {
                None => Ok(None),
                Some((_, value)) => value
                    .to_str()
                    .map(|text| Some(text.to_string()).filter(|text| !text.is_empty()))
                    .ok_or_else(|| Failure::inconclusive("a trust setting is not text")),
            }
        };
        let (server, root, address) = (
            set("HYALOS_TRUST_SERVER_NAME")?,
            set("HYALOS_TRUST_ROOT")?,
            set("HYALOS_TRUST_PROVIDER_ADDRESS")?,
        );
        let test = server.is_some() || root.is_some() || address.is_some();
        let mut trust = Trust::real();
        trust.test = test;
        if let Some(server) = server {
            trust.server = server;
        }
        if let Some(file) = root {
            let der = std::fs::read(file)
                .map_err(|_| Failure::inconclusive("cannot read the test root"))?;
            trust.extra_roots.push(der);
        }
        Ok(Self { trust })
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
        let seen = attest_check::check(&bytes, notary, &verifier.trust).map_err(|refusal| {
            refused!(
                "answer {index}: {}",
                match refusal {
                    Refusal::UntrustedNotary => "not attested by the trusted notary",
                    Refusal::Invalid => "the attestation does not verify",
                }
            )
        })?;
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
