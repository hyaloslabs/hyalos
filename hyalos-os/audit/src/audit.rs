//! The whole audit of one record, and its verdict.

use crate::answers::{Verifier, witnessed};
use crate::chain::{Auditor, Claim, LogSource};
use crate::failure::{Failure, FailureKind};
use crate::files::{FileError, FileKind, Files};
use crate::record::*;
use crate::replay::replay;
use crate::report::audit_report;
use std::path::Path;

/// The outcome of a check. Only a completed check is `Passed`, and only it holds a report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Passed(String),
    Refused(String),
    Inconclusive(String),
}

/// Audits a record and gives its verdict.
pub fn check(
    record: &str,
    inputs: &AuditInputs,
    log: LogSource,
    verifier: &Verifier,
    files: &'static dyn Files,
) -> Verdict {
    match audit_record(record, inputs, log, verifier, files) {
        Ok(report) => Verdict::Passed(report),
        Err(Failure {
            kind: FailureKind::Refused,
            message,
        }) => Verdict::Refused(message),
        Err(Failure {
            kind: FailureKind::Inconclusive,
            message,
        }) => Verdict::Inconclusive(message),
    }
}

/// What `audit` and `attested` are given on the command line.
pub struct AuditInputs {
    pub dir: Option<String>,
    pub operator: Option<String>,
    /// The trusted notary's signing key, as verify takes it.
    pub notary: String,
    pub verify: String,
    /// `--legacy-v1`: a version 1 record may be read. It changes nothing for version 2.
    pub legacy: bool,
    /// `--guest`: the guest a version 2 record is replayed on.
    pub guest: Option<String>,
}

/// The notary's signing key from its file's text: one compressed secp256k1 point in lowercase hex.
pub fn notary_key_text(text: &str) -> Result<String, String> {
    let key = text.trim();
    let hex_key = key.len() == 66 && key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if hex_key && (key.starts_with("02") || key.starts_with("03")) {
        Ok(key.to_string())
    } else {
        Err("the notary's public key is not a signing public key".into())
    }
}

/// The whole audit of one record. Everything is checked before anything is reported, so a failure
/// states no proof.
pub fn audit_record(
    record: &str,
    inputs: &AuditInputs,
    log: LogSource,
    verifier: &Verifier,
    files: &'static dyn Files,
) -> Result<String, Failure> {
    let bytes = files
        .read(Path::new(record), FileKind::Transcript)
        .map_err(|error| match error {
            FileError::TooLarge => Failure::refused("the record is over its size limit"),
            _ => Failure::inconclusive("cannot read"),
        })?;
    let t = read_transcript(&bytes)?;
    if t.version == 1 && !inputs.legacy {
        return Err(
            "this is a version 1 record, with no attempt budget: audit it with --legacy-v1".into(),
        );
    }
    // Every record but a version 1 record is replayed, so none reaches the report without it.
    let guest = if t.version == 1 {
        None
    } else {
        let path = inputs
            .guest
            .as_deref()
            .ok_or_else(|| Failure::inconclusive("the guest is not given: --guest <file>"))?;
        let guest = files
            .read(Path::new(path), FileKind::Guest)
            .map_err(|error| match error {
                FileError::TooLarge => Failure::refused("the guest is over its size limit"),
                _ => Failure::inconclusive("the guest is not a readable file"),
            })?;
        if sha(&guest) != t.guest_sha256 {
            return Err("the guest is not this record's".into());
        }
        Some(guest)
    };
    let salt = files
        .read(Path::new(&format!("{record}.salt")), FileKind::Salt)
        .map_err(|error| match error {
            FileError::Missing => Failure::refused("no salt for this record"),
            FileError::TooLarge => Failure::refused("the salt is over its size limit"),
            FileError::Unreadable => Failure::inconclusive("the salt cannot be read"),
        })?;
    let commitment = sha([
        salt.as_slice(),
        &serde_json::to_vec(&t).map_err(|_| "encode")?,
    ]
    .concat()
    .as_slice());
    let operator = inputs.operator.as_deref().ok_or_else(|| {
        Failure::inconclusive("the operator's public key is not given: --operator-public <file>")
    })?;
    let dir = inputs
        .dir
        .as_deref()
        .ok_or_else(|| Failure::inconclusive("the links directory is not given"))?;
    let claim = claim_of(&t, commitment)?;
    let audited = Auditor::new(dir, operator, log, files)?.audit(&claim)?;
    let answers = witnessed(
        &t,
        record,
        &inputs.notary,
        verifier,
        Some((&audited.intent, audited.window)),
        files,
    )?;
    if t.version != 1 {
        let attempt = answers
            .attempt
            .as_ref()
            .ok_or("the attestations carry no ticket")?;
        if attempt.task != claim.task {
            return Err("the ticket's task is not this record's".into());
        }
    }
    // Always in this process, so the audit opens no kernel file.
    if let Some(guest) = &guest {
        replay(guest, &t).map_err(|_| "the record does not replay on its guest")?;
    }
    let proofs = t
        .answers
        .iter()
        .map(|answer| answer.provider_proofs.len())
        .sum();
    Ok(audit_report(
        &audited,
        answers.lines.len(),
        answers.test_settings,
        answers.attempt.as_ref(),
        proofs,
        answers.oldest_pass,
    ))
}

/// What the record itself says it is, for the audit to hold the chain to.
pub fn claim_of(t: &Transcript, commitment: String) -> Result<Claim, String> {
    let inputs = t
        .inputs_hex
        .iter()
        .map(|i| unhex(i))
        .collect::<Option<Vec<_>>>()
        .ok_or("the record's inputs are unreadable")?;
    let task = if t.version == 1 {
        task_of(&t.guest_sha256, &inputs)
    } else {
        task_for(t.version, t.profile.as_deref(), &t.guest_sha256, &inputs)
            .ok_or("the record's guest hash is unreadable")?
    };
    Ok(Claim {
        commitment,
        task,
        guest: t.guest_sha256.clone(),
        version: t.version,
    })
}
