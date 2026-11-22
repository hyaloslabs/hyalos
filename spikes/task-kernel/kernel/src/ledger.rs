//! The operator's public chain of commitments. Every run adds two links: an intent before the run
//! (so a run cannot be repeated until the model says something convenient without it showing) and
//! a result after it. Each link names the one before it, and each link's hash is signed and entered
//! in a public transparency log (Sigstore's Rekor). The rule an auditor applies: every entry the log
//! holds under the operator's key must be a link of one unbroken chain. A second record for a task,
//! a hidden fork or an unlogged record all break that rule in a way anyone can see.
//!
//! Only salted hashes leave the machine. The log is public and permanent.

use crate::{Reason, sha};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const REKOR: &str = "https://rekor.sigstore.dev/api/v1";
/// The public key of the log at `REKOR`, from Sigstore's trusted root
/// (`core/hyalos/tests/fixtures/marketplace-r10/trusted-root.json:107`). Its SHA-256 is the log ID.
const REKOR_KEY: &str = "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE2G2Y+2tabdTV5BcGiBIx0a9fAFwrkBbmLSGtks4L3qX6yYY0zufBnhC8Ur/iy55GhWP/9A/bY2LhC30M9+RYtw==";
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

/// The bytes of one PEM public key. A file holding anything else, a private key included, is `None`.
fn public_der(text: &str) -> Option<Vec<u8>> {
    let inner = text.trim().strip_prefix("-----BEGIN PUBLIC KEY-----")?.strip_suffix("-----END PUBLIC KEY-----")?;
    let plain: String = inner.split_whitespace().collect();
    B64.decode(plain).ok().filter(|der| !der.is_empty())
}

fn public_pem(der: &[u8]) -> String {
    let text = B64.encode(der);
    let lines: Vec<&str> = text.as_bytes().chunks(64).filter_map(|c| std::str::from_utf8(c).ok()).collect();
    format!("-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n", lines.join("\n"))
}

/// A private directory for the files `openssl` needs, removed when the check ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Option<Self> {
        let mut name = [0u8; 16];
        fs::File::open("/dev/urandom").and_then(|mut device| std::io::Read::read_exact(&mut device, &mut name)).ok()?;
        let path = PathBuf::from(format!("/tmp/hyalos-audit-{}", crate::hex(&name)));
        fs::DirBuilder::new().mode(0o700).create(&path).ok()?;
        Some(Self(path))
    }

    /// True only when `openssl` confirms the signature over the message under this public key.
    fn verifies(&self, key: &[u8], signature: &[u8], message: &[u8]) -> bool {
        let (k, s, m) = (self.0.join("key.pem"), self.0.join("signature"), self.0.join("message"));
        let written = fs::write(&k, public_pem(key)).is_ok() && fs::write(&s, signature).is_ok() && fs::write(&m, message).is_ok();
        let (Some(k), Some(s), Some(m)) = (k.to_str(), s.to_str(), m.to_str()) else { return false };
        written && openssl(&["dgst", "-sha256", "-verify", k, "-signature", s, m]).is_some()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn node(left: &[u8], right: &[u8]) -> Vec<u8> {
    Sha256::digest([&[1u8], left, right].concat()).to_vec()
}

/// The root that an entry's leaf and its proof give, by the rule of RFC 6962.
fn inclusion_root(leaf: Vec<u8>, index: u64, size: u64, hashes: &[Vec<u8>]) -> Option<Vec<u8>> {
    if index >= size {
        return None;
    }
    let (mut at, mut last, mut root) = (index, size - 1, leaf);
    for hash in hashes {
        if last == 0 || hash.len() != 32 {
            return None;
        }
        if at & 1 == 1 || at == last {
            root = node(hash, &root);
            if at & 1 == 0 {
                while at != 0 && at & 1 == 0 {
                    at >>= 1;
                    last >>= 1;
                }
            }
        } else {
            root = node(&root, hash);
        }
        at >>= 1;
        last >>= 1;
    }
    (last == 0).then_some(root)
}

/// Where the list of entries under the operator's key comes from. It is the one answer taken from
/// the log without a proof, and the report names that as a limit.
enum Log {
    Public { base: String, agent: ureq::Agent },
    /// A directory of stored replies, in tests. Its report is marked and claims nothing of the real log.
    StandIn(PathBuf),
}

/// What a passing audit found.
pub(crate) struct Audited {
    pub(crate) link: u64,
    pub(crate) links: usize,
    pub(crate) runs: usize,
    pub(crate) stand_in: bool,
}

/// The audit's view of a chain: public keys and stored replies. It opens no private key and reads
/// nothing under the operator's home directory.
pub(crate) struct Auditor {
    dir: PathBuf,
    operator: Vec<u8>,
    log_key: Vec<u8>,
    log: Log,
}

impl Auditor {
    /// `stand_in` is a directory of replies and the file of the key that signed them.
    pub(crate) fn new(dir: &str, operator_public: &str, stand_in: Option<(String, String)>) -> Result<Self, String> {
        let read_key = |file: &str, name: &str| {
            fs::read_to_string(file).ok().and_then(|text| public_der(&text)).ok_or_else(|| format!("{name} is not a readable public key"))
        };
        let operator = read_key(operator_public, "the operator's public key")?;
        let (log_key, log) = match stand_in {
            Some((replies, key)) => (read_key(&key, "the stand-in log's key")?, Log::StandIn(PathBuf::from(replies))),
            None => {
                let key = B64.decode(REKOR_KEY).map_err(|_| "the pinned log key is unreadable")?;
                (key, Log::Public { base: REKOR.to_string(), agent: ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).build() })
            }
        };
        Ok(Self { dir: PathBuf::from(dir), operator, log_key, log })
    }

    /// Verifies one stored reply against its link, from the reply and the keys alone.
    fn check_entry(&self, scratch: &Scratch, seq: usize, link: &[u8]) -> Result<(), String> {
        let fail = |what: &str| format!("link {seq}: {what}");
        let bytes = fs::read(self.dir.join(format!("{seq:05}.rekor.json"))).map_err(|_| fail("no stored log reply"))?;
        let reply: Value = serde_json::from_slice(&bytes).map_err(|_| fail("the stored log reply is unreadable"))?;
        let entry = reply.as_object().filter(|o| o.len() == 1).and_then(|o| o.values().next()).ok_or_else(|| fail("the stored log reply is unreadable"))?;

        let log_id = sha(&self.log_key);
        if entry["logID"] != json!(log_id) {
            return Err(fail("the reply is not from the log's key"));
        }
        let body_text = entry["body"].as_str().ok_or_else(|| fail("the stored log reply is unreadable"))?;
        let body_bytes = B64.decode(body_text).map_err(|_| fail("the stored log reply is unreadable"))?;
        let body: Value = serde_json::from_slice(&body_bytes).map_err(|_| fail("the stored log reply is unreadable"))?;
        let hash = &body["spec"]["data"]["hash"];
        if body["kind"] != "hashedrekord" || hash["algorithm"] != "sha256" || hash["value"] != json!(sha(link)) {
            return Err(fail("the reply is for another link"));
        }

        let signed_time = || -> Option<bool> {
            let time = entry["integratedTime"].as_u64()?;
            let index = entry["logIndex"].as_u64()?;
            let signature = B64.decode(entry["verification"]["signedEntryTimestamp"].as_str()?).ok()?;
            // Keys sorted, no spaces: the form the log signed.
            let text = format!(
                "{{\"body\":{},\"integratedTime\":{time},\"logID\":{},\"logIndex\":{index}}}",
                serde_json::to_string(body_text).ok()?,
                serde_json::to_string(&log_id).ok()?
            );
            Some(scratch.verifies(&self.log_key, &signature, text.as_bytes()))
        };
        if signed_time() != Some(true) {
            return Err(fail("the signed time does not verify"));
        }

        let proof = &entry["verification"]["inclusionProof"];
        let included = || -> Option<bool> {
            let (note, signatures) = proof["checkpoint"].as_str()?.split_once("\n\n")?;
            let lines: Vec<&str> = note.split('\n').collect();
            let [origin, size, root] = lines.as_slice() else { return Some(false) };
            let hint = crate::unhex(&log_id)?;
            let signed = signatures.lines().filter_map(|line| B64.decode(line.strip_prefix("— ")?.rsplit(' ').next()?).ok()).find(|s| s.len() > 4 && s[..4] == hint[..4])?;
            if origin.is_empty() || !scratch.verifies(&self.log_key, &signed[4..], format!("{note}\n").as_bytes()) {
                return Some(false);
            }
            let tree = proof["treeSize"].as_u64()?;
            let root = B64.decode(root).ok()?;
            if size.parse::<u64>().ok()? != tree || proof["rootHash"].as_str().and_then(crate::unhex)? != root {
                return Some(false);
            }
            let hashes = proof["hashes"].as_array()?.iter().map(|h| h.as_str().and_then(crate::unhex)).collect::<Option<Vec<_>>>()?;
            let leaf = Sha256::digest([&[0u8], body_bytes.as_slice()].concat()).to_vec();
            Some(inclusion_root(leaf, proof["logIndex"].as_u64()?, tree, &hashes)? == root)
        };
        if included() != Some(true) {
            return Err(fail("the inclusion proof does not verify"));
        }

        let key = &body["spec"]["signature"]["publicKey"]["content"];
        let signer = key.as_str().and_then(|k| B64.decode(k).ok()).and_then(|pem| String::from_utf8(pem).ok()).and_then(|pem| public_der(&pem));
        if signer.as_deref() != Some(self.operator.as_slice()) {
            return Err(fail("the link is not signed by the operator's key"));
        }
        let signature = body["spec"]["signature"]["content"].as_str().and_then(|s| B64.decode(s).ok());
        if !signature.is_some_and(|s| scratch.verifies(&self.operator, &s, link)) {
            return Err(fail("the link's signature does not verify"));
        }
        Ok(())
    }

    /// The hashes the log lists under the operator's key, in the log's order.
    fn logged(&self) -> Result<Vec<String>, String> {
        let unread = || "the log's list of entries under the operator's key could not be read".to_string();
        let listed = |entry: &Value| -> Option<(u64, String, Vec<u8>)> {
            let body: Value = serde_json::from_slice(&B64.decode(entry["body"].as_str()?).ok()?).ok()?;
            let key = B64.decode(body["spec"]["signature"]["publicKey"]["content"].as_str()?).ok()?;
            let key = public_der(std::str::from_utf8(&key).ok()?)?;
            Some((entry["logIndex"].as_u64()?, body["spec"]["data"]["hash"]["value"].as_str()?.to_string(), key))
        };
        let mut entries = Vec::new();
        match &self.log {
            Log::StandIn(dir) => {
                for file in fs::read_dir(dir).map_err(|_| unread())? {
                    let path = file.map_err(|_| unread())?.path();
                    if !path.to_string_lossy().ends_with(".rekor.json") {
                        continue;
                    }
                    let reply: Value = fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).ok_or_else(unread)?;
                    let entry = reply.as_object().and_then(|o| o.values().next()).ok_or_else(unread)?;
                    entries.push(listed(entry).ok_or_else(unread)?);
                }
            }
            Log::Public { base, agent } => {
                let refused = || "the public log could not be asked which entries it holds under the operator's key".to_string();
                let ask = |response: Result<ureq::Response, ureq::Error>| -> Result<Value, String> {
                    serde_json::from_str(&response.map_err(|_| refused())?.into_string().map_err(|_| refused())?).map_err(|_| refused())
                };
                let query = json!({"publicKey": {"format": "x509", "content": B64.encode(public_pem(&self.operator))}});
                let ids = ask(agent.post(&format!("{base}/index/retrieve")).set("content-type", "application/json").send_string(&query.to_string()))?;
                for id in ids.as_array().ok_or_else(refused)? {
                    let reply = ask(agent.get(&format!("{base}/log/entries/{}", id.as_str().ok_or_else(refused)?)).call())?;
                    let entry = reply.as_object().and_then(|o| o.values().next()).ok_or_else(refused)?;
                    entries.push(listed(entry).ok_or_else(refused)?);
                }
            }
        }
        let mut mine: Vec<(u64, String)> = entries.into_iter().filter(|(_, _, key)| *key == self.operator).map(|(index, hash, _)| (index, hash)).collect();
        mine.sort();
        Ok(mine.into_iter().map(|(_, hash)| hash).collect())
    }

    /// Checks one record against the chain and the log. Any failure names the first check it broke.
    pub(crate) fn audit(&self, commitment: &str) -> Result<Audited, String> {
        let links = read_links(&self.dir);
        let scratch = Scratch::new().ok_or("no private temporary directory for the signature checks")?;
        for (seq, link) in links.iter().enumerate() {
            self.check_entry(&scratch, seq, link)?;
        }
        let parsed: Vec<Value> = links.iter().map(|l| serde_json::from_slice(l).unwrap_or_default()).collect();
        for (seq, link) in parsed.iter().enumerate() {
            let prev = seq.checked_sub(1).map(|p| sha(&links[p])).unwrap_or_default();
            if link["seq"] != json!(seq) || link["prev"] != json!(prev) {
                return Err(format!("the chain is broken at link {seq}"));
            }
        }
        let logged = self.logged()?;
        let mine: Vec<String> = links.iter().map(|l| sha(l)).collect();
        if logged != mine {
            return Err(format!(
                "the log lists {} entries under this key and the chain shown has {}: they must be the same list",
                logged.len(),
                mine.len()
            ));
        }
        let results: Vec<&Value> = parsed.iter().filter(|l| l["kind"] == "result").collect();
        let Some(result) = results.iter().find(|l| l["transcript"] == json!(commitment)) else {
            return Err("this record is not in the chain".into());
        };
        let rivals = results.iter().filter(|l| l["intent"] == result["intent"]).count();
        if rivals != 1 {
            return Err(format!("{rivals} results were logged for this one run"));
        }
        let Some(intent) = links.iter().position(|l| json!(sha(l)) == result["intent"]) else {
            return Err("the result names an intent that is not in the chain".into());
        };
        let same_task = |l: &&Value| l["kind"] == "intent" && l["task"] == parsed[intent]["task"];
        Ok(Audited {
            link: result["seq"].as_u64().ok_or("the chain is unreadable")?,
            links: links.len(),
            runs: parsed.iter().filter(same_task).count(),
            stand_in: matches!(self.log, Log::StandIn(_)),
        })
    }
}

#[cfg(test)]
#[path = "audit_tests.rs"]
mod audit_tests;
