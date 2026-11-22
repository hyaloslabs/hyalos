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
use std::{fs, path::PathBuf, process::Command, time::Duration};

const REKOR: &str = "https://rekor.sigstore.dev/api/v1";
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
        (0..).map_while(|seq| fs::read(self.dir.join(format!("{seq:05}.json"))).ok()).collect()
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

    /// What the public log holds under the operator's key: each entry's hash, in the log's order.
    fn logged(&self) -> Result<Vec<String>, String> {
        let query = json!({"publicKey": {"format": "x509", "content": self.public_key().ok_or("no key")?}});
        let ask = |request: ureq::Request, body: Option<String>| -> Result<Value, String> {
            let response = match body {
                Some(body) => request.set("content-type", "application/json").send_string(&body),
                None => request.call(),
            };
            serde_json::from_str(&response.map_err(|_| "log unreachable")?.into_string().map_err(|_| "log unreadable")?)
                .map_err(|_| "log unreadable".to_string())
        };
        let ids = ask(self.agent.post(&format!("{REKOR}/index/retrieve")), Some(query.to_string()))?;
        let mut entries = Vec::new();
        for id in ids.as_array().ok_or("log unreadable")? {
            let entry = ask(self.agent.get(&format!("{REKOR}/log/entries/{}", id.as_str().unwrap_or_default())), None)?;
            let entry = entry.as_object().and_then(|o| o.values().next()).ok_or("log unreadable")?.clone();
            let body: Value = B64
                .decode(entry["body"].as_str().unwrap_or_default())
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .ok_or("log unreadable")?;
            let hash = body["spec"]["data"]["hash"]["value"].as_str().ok_or("log unreadable")?.to_string();
            entries.push((entry["logIndex"].as_u64().ok_or("log unreadable")?, hash));
        }
        entries.sort();
        Ok(entries.into_iter().map(|(_, hash)| hash).collect())
    }

    /// Checks one record against the chain and the public log. Any failure names what broke.
    pub(crate) fn audit(&self, commitment: &str) -> Result<String, String> {
        let links = self.links();
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
                "the public log holds {} entries under this key and the chain shown has {}: they must be the same list",
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
        Ok(format!(
            "in the chain at link {} of {}, all {} links are in the public log, one result for its run, {} run(s) of this task in total",
            result["seq"],
            links.len(),
            logged.len(),
            parsed.iter().filter(same_task).count()
        ))
    }
}
