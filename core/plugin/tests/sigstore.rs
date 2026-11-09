//! The Sigstore check (S7.12, ADR-0007, ADR-0093) against real public bundles, offline. The vectors
//! are in `tests/sigstore/` (see its README): spike #73's two cosign releases, and a GitHub Actions
//! bundle and a Rekor v2 bundle from sigstore-rust's own test data. Each case below is a row of the
//! spike's matrix or an acceptance line of S7.12.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hyalos_plugin::sigstore::{Identity, MAX_BUNDLE, PIPELINE, Refusal, SNAPSHOT, Trust};
use serde_json::Value;

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

const FRESH: &[u8] = include_bytes!("sigstore/cosign-v3.1.4/cosign_checksums.txt");
const FRESH_BUNDLE: &[u8] =
    include_bytes!("sigstore/cosign-v3.1.4/cosign_checksums.txt.sigstore.json");
const OLD: &[u8] = include_bytes!("sigstore/cosign-v3.0.1/cosign_checksums.txt");
const OLD_BUNDLE: &[u8] =
    include_bytes!("sigstore/cosign-v3.0.1/cosign_checksums.txt.sigstore.json");
const CONDA: &[u8] = include_bytes!("sigstore/github/signed-package-2.1.0-hb0f4dca_0.conda");
const CONDA_BUNDLE: &[u8] = include_bytes!("sigstore/github/conda-attestation.sigstore.json");
const REKOR_V2_BUNDLE: &[u8] =
    include_bytes!("sigstore/github/conda-attestation-rekor2.sigstore.json");

/// Who signed cosign's releases: a Google service account.
const COSIGN: Identity<'static> = Identity {
    subject: "keyless@projectsigstore.iam.gserviceaccount.com",
    issuer: "https://accounts.google.com",
    git_ref: None,
};

/// A GitHub Actions workflow on its main branch, with the ref Fulcio wrote into the certificate.
const WORKFLOW: Identity<'static> = Identity {
    subject: "https://github.com/prefix-dev/sigstore-example/.github/workflows/action.yaml@refs/heads/main",
    issuer: "https://token.actions.githubusercontent.com",
    git_ref: Some("refs/heads/main"),
};

/// Who signed the Rekor v2 bundle, on Sigstore's staging instance.
const STAGING_SIGNER: Identity<'static> = Identity {
    subject: "jku@goto.fi",
    issuer: "https://github.com/login/oauth",
    git_ref: None,
};

/// The staging log the Rekor v2 bundle is in (`log2025-alpha3.rekor.sigstage.dev`).
const REKOR_V2_LOG: &str = "09OnDKEw7/hpZiYVPoTRzRbglHk0sylsUovegnRUlJY=";

fn cosign() -> Res<Trust> {
    Ok(Trust::new(SNAPSHOT, COSIGN)?)
}

/// `bundle` with `edit` applied to its JSON.
fn edited(bundle: &[u8], edit: impl FnOnce(&mut Value)) -> Res<Vec<u8>> {
    let mut json: Value = serde_json::from_slice(bundle)?;
    edit(&mut json);
    Ok(serde_json::to_vec(&json)?)
}

/// A trusted root with `edit` applied to its JSON.
fn root(text: &str, edit: impl FnOnce(&mut Value)) -> Res<String> {
    let mut json: Value = serde_json::from_str(text)?;
    edit(&mut json);
    Ok(serde_json::to_string(&json)?)
}

fn material(b: &mut Value) -> &mut Value {
    &mut b["verificationMaterial"]
}

fn entry(b: &mut Value) -> &mut Value {
    &mut b["verificationMaterial"]["tlogEntries"][0]
}

fn remove(value: &mut Value, key: &str) {
    if let Some(map) = value.as_object_mut() {
        map.remove(key);
    }
}

#[test]
fn the_fresh_and_the_year_old_release_verify_offline() -> Res {
    let trust = cosign()?;
    trust.verify(FRESH, FRESH_BUNDLE)?;
    // Its certificate lived ten minutes, a year ago: it is checked at the bundle's signed time.
    trust.verify(OLD, OLD_BUNDLE)?;
    Ok(())
}

#[test]
fn a_changed_artifact_a_wrong_identity_and_a_flipped_signature_are_refused() -> Res {
    let trust = cosign()?;
    let mut changed = FRESH.to_vec();
    changed.extend_from_slice(b"tampered\n");
    assert_eq!(trust.verify(&changed, FRESH_BUNDLE), Err(Refusal::Digest));
    // The right bundle for its file, signed by someone else than the identity asked for.
    let attacker = Trust::new(
        SNAPSHOT,
        Identity {
            subject: "attacker@example.com",
            ..COSIGN
        },
    )?;
    assert_eq!(attacker.verify(FRESH, FRESH_BUNDLE), Err(Refusal::Identity));
    let other_issuer = Trust::new(
        SNAPSHOT,
        Identity {
            issuer: "https://token.actions.githubusercontent.com",
            ..COSIGN
        },
    )?;
    assert_eq!(
        other_issuer.verify(FRESH, FRESH_BUNDLE),
        Err(Refusal::Identity)
    );
    let flipped = edited(FRESH_BUNDLE, |b| {
        let signature = &mut b["messageSignature"]["signature"];
        if let Some(Ok(mut bytes)) = signature.as_str().map(|s| STANDARD.decode(s)) {
            bytes[0] ^= 1;
            *signature = Value::String(STANDARD.encode(bytes));
        }
    })?;
    assert_ne!(flipped, FRESH_BUNDLE);
    // The timestamp authority signed over the signature, so that is the first check to fail.
    assert!(trust.verify(FRESH, &flipped).is_err());
    Ok(())
}

#[test]
fn a_bundle_without_its_log_entry_proof_or_any_signed_time_is_refused() -> Res {
    let trust = cosign()?;
    let no_entries = edited(FRESH_BUNDLE, |b| {
        material(b)["tlogEntries"] = Value::Array(vec![]);
    })?;
    assert_eq!(trust.verify(FRESH, &no_entries), Err(Refusal::Log));
    let no_proof = edited(FRESH_BUNDLE, |b| remove(entry(b), "inclusionProof"))?;
    assert_eq!(trust.verify(FRESH, &no_proof), Err(Refusal::Log));
    // Either signed time is enough on its own: the log's (its promise) or the timestamp authority's.
    let no_promise = edited(FRESH_BUNDLE, |b| remove(entry(b), "inclusionPromise"))?;
    trust.verify(FRESH, &no_promise)?;
    let no_timestamp = edited(FRESH_BUNDLE, |b| {
        remove(material(b), "timestampVerificationData")
    })?;
    trust.verify(FRESH, &no_timestamp)?;
    let no_time = edited(FRESH_BUNDLE, |b| {
        remove(entry(b), "inclusionPromise");
        remove(material(b), "timestampVerificationData");
    })?;
    assert_eq!(trust.verify(FRESH, &no_time), Err(Refusal::Time));
    Ok(())
}

#[test]
fn an_authority_the_snapshot_does_not_list_refuses_the_bundle() -> Res {
    // No fallback to a weaker check: a timestamp it cannot verify is a refusal, even beside the
    // log's own signed time.
    let no_tsa = root(SNAPSHOT, |r| {
        r["timestampAuthorities"] = Value::Array(vec![])
    })?;
    assert!(
        Trust::new(&no_tsa, COSIGN)?
            .verify(FRESH, FRESH_BUNDLE)
            .is_err()
    );
    let no_ca = root(SNAPSHOT, |r| {
        r["certificateAuthorities"] = Value::Array(vec![])
    })?;
    assert_eq!(
        Trust::new(&no_ca, COSIGN)?.verify(FRESH, FRESH_BUNDLE),
        Err(Refusal::Certificate)
    );
    let no_ct = root(SNAPSHOT, |r| r["ctlogs"] = Value::Array(vec![]))?;
    assert!(
        Trust::new(&no_ct, COSIGN)?
            .verify(FRESH, FRESH_BUNDLE)
            .is_err()
    );
    Ok(())
}

#[test]
fn a_github_actions_workflow_identity_verifies_offline_with_its_ref() -> Res {
    Trust::new(SNAPSHOT, WORKFLOW)?.verify(CONDA, CONDA_BUNDLE)?;
    // The same workflow file run from another branch is another identity.
    let other_ref = Trust::new(
        SNAPSHOT,
        Identity {
            git_ref: Some("refs/heads/release"),
            ..WORKFLOW
        },
    )?;
    assert_eq!(
        other_ref.verify(CONDA, CONDA_BUNDLE),
        Err(Refusal::Identity)
    );
    Ok(())
}

#[test]
fn only_the_pipelines_release_workflow_is_trusted_by_this_build() -> Res {
    let pinned = Trust::pinned()?;
    assert_eq!(
        PIPELINE.issuer,
        "https://token.actions.githubusercontent.com"
    );
    assert!(
        PIPELINE
            .subject
            .ends_with(&format!("@{}", PIPELINE.git_ref.unwrap_or("-")))
    );
    // A plugin author's own workflow signed this, and it verifies for that identity (above). It is
    // not the pipeline, so an install refuses it.
    assert_eq!(pinned.verify(CONDA, CONDA_BUNDLE), Err(Refusal::Identity));
    assert_eq!(pinned.verify(FRESH, FRESH_BUNDLE), Err(Refusal::Identity));
    Ok(())
}

#[test]
fn a_rekor_v2_bundle_verifies_only_against_a_snapshot_listing_its_log() -> Res {
    let staging = sigstore_verify::trust_root::SIGSTORE_STAGING_TRUSTED_ROOT;
    let listed = |r: &Value| {
        r["tlogs"]
            .as_array()
            .is_some_and(|logs| logs.iter().any(|l| l["logId"]["keyId"] == REKOR_V2_LOG))
    };
    assert!(listed(&serde_json::from_str(staging)?));
    Trust::new(staging, STAGING_SIGNER)?.verify(CONDA, REKOR_V2_BUNDLE)?;
    let without = root(staging, |r| {
        if let Some(logs) = r["tlogs"].as_array_mut() {
            logs.retain(|l| l["logId"]["keyId"] != REKOR_V2_LOG);
        }
    })?;
    assert!(!listed(&serde_json::from_str(&without)?));
    assert_eq!(
        Trust::new(&without, STAGING_SIGNER)?.verify(CONDA, REKOR_V2_BUNDLE),
        Err(Refusal::Log)
    );
    // The production snapshot this build carries does not list a staging log either.
    assert!(
        Trust::new(SNAPSHOT, STAGING_SIGNER)?
            .verify(CONDA, REKOR_V2_BUNDLE)
            .is_err()
    );
    Ok(())
}

#[test]
fn what_is_not_a_bundle_or_not_a_root_is_refused_by_code() -> Res {
    let trust = cosign()?;
    assert_eq!(trust.verify(FRESH, b"not json"), Err(Refusal::Malformed));
    assert_eq!(trust.verify(FRESH, b"{}"), Err(Refusal::Malformed));
    assert_eq!(trust.verify(FRESH, &[0xff, 0xfe]), Err(Refusal::Malformed));
    let large = vec![b' '; MAX_BUNDLE as usize + 1];
    assert_eq!(trust.verify(FRESH, &large), Err(Refusal::TooLarge));
    assert!(matches!(Trust::new("{}", COSIGN), Err(Refusal::Snapshot)));
    assert!(matches!(
        Trust::new("not json", COSIGN),
        Err(Refusal::Snapshot)
    ));
    for refusal in [Refusal::Identity, Refusal::Log, Refusal::Time] {
        assert!(refusal.code().as_str().starts_with("sigstore."));
    }
    Ok(())
}

#[test]
fn a_bundle_from_a_test_sigstore_verifies_against_its_root_and_no_other() -> Res {
    use hyalos_sigstore_testkit::{Signer, TestSigstore};
    let sigstore = TestSigstore::new().map_err(|e| e.to_string())?;
    let root = sigstore.root().map_err(|e| e.to_string())?;
    let signer = Signer {
        subject: PIPELINE.subject,
        issuer: PIPELINE.issuer,
        git_ref: PIPELINE.git_ref,
    };
    let artifact = b"a skill archive";
    let bundle = sigstore.sign(artifact, signer).map_err(|e| e.to_string())?;
    Trust::new(&root, PIPELINE)?.verify(artifact, &bundle)?;
    assert_eq!(
        Trust::new(&root, PIPELINE)?.verify(b"another archive", &bundle),
        Err(Refusal::Digest)
    );
    // The pinned trust does not know this instance's authority.
    assert!(Trust::pinned()?.verify(artifact, &bundle).is_err());
    let wrong_ref = sigstore
        .sign(
            artifact,
            Signer {
                git_ref: Some("refs/heads/feature"),
                ..signer
            },
        )
        .map_err(|e| e.to_string())?;
    assert_eq!(
        Trust::new(&root, PIPELINE)?.verify(artifact, &wrong_ref),
        Err(Refusal::Identity)
    );
    Ok(())
}
