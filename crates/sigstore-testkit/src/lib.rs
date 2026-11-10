//! A Sigstore of our own for tests (S7.12, ADR-0093).
//!
//! An install verifies a release's Sigstore bundle against a trusted root and an identity. The
//! public-good instance cannot sign a test's own skill archive, so this makes a small instance whose
//! keys live only in the test process: a certificate authority (Fulcio's part), a
//! certificate-transparency log (for the certificate's SCT) and a Rekor v1 log (an entry with its
//! inclusion proof, signed checkpoint and signed entry timestamp). [`TestSigstore::root`] is its
//! `trusted_root.json`, and [`TestSigstore::sign`] returns a v0.3 bundle for an artifact, signed as
//! an identity, that the real verifier accepts.
//!
//! Nothing here is used by the binary: the binary's trust is pinned (`Trust::pinned`), and a test
//! hands this root to the install code in-process. The keys are made fresh for each instance.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::json;
use sha2::{Digest as _, Sha256};
use sigstore_verify::crypto::KeyPair;
use tls_codec::TlsByteVecU16;
use x509_cert::der::asn1::{BitString, Ia5String, OctetString, Utf8StringRef};
use x509_cert::der::oid::ObjectIdentifier;
use x509_cert::der::{Decode, Encode};
use x509_cert::ext::Extension;
use x509_cert::ext::pkix::name::GeneralName;
use x509_cert::ext::pkix::sct::{
    DigitallySigned, HashAlgorithm, LogId, SerializedSct, SignatureAlgorithm,
    SignatureAndHashAlgorithm, SignedCertificateTimestamp, SignedCertificateTimestampList, Version,
};
use x509_cert::ext::pkix::{
    BasicConstraints, ExtendedKeyUsage, KeyUsage, KeyUsages, SubjectAltName,
};
use x509_cert::name::Name;
use x509_cert::serial_number::SerialNumber;
use x509_cert::spki::{AlgorithmIdentifierOwned, SubjectPublicKeyInfoOwned};
use x509_cert::time::Validity;
use x509_cert::{Certificate, TbsCertificate};

pub type Error = Box<dyn std::error::Error + Send + Sync>;

const ECDSA_WITH_SHA256: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.4.3.2");
const KEY_USAGE: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.5.29.15");
const BASIC_CONSTRAINTS: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.5.29.19");
const SUBJECT_ALT_NAME: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.5.29.17");
const EXTENDED_KEY_USAGE: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.5.29.37");
const CODE_SIGNING: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.6.1.5.5.7.3.3");
const SCT_LIST: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.6.1.4.1.11129.2.4.2");
/// Fulcio's OIDC issuer, a DER UTF8String.
const FULCIO_ISSUER: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.6.1.4.1.57264.1.8");
/// Fulcio's source repository URI, a DER UTF8String.
const FULCIO_REPOSITORY: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.6.1.4.1.57264.1.12");
/// Fulcio's source repository ref, a DER UTF8String.
const FULCIO_REF: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.6.1.4.1.57264.1.14");

/// The log's name in its checkpoints.
const ORIGIN: &str = "rekor.test.invalid - 1";

/// A signer's identity as Fulcio would write it into a certificate.
#[derive(Debug, Clone, Copy)]
pub struct Signer<'a> {
    /// The SAN: a URI for a workflow, otherwise an email address.
    pub subject: &'a str,
    pub issuer: &'a str,
    pub repository: Option<&'a str>,
    pub git_ref: Option<&'a str>,
}

/// One test instance: its keys and the trusted root that lists them.
pub struct TestSigstore {
    ca: KeyPair,
    ca_cert: Vec<u8>,
    ct: KeyPair,
    rekor: KeyPair,
}

impl TestSigstore {
    pub fn new() -> Result<Self, Error> {
        let ca = KeyPair::generate_ecdsa_p256()?;
        let name: Name = "O=hyalos-test,CN=test-ca".parse()?;
        let now = now();
        let tbs = TbsCertificate {
            version: x509_cert::Version::V3,
            serial_number: SerialNumber::new(&[1])?,
            signature: algorithm(),
            issuer: name.clone(),
            validity: validity(now - 86_400, now + 86_400 * 365)?,
            subject: name,
            subject_public_key_info: spki(&ca)?,
            issuer_unique_id: None,
            subject_unique_id: None,
            extensions: Some(vec![
                extension(
                    BASIC_CONSTRAINTS,
                    true,
                    &BasicConstraints {
                        ca: true,
                        path_len_constraint: None,
                    },
                )?,
                extension(
                    KEY_USAGE,
                    true,
                    &KeyUsage(KeyUsages::KeyCertSign | KeyUsages::CRLSign),
                )?,
            ]),
        };
        let ca_cert = signed_by(tbs, &ca)?;
        Ok(Self {
            ca,
            ca_cert,
            ct: KeyPair::generate_ecdsa_p256()?,
            rekor: KeyPair::generate_ecdsa_p256()?,
        })
    }

    /// This instance's `trusted_root.json`.
    pub fn root(&self) -> Result<String, Error> {
        let since = json!({"start": "2020-01-01T00:00:00Z"});
        let log = |key: &KeyPair, url: &str| -> Result<serde_json::Value, Error> {
            let der = key.public_key_der()?;
            Ok(json!({
                "baseUrl": url,
                "hashAlgorithm": "SHA2_256",
                "publicKey": {
                    "rawBytes": STANDARD.encode(der.as_bytes()),
                    "keyDetails": "PKIX_ECDSA_P256_SHA_256",
                    "validFor": since,
                },
                "logId": {"keyId": STANDARD.encode(Sha256::digest(der.as_bytes()))},
            }))
        };
        let root = json!({
            "mediaType": "application/vnd.dev.sigstore.trustedroot+json;version=0.1",
            "tlogs": [log(&self.rekor, "https://rekor.test.invalid")?],
            "certificateAuthorities": [{
                "subject": {"organization": "hyalos-test", "commonName": "test-ca"},
                "uri": "https://fulcio.test.invalid",
                "certChain": {"certificates": [{"rawBytes": STANDARD.encode(&self.ca_cert)}]},
                "validFor": since,
            }],
            "ctlogs": [log(&self.ct, "https://ctfe.test.invalid")?],
            "timestampAuthorities": [],
        });
        Ok(serde_json::to_string(&root)?)
    }

    /// A v0.3 bundle for `artifact`, signed now as `signer` and logged in this instance's log.
    pub fn sign(&self, artifact: &[u8], signer: Signer<'_>) -> Result<Vec<u8>, Error> {
        let key = KeyPair::generate_ecdsa_p256()?;
        let now = now();
        let cert = self.leaf(&key, signer, now)?;
        let signature = key.sign(artifact)?;
        let signature = STANDARD.encode(signature.as_bytes());
        let digest = Sha256::digest(artifact);

        // The hashedrekord entry Rekor would have written for it, as one leaf of a log of one.
        let pem = format!(
            "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
            STANDARD.encode(&cert)
        );
        let body = serde_json::to_vec(&json!({
            "apiVersion": "0.0.1",
            "kind": "hashedrekord",
            "spec": {
                "data": {"hash": {"algorithm": "sha256", "value": hex(&digest)}},
                "signature": {
                    "content": signature,
                    "publicKey": {"content": STANDARD.encode(pem)},
                },
            },
        }))?;
        let mut leaf = Sha256::new();
        leaf.update([0u8]);
        leaf.update(&body);
        let root_hash = STANDARD.encode(leaf.finalize());
        let rekor_der = self.rekor.public_key_der()?;
        let log_id = Sha256::digest(rekor_der.as_bytes());
        let integrated = now - 30;

        let note = format!("{ORIGIN}\n1\n{root_hash}\n");
        let mut hinted = log_id[..4].to_vec();
        hinted.extend_from_slice(self.rekor.sign(note.as_bytes())?.as_bytes());
        let checkpoint = format!(
            "{note}\n— {} {}\n",
            "rekor.test.invalid",
            STANDARD.encode(hinted)
        );

        // The signed entry timestamp: the log's signature over the entry's canonical JSON.
        let promise = format!(
            r#"{{"body":"{}","integratedTime":{integrated},"logID":"{}","logIndex":0}}"#,
            STANDARD.encode(&body),
            hex(&log_id)
        );
        let promise = self.rekor.sign(promise.as_bytes())?;

        let bundle = json!({
            "mediaType": "application/vnd.dev.sigstore.bundle.v0.3+json",
            "verificationMaterial": {
                "certificate": {"rawBytes": STANDARD.encode(&cert)},
                "tlogEntries": [{
                    "logIndex": "0",
                    "logId": {"keyId": STANDARD.encode(log_id)},
                    "kindVersion": {"kind": "hashedrekord", "version": "0.0.1"},
                    "integratedTime": integrated.to_string(),
                    "inclusionPromise": {"signedEntryTimestamp": STANDARD.encode(promise.as_bytes())},
                    "inclusionProof": {
                        "logIndex": "0",
                        "rootHash": root_hash,
                        "treeSize": "1",
                        "hashes": [],
                        "checkpoint": {"envelope": checkpoint},
                    },
                    "canonicalizedBody": STANDARD.encode(&body),
                }],
            },
            "messageSignature": {
                "messageDigest": {"algorithm": "SHA2_256", "digest": STANDARD.encode(digest)},
                "signature": signature,
            },
        });
        Ok(serde_json::to_vec(&bundle)?)
    }

    /// A ten-minute signing certificate for `key`, as Fulcio issues one, with its SCT.
    fn leaf(&self, key: &KeyPair, signer: Signer<'_>, now: u64) -> Result<Vec<u8>, Error> {
        let issuer = Certificate::from_der(&self.ca_cert)?
            .tbs_certificate
            .subject;
        let san = if signer.subject.contains("://") {
            GeneralName::UniformResourceIdentifier(Ia5String::new(signer.subject)?)
        } else {
            GeneralName::Rfc822Name(Ia5String::new(signer.subject)?)
        };
        let mut extensions = vec![
            extension(
                KEY_USAGE,
                true,
                &KeyUsage(KeyUsages::DigitalSignature.into()),
            )?,
            extension(
                EXTENDED_KEY_USAGE,
                false,
                &ExtendedKeyUsage(vec![CODE_SIGNING]),
            )?,
            extension(SUBJECT_ALT_NAME, true, &SubjectAltName(vec![san]))?,
            extension(FULCIO_ISSUER, false, &Utf8StringRef::new(signer.issuer)?)?,
        ];
        if let Some(repository) = signer.repository {
            extensions.push(extension(
                FULCIO_REPOSITORY,
                false,
                &Utf8StringRef::new(repository)?,
            )?);
        }
        if let Some(git_ref) = signer.git_ref {
            extensions.push(extension(FULCIO_REF, false, &Utf8StringRef::new(git_ref)?)?);
        }
        let start = now - 60;
        let mut tbs = TbsCertificate {
            version: x509_cert::Version::V3,
            serial_number: SerialNumber::new(&now.to_be_bytes())?,
            signature: algorithm(),
            issuer,
            validity: validity(start, start + 600)?,
            subject: Name::default(),
            subject_public_key_info: spki(key)?,
            issuer_unique_id: None,
            subject_unique_id: None,
            extensions: Some(extensions.clone()),
        };

        // RFC 6962's precertificate entry: the TBS without the SCT, and the issuer's key hash.
        let precert = tbs.to_der()?;
        let issuer_key = Sha256::digest(self.ca.public_key_der()?.as_bytes());
        let timestamp = start * 1000;
        let mut signed = vec![0u8, 0u8];
        signed.extend_from_slice(&timestamp.to_be_bytes());
        signed.extend_from_slice(&1u16.to_be_bytes());
        signed.extend_from_slice(&issuer_key);
        let len = u32::try_from(precert.len())?.to_be_bytes();
        signed.extend_from_slice(&len[1..]);
        signed.extend_from_slice(&precert);
        signed.extend_from_slice(&0u16.to_be_bytes());
        let sct = SignedCertificateTimestamp {
            version: Version::V1,
            log_id: LogId {
                key_id: Sha256::digest(self.ct.public_key_der()?.as_bytes()).into(),
            },
            timestamp,
            extensions: TlsByteVecU16::from_slice(&[]),
            signature: DigitallySigned {
                algorithm: SignatureAndHashAlgorithm {
                    hash: HashAlgorithm::Sha256,
                    signature: SignatureAlgorithm::Ecdsa,
                },
                signature: TlsByteVecU16::from_slice(self.ct.sign(&signed)?.as_bytes()),
            },
        };
        let list = SignedCertificateTimestampList::new(&[SerializedSct::new(sct)?])
            .map_err(|e| format!("cannot encode the SCT list: {e:?}"))?;
        extensions.push(extension(SCT_LIST, false, &list)?);
        tbs.extensions = Some(extensions);
        signed_by(tbs, &self.ca)
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn algorithm() -> AlgorithmIdentifierOwned {
    AlgorithmIdentifierOwned {
        oid: ECDSA_WITH_SHA256,
        parameters: None,
    }
}

fn spki(key: &KeyPair) -> Result<SubjectPublicKeyInfoOwned, Error> {
    Ok(SubjectPublicKeyInfoOwned::from_der(
        key.public_key_der()?.as_bytes(),
    )?)
}

fn validity(from: u64, to: u64) -> Result<Validity, Error> {
    Ok(Validity {
        not_before: x509_cert::time::Time::try_from(UNIX_EPOCH + Duration::from_secs(from))?,
        not_after: x509_cert::time::Time::try_from(UNIX_EPOCH + Duration::from_secs(to))?,
    })
}

fn extension(
    oid: ObjectIdentifier,
    critical: bool,
    value: &impl Encode,
) -> Result<Extension, Error> {
    Ok(Extension {
        extn_id: oid,
        critical,
        extn_value: OctetString::new(value.to_der()?)?,
    })
}

/// `tbs` signed by `key`: the root's own, for the root.
fn signed_by(tbs: TbsCertificate, key: &KeyPair) -> Result<Vec<u8>, Error> {
    let signature = key.sign(&tbs.to_der()?)?;
    let cert = Certificate {
        tbs_certificate: tbs,
        signature_algorithm: algorithm(),
        signature: BitString::from_bytes(signature.as_bytes())?,
    };
    Ok(cert.to_der()?)
}
