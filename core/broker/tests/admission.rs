//! The broker refuses a job it should not serve (S3.1, ADR-0004, ADR-0031).

use std::cell::Cell;
use std::collections::BTreeSet;

use ed25519_dalek::Signer;
use hyalos_broker::{Admission, Credential, LiveJobs, Refusal};
use hyalos_types::{
    Budget, Clock, JobId, MANIFEST_VERSION, Manifest, ManifestKey, OpenError, Role, SignedManifest,
    Timestamp,
};

type Res = Result<(), Box<dyn std::error::Error>>;

struct Now(Cell<u64>);

impl Clock for Now {
    fn now(&self) -> Timestamp {
        Timestamp::from_millis(self.0.get())
    }
}

struct Live(BTreeSet<u64>);

impl LiveJobs for Live {
    fn is_live(&self, job: JobId) -> bool {
        self.0.contains(&job.seq())
    }
}

const JOB: u64 = 12;
const DEADLINE: u64 = 10_000;
const SEED: [u8; 32] = [3; 32];
const TAG: &[u8] = b"hyalos.manifest.signature\0";

fn manifest(role: Role) -> Manifest {
    Manifest {
        version: MANIFEST_VERSION,
        job: JobId::from_seq(JOB),
        role,
        tainted: false,
        repo: "hyaloslabs/hyalos".into(),
        branch_prefix: "hyalos/".into(),
        pr_base: None,
        actions: BTreeSet::new(),
        egress_hosts: BTreeSet::new(),
        tools: BTreeSet::new(),
        data_flows: BTreeSet::new(),
        budget: Budget { tokens: 1 },
        deadline: Timestamp::from_millis(DEADLINE),
        approval: None,
        handles: Default::default(),
        result: None,
        closes: None,
    }
}

fn key() -> ManifestKey {
    ManifestKey::from_seed(SEED)
}

fn credential(role: Role) -> Credential {
    Credential {
        job: JobId::from_seq(JOB),
        role,
    }
}

/// A signature made by hand with the engine's key, as only the engine can: under the tag and the
/// version, over whatever bytes it is given.
fn engine_signs(version: u32, payload: &[u8]) -> [u8; 64] {
    let mut message = TAG.to_vec();
    message.extend_from_slice(&version.to_be_bytes());
    message.extend_from_slice(payload);
    ed25519_dalek::SigningKey::from_bytes(&SEED)
        .sign(&message)
        .to_bytes()
}

struct Rig {
    admission: Admission,
    live: Live,
    clock: Now,
}

fn rig() -> Rig {
    Rig {
        admission: Admission::new(key().public()),
        live: Live([JOB].into()),
        clock: Now(Cell::new(1_000)),
    }
}

impl Rig {
    fn admit(&self, signed: &SignedManifest, credential: &Credential) -> Result<Manifest, Refusal> {
        self.admission
            .admit(signed, credential, &self.live, &self.clock)
    }
}

#[test]
fn a_manifest_the_engine_signed_for_a_live_job_and_its_role_is_admitted() -> Res {
    let r = rig();
    for role in Role::ALL {
        let signed = key().sign(&manifest(role))?;
        assert_eq!(r.admit(&signed, &credential(role)), Ok(manifest(role)));
    }
    Ok(())
}

#[test]
fn a_signature_that_is_not_the_engines_is_refused() -> Res {
    let r = rig();
    let signed = key().sign(&manifest(Role::Actor))?;
    let bad = Err(Refusal::Manifest(OpenError::BadSignature));

    // Signed by another key.
    let other = ManifestKey::from_seed([4; 32]).sign(&manifest(Role::Actor))?;
    assert_eq!(r.admit(&other, &credential(Role::Actor)), bad);

    // One byte of the document changed after signing, anywhere in it.
    for i in 0..signed.payload().len() {
        let mut payload = signed.payload().to_vec();
        payload[i] ^= 1;
        let tampered = SignedManifest::from_parts(signed.version(), payload, *signed.signature());
        assert_eq!(
            r.admit(&tampered, &credential(Role::Actor)),
            bad,
            "byte {i}"
        );
    }

    // One bit of the signature changed, anywhere in it, and a signature of zeros.
    for i in 0..64 {
        let mut signature = *signed.signature();
        signature[i] ^= 0x80;
        let forged =
            SignedManifest::from_parts(signed.version(), signed.payload().to_vec(), signature);
        assert_eq!(r.admit(&forged, &credential(Role::Actor)), bad, "byte {i}");
    }
    let zeros = SignedManifest::from_parts(signed.version(), signed.payload().to_vec(), [0; 64]);
    assert_eq!(r.admit(&zeros, &credential(Role::Actor)), bad);
    Ok(())
}

#[test]
fn a_signature_over_the_payload_alone_is_not_a_manifest_signature() -> Res {
    // Signatures are made under a tag, so whatever else the engine signs, even the same bytes, cannot
    // be sent as a manifest. This signs the payload with no tag, as another message would be.
    let r = rig();
    let payload = key().sign(&manifest(Role::Actor))?.payload().to_vec();
    let raw = ed25519_dalek::SigningKey::from_bytes(&SEED)
        .sign(&payload)
        .to_bytes();
    let untagged = SignedManifest::from_parts(MANIFEST_VERSION, payload, raw);
    assert_eq!(
        r.admit(&untagged, &credential(Role::Actor)),
        Err(Refusal::Manifest(OpenError::BadSignature))
    );
    Ok(())
}

#[test]
fn an_unknown_version_is_refused_even_with_a_valid_signature() -> Res {
    let r = rig();
    let payload = serde_json::to_vec(&manifest(Role::Actor))?;
    for version in [0, MANIFEST_VERSION + 1, u32::MAX] {
        // Signed properly under that version, so only the version is wrong.
        let signed =
            SignedManifest::from_parts(version, payload.clone(), engine_signs(version, &payload));
        assert_eq!(
            r.admit(&signed, &credential(Role::Actor)),
            Err(Refusal::Manifest(OpenError::UnknownVersion {
                found: version
            }))
        );
    }
    Ok(())
}

#[test]
fn a_known_version_cannot_be_relabelled_after_signing() -> Res {
    // The signature covers the version, so an envelope sent as another known version fails on the
    // signature. (Only one version exists today; the version check must not be the only guard.)
    let r = rig();
    let signed = key().sign(&manifest(Role::Actor))?;
    let relabelled = SignedManifest::from_parts(
        MANIFEST_VERSION,
        signed.payload().to_vec(),
        engine_signs(0, signed.payload()),
    );
    assert_eq!(
        r.admit(&relabelled, &credential(Role::Actor)),
        Err(Refusal::Manifest(OpenError::BadSignature))
    );
    Ok(())
}

#[test]
fn a_document_that_says_another_version_than_it_was_sent_under_is_refused() -> Res {
    // Properly signed, as the engine's own signing call never would: it refuses to.
    let r = rig();
    let mut m = manifest(Role::Actor);
    m.version = MANIFEST_VERSION + 1;
    let payload = serde_json::to_vec(&m)?;
    let signed = SignedManifest::from_parts(
        MANIFEST_VERSION,
        payload.clone(),
        engine_signs(MANIFEST_VERSION, &payload),
    );
    assert_eq!(
        r.admit(&signed, &credential(Role::Actor)),
        Err(Refusal::Manifest(OpenError::VersionMismatch {
            envelope: MANIFEST_VERSION,
            document: MANIFEST_VERSION + 1
        }))
    );
    Ok(())
}

#[test]
fn a_signed_payload_that_is_not_a_manifest_is_refused() {
    let r = rig();
    for payload in [&b"{}"[..], b"not json", b"", b"{\"version\":1}"] {
        let signed = SignedManifest::from_parts(
            MANIFEST_VERSION,
            payload.to_vec(),
            engine_signs(MANIFEST_VERSION, payload),
        );
        assert_eq!(
            r.admit(&signed, &credential(Role::Actor)),
            Err(Refusal::Manifest(OpenError::Malformed)),
            "{payload:?}"
        );
    }
}

#[test]
fn a_manifest_past_its_deadline_is_refused_and_one_at_it_is_not() -> Res {
    let r = rig();
    let signed = key().sign(&manifest(Role::Actor))?;
    let c = credential(Role::Actor);
    r.clock.0.set(DEADLINE - 1);
    assert!(r.admit(&signed, &c).is_ok());
    r.clock.0.set(DEADLINE);
    assert!(
        r.admit(&signed, &c).is_ok(),
        "the deadline itself is still valid"
    );
    r.clock.0.set(DEADLINE + 1);
    assert_eq!(
        r.admit(&signed, &c),
        Err(Refusal::Expired {
            deadline: Timestamp::from_millis(DEADLINE),
            now: Timestamp::from_millis(DEADLINE + 1)
        })
    );
    r.clock.0.set(u64::MAX);
    assert!(matches!(r.admit(&signed, &c), Err(Refusal::Expired { .. })));
    Ok(())
}

#[test]
fn a_job_that_is_not_live_is_refused() -> Res {
    let mut r = rig();
    let signed = key().sign(&manifest(Role::Actor))?;
    let c = credential(Role::Actor);
    assert!(r.admit(&signed, &c).is_ok());
    let gone = Err(Refusal::JobNotLive(JobId::from_seq(JOB)));
    // The job ended, or never started: a replay of an old manifest.
    r.live = Live(BTreeSet::new());
    assert_eq!(r.admit(&signed, &c), gone);
    // Another job being live does not help.
    r.live = Live([JOB + 1].into());
    assert_eq!(r.admit(&signed, &c), gone);
    Ok(())
}

#[test]
fn a_manifest_presented_with_another_jobs_credential_is_refused() -> Res {
    let mut r = rig();
    r.live = Live([JOB, JOB + 1].into());
    let signed = key().sign(&manifest(Role::Actor))?;
    let theirs = Credential {
        job: JobId::from_seq(JOB + 1),
        role: Role::Actor,
    };
    assert_eq!(
        r.admit(&signed, &theirs),
        Err(Refusal::WrongJob {
            manifest: JobId::from_seq(JOB),
            credential: JobId::from_seq(JOB + 1)
        })
    );
    Ok(())
}

#[test]
fn a_manifest_whose_role_is_not_the_credentials_is_refused_for_every_pair() -> Res {
    let r = rig();
    for manifest_role in Role::ALL {
        let signed = key().sign(&manifest(manifest_role))?;
        for credential_role in Role::ALL {
            let result = r.admit(&signed, &credential(credential_role));
            if manifest_role == credential_role {
                assert!(result.is_ok(), "{manifest_role:?}");
            } else {
                assert_eq!(
                    result,
                    Err(Refusal::RoleMismatch {
                        manifest: manifest_role,
                        credential: credential_role
                    }),
                    "{manifest_role:?} with a {credential_role:?} credential"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn a_refusal_says_why_and_nothing_from_inside_the_document() -> Res {
    let r = rig();
    let signed = key().sign(&manifest(Role::Actor))?;
    r.clock.0.set(DEADLINE + 5);
    let said = r
        .admit(&signed, &credential(Role::Actor))
        .err()
        .ok_or("expected a refusal")?
        .to_string();
    assert!(said.contains("deadline"), "{said}");
    for secret in ["hyaloslabs/hyalos", "hyalos/"] {
        assert!(!said.contains(secret), "{said}");
    }
    Ok(())
}

#[test]
fn what_travels_to_the_broker_is_checked_the_same_way() -> Res {
    let r = rig();
    let signed = key().sign(&manifest(Role::Planner))?;
    let received = SignedManifest::from_bytes(&signed.to_bytes()?)?;
    assert_eq!(
        r.admit(&received, &credential(Role::Planner)),
        Ok(manifest(Role::Planner))
    );
    for junk in [&b""[..], b"{}", b"[]", b"null", b"\xff\xfe"] {
        assert!(SignedManifest::from_bytes(junk).is_err(), "{junk:?}");
    }
    Ok(())
}
