//! The manifest's signed form is pinned: a change to a byte of it is a schema change (ADR-0004,
//! ADR-0031) and must be a deliberate one.

use std::collections::BTreeSet;

use hyalos_types::{
    Approval, Budget, JobId, MANIFEST_VERSION, Manifest, ManifestKey, OpenError, Role,
    SignedManifest, Timestamp,
};

type Res = Result<(), Box<dyn std::error::Error>>;

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

fn manifest() -> Manifest {
    Manifest {
        version: MANIFEST_VERSION,
        job: JobId::from_seq(12),
        role: Role::Actor,
        tainted: true,
        repo: "hyaloslabs/hyalos".into(),
        branch_prefix: "hyalos/".into(),
        // Given out of order and with a repeat: the document sorts and dedupes.
        actions: set(&["push", "open_pr", "push"]),
        egress_hosts: set(&["github.com", "api.github.com"]),
        tools: set(&["Bash", "Edit", "Read"]),
        data_flows: set(&["repo->model"]),
        budget: Budget { tokens: 200_000 },
        deadline: Timestamp::from_millis(1_800_000_000_000),
        approval: Some(Approval {
            head_sha: "0123456789abcdef0123456789abcdef01234567".into(),
            event: 77,
        }),
    }
}

const PINNED_PAYLOAD: &str = r#"{"version":1,"job":12,"role":"actor","tainted":true,"repo":"hyaloslabs/hyalos","branch_prefix":"hyalos/","actions":["open_pr","push"],"egress_hosts":["api.github.com","github.com"],"tools":["Bash","Edit","Read"],"data_flows":["repo->model"],"budget":{"tokens":200000},"deadline":1800000000000,"approval":{"head_sha":"0123456789abcdef0123456789abcdef01234567","event":77}}"#;

/// Computed independently (Python `cryptography`, Ed25519 over the tag, the version as 4 big-endian
/// bytes and the payload above), so it checks the code and not only itself.
const PINNED_SIGNATURE: &str = "b247298149739dd959e70e3d0bfefacff14235eed2fc8fbecb004d88c30839a120c1c6c4babbe637bf9fb6e75803363dd2d5c0aec8031654ecdb2f24d8261b05";

fn key() -> ManifestKey {
    ManifestKey::from_seed([7; 32])
}

#[test]
fn the_signed_payload_is_pinned() -> Res {
    let signed = key().sign(&manifest())?;
    assert_eq!(
        String::from_utf8(signed.payload().to_vec())?,
        PINNED_PAYLOAD
    );
    assert_eq!(signed.version(), 1);
    Ok(())
}

#[test]
fn the_signature_is_pinned_and_signing_is_deterministic() -> Res {
    let a = key().sign(&manifest())?;
    let b = key().sign(&manifest())?;
    assert_eq!(a, b);
    assert_eq!(hyalos_types::hex::to_hex(a.signature()), PINNED_SIGNATURE);
    Ok(())
}

#[test]
fn a_pinned_envelope_opens_and_gives_back_the_manifest() -> Res {
    let signed = key().sign(&manifest())?;
    assert_eq!(signed.open(&key().public())?, manifest());
    // Through the wire form too.
    let again = SignedManifest::from_bytes(&signed.to_bytes()?)?;
    assert_eq!(again, signed);
    assert_eq!(again.open(&key().public())?, manifest());
    Ok(())
}

#[test]
fn the_same_manifest_is_the_same_bytes_whatever_order_its_sets_were_built_in() -> Res {
    let mut other = manifest();
    other.tools = set(&["Read", "Bash", "Edit"]);
    other.egress_hosts = set(&["github.com", "api.github.com"]);
    assert_eq!(key().sign(&other)?, key().sign(&manifest())?);
    Ok(())
}

#[test]
fn a_manifest_in_another_version_is_not_signed() {
    for version in [0, MANIFEST_VERSION + 1, u32::MAX] {
        let mut m = manifest();
        m.version = version;
        assert!(key().sign(&m).is_err(), "{version}");
    }
}

#[test]
fn the_public_key_is_not_the_seed_and_debug_prints_neither_secret() {
    let k = key();
    let text = format!("{k:?}");
    assert!(text.contains(&k.public().to_hex()));
    assert!(!text.contains(&"07".repeat(8)));
}

#[test]
fn an_unknown_version_is_refused_before_the_signature_is_looked_at() -> Res {
    let signed = key().sign(&manifest())?;
    // The signature is garbage and the version unknown: the version is what is reported.
    for version in [0, 2, u32::MAX] {
        let relabelled = SignedManifest::from_parts(version, signed.payload().to_vec(), [0; 64]);
        assert_eq!(
            relabelled.open(&key().public()),
            Err(OpenError::UnknownVersion { found: version })
        );
    }
    Ok(())
}

#[test]
fn an_envelope_whose_hex_is_not_hex_is_refused() -> Res {
    let good = key().sign(&manifest())?;
    let signature = hyalos_types::hex::to_hex(good.signature());
    for manifest_hex in ["abc", "zz", "0", "é1", "00 "] {
        let wire =
            format!(r#"{{"version":1,"manifest":"{manifest_hex}","signature":"{signature}"}}"#);
        assert!(
            SignedManifest::from_bytes(wire.as_bytes()).is_err(),
            "{manifest_hex:?}"
        );
    }
    // The signature must be exactly 64 bytes.
    for short in [
        &signature[..126],
        &signature[..0],
        &format!("{signature}00")[..],
    ] {
        let wire = format!(
            r#"{{"version":1,"manifest":"{}","signature":"{short}"}}"#,
            hyalos_types::hex::to_hex(good.payload())
        );
        assert!(
            SignedManifest::from_bytes(wire.as_bytes()).is_err(),
            "{}",
            short.len()
        );
    }
    Ok(())
}
