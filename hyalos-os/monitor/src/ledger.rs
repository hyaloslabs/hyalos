//! The operator's public chain of commitments. Every run adds two links: an intent before the run
//! (so a run cannot be repeated until the model says something convenient without it showing) and
//! a result after it. Each link names the one before it, and each link's hash is signed and entered
//! in a public transparency log (Sigstore's Rekor). The rule an auditor applies: every entry the log
//! holds under the operator's key must be a link of one unbroken chain. A second record for a task,
//! a hidden fork or an unlogged record all break that rule in a way anyone can see.
//!
//! Only salted hashes leave the machine. The log is public and permanent.

#[allow(unused_imports)]
pub(crate) use hyalos_audit::chain::{Audited, Auditor, Claim, LogSource, REKOR, REKOR_KEY, inclusion_root, node, public_der, public_pem};
use crate::{Reason, sha};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use serde_json::{Value, json};
#[cfg(all(test, feature = "test-trust"))]
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const CALENDAR: &str = "https://alice.btc.calendar.opentimestamps.org/digest";
const OTS_HEADER: &[u8] = b"\x00OpenTimestamps\x00\x00Proof\x00\xbf\x89\xe2\xe8\x84\xe8\x92\x94\x01\x08";

pub(crate) struct Ledger {
    dir: PathBuf,
    key: PathBuf,
    agent: ureq::Agent,
}

fn openssl(args: &[&str]) -> Option<Vec<u8>> {
    let output = Command::new("openssl").args(args).output().ok()?;
    output.status.success().then_some(output.stdout)
}

impl Ledger {
    /// Opens the chain in `dir`, making the operator's signing key on first use (mode 600).
    pub(crate) fn open(dir: &str, home: &str) -> Option<Self> {
        let key = PathBuf::from(format!("{home}/.config/hyalos-spike/ledger-key.pem"));
        fs::create_dir_all(dir).ok()?;
        if !key.exists() {
            let path = key.to_str()?;
            openssl(&["genpkey", "-algorithm", "EC", "-pkeyopt", "ec_paramgen_curve:P-256", "-out", path])?;
            Command::new("chmod").args(["600", path]).status().ok()?;
        }
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).build();
        Some(Self { dir: PathBuf::from(dir), key, agent })
    }

    fn public_key(&self) -> Option<String> {
        Some(B64.encode(openssl(&["pkey", "-in", self.key.to_str()?, "-pubout"])?))
    }

    /// The SHA-256 of the operator public key's DER bytes, which a version 2 intent names.
    pub(crate) fn fingerprint(&self) -> Option<String> {
        let public = openssl(&["pkey", "-in", self.key.to_str()?, "-pubout"])?;
        Some(sha(&public_der(std::str::from_utf8(&public).ok()?)?))
    }

    /// The chain's links in order, as the exact bytes that were hashed and logged.
    pub(crate) fn links(&self) -> Vec<Vec<u8>> {
        read_links(&self.dir)
    }

    /// Adds a link: names the previous one, signs it, enters it in the public log, then keeps it.
    /// A link that could not be logged is not kept. Returns its number and hash.
    pub(crate) fn append(&self, mut link: Value) -> Result<(usize, String), Reason> {
        let fail = Reason::LedgerFailed;
        let links = self.links();
        link["seq"] = json!(links.len());
        link["prev"] = json!(links.last().map(|l| sha(l)).unwrap_or_default());
        let bytes = serde_json::to_vec(&link).map_err(|_| fail)?;
        let pending = self.dir.join("pending.json");
        fs::write(&pending, &bytes).map_err(|_| fail)?;
        let (key, file) = (self.key.to_str().ok_or(fail)?, pending.to_str().ok_or(fail)?);
        let signature = openssl(&["dgst", "-sha256", "-sign", key, file]).ok_or(fail)?;
        let entry = json!({"apiVersion": "0.0.1", "kind": "hashedrekord", "spec": {
            "data": {"hash": {"algorithm": "sha256", "value": sha(&bytes)}},
            "signature": {"content": B64.encode(signature), "publicKey": {"content": self.public_key().ok_or(fail)?}},
        }});
        let logged = self
            .agent
            .post(&format!("{REKOR}/log/entries"))
            .set("content-type", "application/json")
            .send_string(&entry.to_string())
            .map_err(|_| fail)?
            .into_string()
            .map_err(|_| fail)?;
        let seq = links.len();
        fs::write(self.dir.join(format!("{seq:05}.rekor.json")), logged).map_err(|_| fail)?;
        fs::rename(&pending, self.dir.join(format!("{seq:05}.json"))).map_err(|_| fail)?;
        Ok((seq, sha(&bytes)))
    }

    /// The time the log's stored reply gives for a link. The monitor uses it only to know how long
    /// to wait. The audit verifies every signed time itself.
    pub(crate) fn signed_time(&self, seq: usize) -> Option<u64> {
        let reply: Value = serde_json::from_slice(&fs::read(self.dir.join(format!("{seq:05}.rekor.json"))).ok()?).ok()?;
        reply.as_object().filter(|o| o.len() == 1)?.values().next()?["integratedTime"].as_u64()
    }

    /// Asks a Bitcoin timestamp calendar to anchor a link's hash. The proof is pending until the
    /// calendar's next Bitcoin transaction confirms, so it is kept, not verified, here.
    pub(crate) fn timestamp(&self, seq: usize) -> Option<()> {
        let digest = crate::unhex(&sha(self.links().get(seq)?))?;
        let mut reply = Vec::new();
        let response = self.agent.post(CALENDAR).set("accept", "application/vnd.opentimestamps.v1").send_bytes(&digest).ok()?;
        std::io::Read::read_to_end(&mut std::io::Read::take(response.into_reader(), 1 << 16), &mut reply).ok()?;
        fs::write(self.dir.join(format!("{seq:05}.ots")), [OTS_HEADER, &digest, &reply].concat()).ok()
    }
}

fn read_links(dir: &Path) -> Vec<Vec<u8>> {
    (0..).map_while(|seq| fs::read(dir.join(format!("{seq:05}.json"))).ok()).collect()
}

#[cfg(all(test, feature = "test-trust"))]
#[path = "audit_tests.rs"]
mod audit_tests;

#[cfg(all(test, feature = "test-trust"))]
#[path = "forgery_tests.rs"]
mod forgery_tests;

#[cfg(all(test, feature = "test-trust"))]
#[path = "fixture_tests.rs"]
mod fixture_tests;
