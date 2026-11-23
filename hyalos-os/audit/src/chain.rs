//! The audit of the operator's public chain: every entry the log holds under the operator's key
//! must be a link of one unbroken chain. It opens no private key and signs nothing.

use crate::failure::Failure;
use crate::files::{FileError, FileKind, Files};
use crate::record::{hex, sha, unhex};
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

macro_rules! refused {
    ($($arg:tt)*) => { Failure::refused(format!($($arg)*)) };
}

pub const REKOR: &str = "https://rekor.sigstore.dev/api/v1";
/// The public key of the log at `REKOR`, from Sigstore's trusted root
/// (`core/hyalos/tests/fixtures/marketplace-r10/trusted-root.json:107`). Its SHA-256 is the log ID.
pub const REKOR_KEY: &str = "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE2G2Y+2tabdTV5BcGiBIx0a9fAFwrkBbmLSGtks4L3qX6yYY0zufBnhC8Ur/iy55GhWP/9A/bY2LhC30M9+RYtw==";
fn openssl(args: &[&str]) -> Option<Vec<u8>> {
    let output = Command::new("openssl").args(args).output().ok()?;
    output.status.success().then_some(output.stdout)
}

/// The links of the chain in `dir`, from 00000 up to the first that is absent.
fn read_links(files: &dyn Files, dir: &Path) -> Result<Vec<Vec<u8>>, Failure> {
    let mut links = Vec::new();
    loop {
        let seq = links.len();
        match files.read(&dir.join(format!("{seq:05}.json")), FileKind::Link) {
            Ok(link) => links.push(link),
            Err(FileError::Missing) => return Ok(links),
            Err(FileError::TooLarge) => {
                return Err(refused!("link {seq}: it is over its size limit"));
            }
            Err(FileError::Unreadable) => {
                return Err(Failure::inconclusive(format!(
                    "link {seq}: it cannot be read"
                )));
            }
        }
    }
}

/// The bytes of one PEM public key. A file holding anything else, a private key included, is `None`.
pub fn public_der(text: &str) -> Option<Vec<u8>> {
    let inner = text
        .trim()
        .strip_prefix("-----BEGIN PUBLIC KEY-----")?
        .strip_suffix("-----END PUBLIC KEY-----")?;
    let plain: String = inner.split_whitespace().collect();
    B64.decode(plain).ok().filter(|der| !der.is_empty())
}

pub fn public_pem(der: &[u8]) -> String {
    let text = B64.encode(der);
    let lines: Vec<&str> = text
        .as_bytes()
        .chunks(64)
        .filter_map(|c| std::str::from_utf8(c).ok())
        .collect();
    format!(
        "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
        lines.join("\n")
    )
}

/// A private directory for the files `openssl` needs, removed when the check ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Option<Self> {
        let mut name = [0u8; 16];
        fs::File::open("/dev/urandom")
            .and_then(|mut device| std::io::Read::read_exact(&mut device, &mut name))
            .ok()?;
        let path = PathBuf::from(format!("/tmp/hyalos-audit-{}", hex(&name)));
        fs::DirBuilder::new().mode(0o700).create(&path).ok()?;
        Some(Self(path))
    }

    /// True only when `openssl` confirms the signature over the message under this public key.
    fn verifies(&self, key: &[u8], signature: &[u8], message: &[u8]) -> bool {
        let (k, s, m) = (
            self.0.join("key.pem"),
            self.0.join("signature"),
            self.0.join("message"),
        );
        let written = fs::write(&k, public_pem(key)).is_ok()
            && fs::write(&s, signature).is_ok()
            && fs::write(&m, message).is_ok();
        let (Some(k), Some(s), Some(m)) = (k.to_str(), s.to_str(), m.to_str()) else {
            return false;
        };
        written && openssl(&["dgst", "-sha256", "-verify", k, "-signature", s, m]).is_some()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn node(left: &[u8], right: &[u8]) -> Vec<u8> {
    Sha256::digest([&[1u8], left, right].concat()).to_vec()
}

/// The root that an entry's leaf and its proof give, by the rule of RFC 6962.
pub fn inclusion_root(leaf: Vec<u8>, index: u64, size: u64, hashes: &[Vec<u8>]) -> Option<Vec<u8>> {
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

/// Where the audit is told to find the log. A stand-in exists only in a `test-trust` build.
pub enum LogSource {
    Public,
    #[cfg(feature = "test-trust")]
    StandIn(String, String),
    /// A public-log client aimed at `base`, in tests of a log that cannot be reached. No setting
    /// selects it.
    #[cfg(feature = "test-trust")]
    PublicAt(String),
}

impl LogSource {
    /// A public-log client aimed at `base`, in tests. In a build without `test-trust` it is the
    /// real log, so nothing but that feature can redirect the audit.
    pub fn public_at(base: String) -> Self {
        #[cfg(feature = "test-trust")]
        return Self::PublicAt(base);
        #[cfg(not(feature = "test-trust"))]
        {
            let _ = base;
            Self::Public
        }
    }

    /// The log this process is configured for. Without `test-trust` this reads no setting.
    #[cfg(feature = "test-trust")]
    pub fn from_env() -> Result<Self, Failure> {
        match (
            std::env::var("HYALOS_TRUST_LOG_DIR"),
            std::env::var("HYALOS_TRUST_LOG_KEY"),
        ) {
            (Ok(dir), Ok(key)) => Ok(Self::StandIn(dir, key)),
            (Err(_), Err(_)) => Ok(Self::Public),
            _ => Err(
                "a stand-in log needs both HYALOS_TRUST_LOG_DIR and HYALOS_TRUST_LOG_KEY".into(),
            ),
        }
    }

    /// The log this process is configured for. Without `test-trust` this reads no setting.
    #[cfg(not(feature = "test-trust"))]
    pub fn from_env() -> Result<Self, Failure> {
        Ok(Self::Public)
    }
}

/// Where the list of entries under the operator's key comes from. It is the one answer taken from
/// the log without a proof, and the report names that as a limit.
enum Log {
    Public {
        base: String,
        agent: ureq::Agent,
    },
    /// A directory of stored replies, in tests. Its report is marked and claims nothing of the real log.
    #[cfg(feature = "test-trust")]
    StandIn(PathBuf),
}

/// What a passing audit found.
pub struct Audited {
    pub link: u64,
    pub links: usize,
    /// Runs of this task, as the intent links name the task.
    pub runs: usize,
    /// Runs of any task under this key. An operator can label a discarded run with another task,
    /// but cannot leave it out of this number.
    pub all_runs: usize,
    pub stand_in: bool,
    /// The hash of this record's intent link: the run every answer must name.
    pub intent: String,
    /// The times the log signed for this run's intent link and result link.
    pub window: (u64, u64),
    /// The version of this run's record and links.
    pub version: u32,
}

/// What a record says about itself, recomputed from the record: its commitment, its task and its
/// guest. The audit holds the chain to these rather than to the links' own labels.
pub struct Claim {
    pub commitment: String,
    pub task: String,
    pub guest: String,
    /// The record's version, which its links must carry.
    pub version: u32,
}

/// The audit's view of a chain: public keys and stored replies. It opens no private key and reads
/// nothing under the operator's home directory.
pub struct Auditor {
    dir: PathBuf,
    operator: Vec<u8>,
    log_key: Vec<u8>,
    log: Log,
    files: &'static dyn Files,
}

impl Auditor {
    /// Opens the audit of the chain in `dir`. A key file that cannot be read is inconclusive, and one
    /// that holds no public key is refused.
    pub fn new(
        dir: &str,
        operator_public: &str,
        source: LogSource,
        files: &'static dyn Files,
    ) -> Result<Self, Failure> {
        let read_key = |file: &str, name: &str| -> Result<Vec<u8>, Failure> {
            let bytes =
                files
                    .read(Path::new(file), FileKind::Key)
                    .map_err(|error| match error {
                        FileError::TooLarge => {
                            Failure::refused(format!("{name} is over its size limit"))
                        }
                        _ => Failure::inconclusive(format!("{name} is not a readable public key")),
                    })?;
            std::str::from_utf8(&bytes)
                .ok()
                .and_then(public_der)
                .ok_or_else(|| Failure::refused(format!("{name} is not a readable public key")))
        };
        let operator = read_key(operator_public, "the operator's public key")?;
        let (log_key, log) = match source {
            #[cfg(feature = "test-trust")]
            LogSource::StandIn(replies, key) => (
                read_key(&key, "the stand-in log's key")?,
                Log::StandIn(PathBuf::from(replies)),
            ),
            #[cfg(feature = "test-trust")]
            LogSource::PublicAt(base) => {
                let key = B64
                    .decode(REKOR_KEY)
                    .map_err(|_| "the pinned log key is unreadable")?;
                let agent = ureq::AgentBuilder::new()
                    .timeout(Duration::from_secs(2))
                    .build();
                (key, Log::Public { base, agent })
            }
            LogSource::Public => {
                let key = B64
                    .decode(REKOR_KEY)
                    .map_err(|_| "the pinned log key is unreadable")?;
                (
                    key,
                    Log::Public {
                        base: REKOR.to_string(),
                        agent: ureq::AgentBuilder::new()
                            .timeout(Duration::from_secs(30))
                            .build(),
                    },
                )
            }
        };
        Ok(Self {
            dir: PathBuf::from(dir),
            operator,
            log_key,
            log,
            files,
        })
    }

    /// Verifies one stored reply against its link, from the reply and the keys alone, and gives
    /// the time the log signed for it.
    fn check_entry(&self, scratch: &Scratch, seq: usize, link: &[u8]) -> Result<u64, Failure> {
        let fail = |what: &str| Failure::refused(format!("link {seq}: {what}"));
        let bytes = self
            .files
            .read(
                &self.dir.join(format!("{seq:05}.rekor.json")),
                FileKind::Link,
            )
            .map_err(|error| match error {
                FileError::Missing => fail("no stored log reply"),
                FileError::TooLarge => fail("the stored log reply is over its size limit"),
                FileError::Unreadable => Failure::inconclusive(format!(
                    "link {seq}: the stored log reply cannot be read"
                )),
            })?;
        let reply: Value = serde_json::from_slice(&bytes)
            .map_err(|_| fail("the stored log reply is unreadable"))?;
        let entry = reply
            .as_object()
            .filter(|o| o.len() == 1)
            .and_then(|o| o.values().next())
            .ok_or_else(|| fail("the stored log reply is unreadable"))?;

        let log_id = sha(&self.log_key);
        if entry["logID"] != json!(log_id) {
            return Err(fail("the reply is not from the log's key"));
        }
        let body_text = entry["body"]
            .as_str()
            .ok_or_else(|| fail("the stored log reply is unreadable"))?;
        let body_bytes = B64
            .decode(body_text)
            .map_err(|_| fail("the stored log reply is unreadable"))?;
        let body: Value = serde_json::from_slice(&body_bytes)
            .map_err(|_| fail("the stored log reply is unreadable"))?;
        let hash = &body["spec"]["data"]["hash"];
        if body["kind"] != "hashedrekord"
            || hash["algorithm"] != "sha256"
            || hash["value"] != json!(sha(link))
        {
            return Err(fail("the reply is for another link"));
        }

        let signed_time = || -> Option<bool> {
            let time = entry["integratedTime"].as_u64()?;
            let index = entry["logIndex"].as_u64()?;
            let signature = B64
                .decode(entry["verification"]["signedEntryTimestamp"].as_str()?)
                .ok()?;
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
            let [origin, size, root] = lines.as_slice() else {
                return Some(false);
            };
            let hint = unhex(&log_id)?;
            let signed = signatures
                .lines()
                .filter_map(|line| {
                    B64.decode(line.strip_prefix("— ")?.rsplit(' ').next()?)
                        .ok()
                })
                .find(|s| s.len() > 4 && s[..4] == hint[..4])?;
            if origin.is_empty()
                || !scratch.verifies(&self.log_key, &signed[4..], format!("{note}\n").as_bytes())
            {
                return Some(false);
            }
            let tree = proof["treeSize"].as_u64()?;
            let root = B64.decode(root).ok()?;
            if size.parse::<u64>().ok()? != tree
                || proof["rootHash"].as_str().and_then(unhex)? != root
            {
                return Some(false);
            }
            let hashes = proof["hashes"]
                .as_array()?
                .iter()
                .map(|h| h.as_str().and_then(unhex))
                .collect::<Option<Vec<_>>>()?;
            let leaf = Sha256::digest([&[0u8], body_bytes.as_slice()].concat()).to_vec();
            Some(inclusion_root(leaf, proof["logIndex"].as_u64()?, tree, &hashes)? == root)
        };
        if included() != Some(true) {
            return Err(fail("the inclusion proof does not verify"));
        }

        let key = &body["spec"]["signature"]["publicKey"]["content"];
        let signer = key
            .as_str()
            .and_then(|k| B64.decode(k).ok())
            .and_then(|pem| String::from_utf8(pem).ok())
            .and_then(|pem| public_der(&pem));
        if signer.as_deref() != Some(self.operator.as_slice()) {
            return Err(fail("the link is not signed by the operator's key"));
        }
        let signature = body["spec"]["signature"]["content"]
            .as_str()
            .and_then(|s| B64.decode(s).ok());
        if !signature.is_some_and(|s| scratch.verifies(&self.operator, &s, link)) {
            return Err(fail("the link's signature does not verify"));
        }
        entry["integratedTime"]
            .as_u64()
            .ok_or_else(|| fail("the signed time does not verify"))
    }

    #[cfg(feature = "test-trust")]
    fn is_stand_in(&self) -> bool {
        matches!(self.log, Log::StandIn(_))
    }

    #[cfg(not(feature = "test-trust"))]
    fn is_stand_in(&self) -> bool {
        false
    }

    /// The hashes the log lists under the operator's key, in the log's order.
    fn logged(&self) -> Result<Vec<String>, Failure> {
        #[cfg(feature = "test-trust")]
        let unread = || {
            Failure::inconclusive(
                "the log's list of entries under the operator's key could not be read",
            )
        };
        let listed = |entry: &Value| -> Option<(u64, String, Vec<u8>)> {
            let body: Value =
                serde_json::from_slice(&B64.decode(entry["body"].as_str()?).ok()?).ok()?;
            let key = B64
                .decode(body["spec"]["signature"]["publicKey"]["content"].as_str()?)
                .ok()?;
            let key = public_der(std::str::from_utf8(&key).ok()?)?;
            Some((
                entry["logIndex"].as_u64()?,
                body["spec"]["data"]["hash"]["value"].as_str()?.to_string(),
                key,
            ))
        };
        let mut entries = Vec::new();
        match &self.log {
            #[cfg(feature = "test-trust")]
            Log::StandIn(dir) => {
                for file in fs::read_dir(dir).map_err(|_| unread())? {
                    let path = file.map_err(|_| unread())?.path();
                    if !path.to_string_lossy().ends_with(".rekor.json") {
                        continue;
                    }
                    let reply: Value = fs::read(&path)
                        .ok()
                        .and_then(|b| serde_json::from_slice(&b).ok())
                        .ok_or_else(unread)?;
                    let entry = reply
                        .as_object()
                        .and_then(|o| o.values().next())
                        .ok_or_else(unread)?;
                    entries.push(listed(entry).ok_or_else(unread)?);
                }
            }
            Log::Public { base, agent } => {
                let refused = || {
                    Failure::inconclusive(
                        "the public log could not be asked which entries it holds under the operator's key",
                    )
                };
                let ask =
                    |response: Result<ureq::Response, ureq::Error>| -> Result<Value, Failure> {
                        serde_json::from_str(
                            &response
                                .map_err(|_| refused())?
                                .into_string()
                                .map_err(|_| refused())?,
                        )
                        .map_err(|_| refused())
                    };
                let query = json!({"publicKey": {"format": "x509", "content": B64.encode(public_pem(&self.operator))}});
                let ids = ask(agent
                    .post(&format!("{base}/index/retrieve"))
                    .set("content-type", "application/json")
                    .send_string(&query.to_string()))?;
                for id in ids.as_array().ok_or_else(refused)? {
                    let reply = ask(agent
                        .get(&format!(
                            "{base}/log/entries/{}",
                            id.as_str().ok_or_else(refused)?
                        ))
                        .call())?;
                    let entry = reply
                        .as_object()
                        .and_then(|o| o.values().next())
                        .ok_or_else(refused)?;
                    let listed = listed(entry).ok_or_else(refused)?;
                    if listed.2 != self.operator {
                        return Err("the public log listed an entry under another key".into());
                    }
                    entries.push(listed);
                }
            }
        }
        let mut mine: Vec<(u64, String)> = entries
            .into_iter()
            .filter(|(_, _, key)| *key == self.operator)
            .map(|(index, hash, _)| (index, hash))
            .collect();
        mine.sort();
        Ok(mine.into_iter().map(|(_, hash)| hash).collect())
    }

    /// Checks one record against the chain and the log. Any failure names the first check it broke.
    pub fn audit(&self, claim: &Claim) -> Result<Audited, Failure> {
        let links = read_links(self.files, &self.dir)?;
        let scratch = Scratch::new().ok_or_else(|| {
            Failure::inconclusive("no private temporary directory for the signature checks")
        })?;
        let mut times = Vec::with_capacity(links.len());
        for (seq, link) in links.iter().enumerate() {
            times.push(self.check_entry(&scratch, seq, link)?);
        }
        let parsed: Vec<Value> = links
            .iter()
            .map(|l| serde_json::from_slice(l).unwrap_or_default())
            .collect();
        for (seq, link) in parsed.iter().enumerate() {
            let prev = seq
                .checked_sub(1)
                .map(|p| sha(&links[p]))
                .unwrap_or_default();
            if link["seq"] != json!(seq) || link["prev"] != json!(prev) {
                return Err(refused!("the chain is broken at link {seq}"));
            }
        }
        // A link of a version this program does not know could mean anything, so none is read past.
        for (seq, link) in parsed.iter().enumerate() {
            if !matches!(link["v"].as_u64(), Some(1..=4)) {
                return Err(refused!("link {seq}: its version is not known"));
            }
        }
        let logged = self.logged()?;
        let mine: Vec<String> = links.iter().map(|l| sha(l)).collect();
        if logged != mine {
            return Err(refused!(
                "the log lists {} entries under this key and the chain shown has {}: they must be the same list",
                logged.len(),
                mine.len()
            ));
        }
        // Every link is an intent or a result, and every result answers one earlier intent, named
        // by its exact hash. A link that fits neither could hide a discarded run.
        let mut intents: Vec<(String, bool)> = Vec::new();
        for (seq, link) in parsed.iter().enumerate() {
            match link["kind"].as_str() {
                Some("intent") => intents.push((sha(&links[seq]), false)),
                Some("result") => {
                    let named = link["intent"].as_str();
                    let Some(intent) = intents
                        .iter_mut()
                        .find(|(hash, _)| Some(hash.as_str()) == named)
                    else {
                        return Err(refused!("link {seq}: the result names no earlier intent"));
                    };
                    if intent.1 {
                        return Err(refused!(
                            "link {seq}: a second result was logged for one run"
                        ));
                    }
                    intent.1 = true;
                    let at = links
                        .iter()
                        .position(|l| Some(sha(l).as_str()) == named)
                        .ok_or("the chain is unreadable")?;
                    if link["v"] != parsed[at]["v"] {
                        return Err(refused!("link {seq}: its version is not its intent's"));
                    }
                }
                _ => return Err(refused!("link {seq}: it is neither an intent nor a result")),
            }
        }
        let holders: Vec<&Value> = parsed
            .iter()
            .filter(|l| l["kind"] == "result" && l["transcript"] == json!(claim.commitment))
            .collect();
        let result = match holders.as_slice() {
            [] => return Err("this record is not in the chain".into()),
            [result] => *result,
            many => return Err(refused!("{} results claim this record", many.len())),
        };
        if result["outcome"] != "ok" {
            return Err("this record's result link says the run was refused".into());
        }
        let intent = links
            .iter()
            .position(|l| json!(sha(l)) == result["intent"])
            .ok_or("the result names an intent that is not in the chain")?;
        // A run hash belongs to one log key, so no intent of version 2 or later may name another
        // (ADR-010).
        let fingerprint = sha(&self.operator);
        if let Some(seq) = parsed.iter().position(|l| {
            l["kind"] == "intent"
                && matches!(l["v"].as_u64(), Some(2..=4))
                && l["operator"] != json!(fingerprint)
        }) {
            return Err(refused!(
                "link {seq}: the intent names another operator key"
            ));
        }
        if parsed[intent]["v"].as_u64() != Some(u64::from(claim.version)) {
            return Err(refused!(
                "link {intent}: the run's links are not this record's version"
            ));
        }
        if parsed[intent]["task"] != json!(claim.task)
            || parsed[intent]["guest_sha256"] != json!(claim.guest)
        {
            return Err(refused!(
                "link {intent}: the intent names another task than this record's"
            ));
        }
        let same_task = |l: &&Value| l["kind"] == "intent" && l["task"] == parsed[intent]["task"];
        let at = result["seq"].as_u64().ok_or("the chain is unreadable")?;
        let result_time = *usize::try_from(at)
            .ok()
            .and_then(|at| times.get(at))
            .ok_or("the chain is unreadable")?;
        Ok(Audited {
            link: at,
            links: links.len(),
            runs: parsed.iter().filter(same_task).count(),
            all_runs: intents.len(),
            stand_in: self.is_stand_in(),
            intent: sha(&links[intent]),
            window: (times[intent], result_time),
            version: claim.version,
        })
    }
}
