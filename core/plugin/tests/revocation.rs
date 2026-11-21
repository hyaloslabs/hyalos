//! The revocation list (S7.7, ADR-0098): which list is accepted, and what it revokes. The tests sign
//! lists with a Sigstore of their own, as the pipeline's identity.

use hyalos_plugin::revocation::{KIND, MAX_LIST, RevocationError, Revocations};
use hyalos_plugin::sigstore::{PIPELINE, Refusal};
use hyalos_plugin::snapshot::Snapshot;
use hyalos_sigstore_testkit::{Signer, TestSigstore};
use hyalos_types::{Digest, Timestamp};

type Res<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

const AS_PIPELINE: Signer<'static> = Signer {
    subject: PIPELINE.subject,
    issuer: PIPELINE.issuer,
    repository: PIPELINE.repository,
    git_ref: PIPELINE.git_ref,
};

fn hex(b: u8) -> String {
    format!("{b:02x}").repeat(32)
}

fn list(entries: serde_json::Value) -> Res<Vec<u8>> {
    Ok(serde_json::to_vec(
        &serde_json::json!({"kind": KIND, "revoked": entries, "version": 1}),
    )?)
}

fn trusting(sigstore: &TestSigstore) -> Res<Snapshot> {
    Ok(Snapshot::trusted(
        &sigstore.root()?,
        PIPELINE,
        Timestamp::from_millis(0),
    )?)
}

#[test]
fn a_list_the_pipeline_signed_revokes_what_it_lists() -> Res {
    let sigstore = TestSigstore::new()?;
    let snapshot = trusting(&sigstore)?;
    let file = list(serde_json::json!([{"digest": hex(1), "reason": "malware.exfiltration"}]))?;
    let accepted =
        Revocations::verify(&snapshot, &file, &sigstore.sign(&file, AS_PIPELINE)?, None)?;
    assert_eq!(accepted.len(), 1);
    assert_eq!(
        accepted
            .reason(&Digest::from_bytes([1; 32]))
            .map(|r| r.as_str()),
        Some("malware.exfiltration")
    );
    assert_eq!(accepted.reason(&Digest::from_bytes([2; 32])), None);
    assert_eq!(accepted.digest(), hyalos_plugin::skill::digest_of(&file));
    // The same list again, or one issued before, cannot replace it: a rollback could un-revoke.
    // Signed at the same second as the first: signing at the clock could land on the next second,
    // which really is newer. The log's time is 30 seconds before the time signed at.
    let same_second = accepted.signed_at().as_millis() / 1000 + 30;
    let again = Revocations::verify(
        &snapshot,
        &file,
        &sigstore.sign_at(&file, AS_PIPELINE, same_second)?,
        Some(accepted.signed_at()),
    );
    assert_eq!(again.unwrap_err(), RevocationError::NotNewer);
    // Read back as kept, with the time the log recorded.
    let kept = Revocations::kept(&file, accepted.signed_at())?;
    assert_eq!(kept, accepted);
    Ok(())
}

#[test]
fn a_list_not_signed_as_the_pipeline_under_the_snapshot_in_force_is_refused() -> Res {
    let (ours, other) = (TestSigstore::new()?, TestSigstore::new()?);
    let snapshot = trusting(&ours)?;
    let file = list(serde_json::json!([]))?;
    assert!(matches!(
        Revocations::verify(&snapshot, &file, &other.sign(&file, AS_PIPELINE)?, None),
        Err(RevocationError::Unverifiable(_))
    ));
    let someone = Signer {
        subject: "https://github.com/someone/fork/.github/workflows/release.yml@refs/heads/main",
        ..AS_PIPELINE
    };
    assert_eq!(
        Revocations::verify(&snapshot, &file, &ours.sign(&file, someone)?, None).unwrap_err(),
        RevocationError::Unverifiable(Refusal::Identity)
    );
    assert_eq!(
        Revocations::verify(&snapshot, &file, &ours.sign(b"other", AS_PIPELINE)?, None)
            .unwrap_err(),
        RevocationError::Unverifiable(Refusal::Digest)
    );
    assert_eq!(
        RevocationError::Unverifiable(Refusal::Digest)
            .code()
            .as_str(),
        "revocations.unverifiable"
    );
    Ok(())
}

#[test]
fn what_is_not_a_revocation_list_is_refused_even_when_signed() -> Res {
    let sigstore = TestSigstore::new()?;
    let snapshot = trusting(&sigstore)?;
    let ok = serde_json::json!({"digest": hex(1), "reason": "malware"});
    let cases = [
        serde_json::json!({"kind": "hyalos.trusted_root_snapshot", "revoked": [], "version": 1}),
        serde_json::json!({"kind": KIND, "revoked": [], "version": 2}),
        serde_json::json!({"kind": KIND, "revoked": [], "version": 1, "note": "x"}),
        serde_json::json!({"kind": KIND, "version": 1}),
        serde_json::json!({"kind": KIND, "revoked": [ok, ok], "version": 1}),
        serde_json::json!({"kind": KIND, "revoked": [{"digest": hex(1)}], "version": 1}),
        serde_json::json!({"kind": KIND, "revoked": [{"digest": "ab", "reason": "x"}], "version": 1}),
        serde_json::json!({"kind": KIND, "revoked": [{"digest": hex(1), "reason": "Free text"}], "version": 1}),
        serde_json::json!({"kind": KIND, "revoked": [{"digest": hex(1), "reason": "x", "by": "y"}], "version": 1}),
    ];
    for case in cases {
        let file = serde_json::to_vec(&case)?;
        assert_eq!(
            Revocations::verify(&snapshot, &file, &sigstore.sign(&file, AS_PIPELINE)?, None)
                .unwrap_err(),
            RevocationError::Malformed,
            "{case}"
        );
    }
    let big = vec![b' '; MAX_LIST as usize + 1];
    assert_eq!(
        Revocations::verify(&snapshot, &big, b"{}", None).unwrap_err(),
        RevocationError::Malformed
    );
    Ok(())
}
