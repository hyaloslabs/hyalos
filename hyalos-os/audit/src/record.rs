//! The record: its types, version rules and task identities.

use hyalos_profile::Profile;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The guest interface both backends implement, as `ABI.md` states it. A record does not carry it.
pub const ABI: &str = "hyalos.abi/1";
pub const MAX_QUESTION: usize = 4 * 1024;
pub const MAX_BODY: usize = 64 * 1024;
pub const MAX_RESULT: usize = 4 * 1024;
pub const MAX_EFFECTS: usize = 1;
/// Schema 0: 0 bug, 1 feature, 2 security, 3 other.
pub const LABELS: i32 = 4;
pub const NOTIFY: i32 = 0;
/// The record and link version new runs write.
pub const CURRENT_VERSION: u32 = 4;
/// How old a signed pass may be at its session. Pinned to the notary's by a test.
pub const CANARY_WINDOW_SECONDS: u64 = 86_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    InvalidModule,
    BadImport,
    BadExport,
    OutOfFuel,
    GuestTrap,
    BadPointer,
    TooLarge,
    UnknownSchema,
    UnknownEffect,
    EffectLimit,
    NoResult,
    ReplayDiverged,
    GuestMismatch,
    KernelFault,
    BadHypercall,
    Timeout,
    NoKvm,
    ModelUnavailable,
    ModelRefused,
    NotaryUnreachable,
    NotaryDeadline,
    WrongNotaryKey,
    ReleaseMismatch,
    ProviderRefused,
    NoAuthorisation,
    AttemptsSpent,
    CanaryHeld,
    EffectFailed,
    LedgerFailed,
    /// A run whose input record 0 is not a contract, refused before its guest loads. Only a spike
    /// run raises it until `hyalos.abi/2` is implemented; that section names it (ADR-0138).
    BadContract,
    /// An inputs file that is not a well-formed framed file, or a version 4 run's record that is
    /// not one line of UTF-8, refused before the guest loads (ADR-0138).
    BadInputs,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub schema: i32,
    pub question_sha256: String,
    pub label: i32,
    pub model: String,
    pub response_id: String,
    /// The hash of a notary's presentation of this answer, when the call was attested.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub attestation: String,
    /// Proofs a provider may one day supply. None is checked and none counts as evidence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provider_proofs: Vec<ProviderProof>,
}

/// One named provider proof. No kind is known, so the audit only counts these (ADR-006).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderProof {
    pub kind: String,
    pub sha256: String,
}

impl ProviderProof {
    pub fn well_formed(&self) -> bool {
        let kind = !self.kind.is_empty()
            && self.kind.len() <= 64
            && self
                .kind
                .bytes()
                .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'/'));
        let digest = self.sha256.len() == 64
            && self
                .sha256
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        kind && digest
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectRecord {
    pub kind: i32,
    pub body_hex: String,
    pub receipt: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transcript {
    pub version: u32,
    /// The execution profile's name. Required in version 4, and absent from older versions' bytes.
    #[serde(
        default,
        deserialize_with = "present_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub profile: Option<String>,
    /// The framed task identity. Required in version 2, and absent from version 1's bytes.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub task: String,
    pub guest_sha256: String,
    pub inputs_hex: Vec<String>,
    pub answers: Vec<Answer>,
    pub effects: Vec<EffectRecord>,
    pub result_hex: String,
    pub fuel_used: u64,
}

/// A field that is present must hold a string: `null` is not the same as leaving it out.
fn present_string<'de, D: serde::Deserializer<'de>>(reader: D) -> Result<Option<String>, D::Error> {
    String::deserialize(reader).map(Some)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

pub fn sha(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// Reads a record, refusing a version this program does not know and a task that is not the one
/// its own guest and inputs give. Every command that takes a record reads it here.
pub fn read_transcript(bytes: &[u8]) -> Result<Transcript, String> {
    let t: Transcript = serde_json::from_slice(bytes).map_err(|_| "cannot parse")?;
    if matches!(t.version, 1..=4) {
        profile_for(t.version, t.profile.as_deref())?;
        let mut proofs = t
            .answers
            .iter()
            .flat_map(|answer| &answer.provider_proofs)
            .peekable();
        if t.version == 1 && proofs.peek().is_some() {
            return Err("a version 1 record cannot carry a provider proof".into());
        }
        if !proofs.all(ProviderProof::well_formed) {
            return Err("a provider proof is malformed".into());
        }
    }
    match t.version {
        1 if t.task.is_empty() => Ok(t),
        1 => Err("a version 1 record cannot carry a task".into()),
        version @ 2..=4 if t.task.is_empty() => {
            Err(format!("this version {version} record has no task"))
        }
        2..=4 => {
            let inputs = t
                .inputs_hex
                .iter()
                .map(|i| unhex(i))
                .collect::<Option<Vec<_>>>()
                .ok_or("the record's inputs are unreadable")?;
            if task_for(t.version, t.profile.as_deref(), &t.guest_sha256, &inputs).as_deref()
                == Some(t.task.as_str())
            {
                Ok(t)
            } else {
                Err("this record's task is not the one its guest and inputs give".into())
            }
        }
        other => Err(format!("record version {other} is not known")),
    }
}

/// A task's name: its guest and inputs, without a salt, so repeated runs of one task show as such.
pub fn task_of(guest_sha256: &str, inputs: &[Vec<u8>]) -> String {
    sha(format!("{guest_sha256}:{}", sha(&inputs.concat())).as_bytes())
}

/// The version 2 task identity: a domain tag, the guest's hash as raw bytes, the input count, then
/// each input with its length, so two different input lists can never give the same bytes.
/// `None` when the guest hash is not 32 bytes of hex.
pub fn task_v2(guest_sha256: &str, inputs: &[Vec<u8>]) -> Option<String> {
    framed_task(b"hyalos.task/2", None, guest_sha256, inputs)
}

/// The version 4 task identity: version 2's frame under its own tag, with the profile's name and
/// length before the guest, so one authorisation covers one task under one profile.
pub fn task_v4(profile: &str, guest_sha256: &str, inputs: &[Vec<u8>]) -> Option<String> {
    framed_task(b"hyalos.task/4", Some(profile), guest_sha256, inputs)
}

/// The identity a new run has, which is what a customer authorises.
pub fn current_task(guest_sha256: &str, inputs: &[Vec<u8>]) -> Option<String> {
    task_v4(hyalos_profile::CURRENT.name, guest_sha256, inputs)
}

/// The framed identity of a version 2 or later record. `None` for any other version, or a version 4
/// record that names no profile.
pub fn task_for(
    version: u32,
    profile: Option<&str>,
    guest_sha256: &str,
    inputs: &[Vec<u8>],
) -> Option<String> {
    match version {
        2 | 3 => task_v2(guest_sha256, inputs),
        4 => task_v4(profile?, guest_sha256, inputs),
        _ => None,
    }
}

/// The profile a record of this version runs under. Versions 1 to 3 imply profile 1 and may not
/// name one, and version 4 must name a known one. Either way, it must be one this build's `wasmi`
/// replays (ADR-0134).
pub fn profile_for(version: u32, named: Option<&str>) -> Result<&'static Profile, String> {
    match (version, named) {
        (1..=3, None) => hyalos_profile::lookup(hyalos_profile::PROFILE_1.name)
            .ok_or_else(|| "this build cannot replay hyalos.profile/1".into()),
        (1..=3, Some(_)) => Err(format!(
            "unknown field `profile` in a version {version} record"
        )),
        (4, None) => Err("version 4 record names no profile".into()),
        (4, Some(name)) => hyalos_profile::lookup(name)
            .ok_or_else(|| format!("unknown profile {}", shown_profile(name))),
        _ => Err(format!("record version {version} is not known")),
    }
}

/// A record's profile name for a refusal. It is the operator's text, so only a plain name is echoed.
pub fn shown_profile(name: &str) -> &str {
    let plain = !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'/'));
    if plain { name } else { "<not a profile name>" }
}

pub fn framed_task(
    tag: &[u8],
    profile: Option<&str>,
    guest_sha256: &str,
    inputs: &[Vec<u8>],
) -> Option<String> {
    let guest = unhex(guest_sha256).filter(|g| g.len() == 32)?;
    let mut framed = Sha256::new();
    framed.update(tag);
    if let Some(name) = profile {
        framed.update((name.len() as u64).to_be_bytes());
        framed.update(name.as_bytes());
    }
    framed.update(&guest);
    framed.update((inputs.len() as u64).to_be_bytes());
    for input in inputs {
        framed.update((input.len() as u64).to_be_bytes());
        framed.update(input);
    }
    Some(hex(&framed.finalize()))
}
