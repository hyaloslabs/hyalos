//! The trusted-root snapshot (S7.11, ADR-0007, ADR-0097): the one this release embeds, and which
//! successor is accepted. The tests sign snapshots with a Sigstore of their own, as the pipeline's
//! identity, so a chain of snapshots can be made; the embedded one is the pipeline's own.

use hyalos_plugin::sigstore::{PIPELINE, Refusal};
use hyalos_plugin::snapshot::{KIND, MAX_SNAPSHOT, Snapshot, SnapshotError};
use hyalos_sigstore_testkit::{Signer, TestSigstore};
use hyalos_types::Timestamp;

type Res<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

const EMBEDDED: &[u8] = include_bytes!("../trust/trusted-root.json");
const EMBEDDED_BUNDLE: &[u8] = include_bytes!("../trust/trusted-root.json.sigstore.json");

/// The pipeline's identity, as the test Sigstore writes it into a certificate.
const AS_PIPELINE: Signer<'static> = Signer {
    subject: PIPELINE.subject,
    issuer: PIPELINE.issuer,
    repository: PIPELINE.repository,
    git_ref: PIPELINE.git_ref,
};

/// A snapshot file holding `root`, as `tools/snapshot.py` wraps one.
fn wrap(root: &str) -> Res<Vec<u8>> {
    let root: serde_json::Value = serde_json::from_str(root)?;
    Ok(serde_json::to_vec(&serde_json::json!({
        "kind": KIND,
        "trusted_root": root,
        "version": 1,
    }))?)
}

/// A snapshot trusting `sigstore`, as of `at`.
fn trusting(sigstore: &TestSigstore, at: u64) -> Res<Snapshot> {
    Ok(Snapshot::trusted(
        &sigstore.root()?,
        PIPELINE,
        Timestamp::from_millis(at),
    )?)
}

#[test]
fn the_embedded_snapshot_is_the_pipelines_and_verifies_under_itself() -> Res {
    let embedded = Snapshot::embedded()?;
    // Release r8 of hyaloslabs/marketplace signed it, at 2026-11-17T01:21:03Z by Rekor's clock.
    assert_eq!(
        embedded.signed_at(),
        Timestamp::from_millis(1_794_878_463_000)
    );
    assert_eq!(
        embedded.digest(),
        hyalos_plugin::skill::digest_of(EMBEDDED),
        "the digest is of the file as published"
    );
    let body: serde_json::Value = serde_json::from_slice(EMBEDDED)?;
    assert_eq!(body["kind"], KIND);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(embedded.root())?,
        body["trusted_root"]
    );
    // The same snapshot again is not newer than itself.
    assert_eq!(
        embedded.successor(EMBEDDED, EMBEDDED_BUNDLE).unwrap_err(),
        SnapshotError::NotNewer
    );
    Ok(())
}

#[test]
fn a_newer_snapshot_the_one_in_force_signed_is_accepted_and_takes_over() -> Res {
    let (old, new) = (TestSigstore::new()?, TestSigstore::new()?);
    let current = trusting(&old, 0)?;
    let file = wrap(&new.root()?)?;
    let bundle = old.sign(&file, AS_PIPELINE)?;
    let next = current.successor(&file, &bundle)?;
    assert!(next.signed_at() > current.signed_at());
    assert_eq!(next.digest(), hyalos_plugin::skill::digest_of(&file));
    // A release the new root's Sigstore signed now verifies, and one the old root's does not.
    let artifact = b"a release";
    next.trust()?
        .verify(artifact, &new.sign(artifact, AS_PIPELINE)?)?;
    assert!(
        next.trust()?
            .verify(artifact, &old.sign(artifact, AS_PIPELINE)?)
            .is_err()
    );
    Ok(())
}

#[test]
fn a_snapshot_the_one_in_force_cannot_verify_needs_a_new_release() -> Res {
    let (old, new) = (TestSigstore::new()?, TestSigstore::new()?);
    let current = trusting(&old, 0)?;
    let file = wrap(&new.root()?)?;
    // Signed by an authority the snapshot in force does not list, as after a rotation.
    let err = current
        .successor(&file, &new.sign(&file, AS_PIPELINE)?)
        .unwrap_err();
    assert!(matches!(err, SnapshotError::Unverifiable(_)), "{err:?}");
    assert_eq!(err.code().as_str(), "snapshot.unverifiable");
    assert!(
        err.to_string().contains("a new Hyalos release is needed"),
        "{err}"
    );
    // And by the right authority for another identity.
    let other = Signer {
        subject: "https://github.com/hyaloslabs/marketplace/.github/workflows/ci.yml@refs/heads/main",
        ..AS_PIPELINE
    };
    assert_eq!(
        current
            .successor(&file, &old.sign(&file, other)?)
            .unwrap_err(),
        SnapshotError::Unverifiable(Refusal::Identity)
    );
    // An unsigned snapshot, or one with a bundle for other bytes, is refused too.
    assert!(matches!(
        current.successor(&file, b"{}").unwrap_err(),
        SnapshotError::Unverifiable(_)
    ));
    assert_eq!(
        current
            .successor(&file, &old.sign(b"other bytes", AS_PIPELINE)?)
            .unwrap_err(),
        SnapshotError::Unverifiable(Refusal::Digest)
    );
    Ok(())
}

#[test]
fn an_older_snapshot_is_refused() -> Res {
    let (old, new) = (TestSigstore::new()?, TestSigstore::new()?);
    // The snapshot in force was issued after anything the test can sign now.
    let current = trusting(&old, u64::MAX / 2)?;
    let file = wrap(&new.root()?)?;
    assert_eq!(
        current
            .successor(&file, &old.sign(&file, AS_PIPELINE)?)
            .unwrap_err(),
        SnapshotError::NotNewer
    );
    Ok(())
}

#[test]
fn what_is_not_a_snapshot_is_refused_even_when_signed() -> Res {
    let old = TestSigstore::new()?;
    let current = trusting(&old, 0)?;
    let root: serde_json::Value = serde_json::from_str(&old.root()?)?;
    let cases = [
        serde_json::json!({"kind": "hyalos.other", "trusted_root": root, "version": 1}),
        serde_json::json!({"kind": KIND, "trusted_root": root, "version": 2}),
        serde_json::json!({"kind": KIND, "version": 1}),
        root.clone(),
    ];
    for case in cases {
        let file = serde_json::to_vec(&case)?;
        assert_eq!(
            current
                .successor(&file, &old.sign(&file, AS_PIPELINE)?)
                .unwrap_err(),
            SnapshotError::Malformed,
            "{case}"
        );
    }
    // A snapshot whose root this build cannot use.
    let file = serde_json::to_vec(&serde_json::json!({
        "kind": KIND,
        "trusted_root": {"mediaType": "application/vnd.dev.sigstore.trustedroot+json;version=0.1"},
        "version": 1,
    }))?;
    assert_eq!(
        current
            .successor(&file, &old.sign(&file, AS_PIPELINE)?)
            .unwrap_err(),
        SnapshotError::Unusable
    );
    let big = vec![b' '; MAX_SNAPSHOT as usize + 1];
    assert_eq!(
        current.successor(&big, b"{}").unwrap_err(),
        SnapshotError::Malformed
    );
    Ok(())
}

#[test]
fn a_kept_snapshot_reads_back_with_its_recorded_time() -> Res {
    let kept = Snapshot::kept(EMBEDDED, Timestamp::from_millis(7))?;
    assert_eq!(kept.signed_at(), Timestamp::from_millis(7));
    assert_eq!(kept.root(), Snapshot::embedded()?.root());
    assert_eq!(
        Snapshot::kept(b"{}", Timestamp::from_millis(7)).unwrap_err(),
        SnapshotError::Malformed
    );
    Ok(())
}
