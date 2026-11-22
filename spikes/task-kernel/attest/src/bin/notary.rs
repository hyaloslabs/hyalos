//! The notary: a program of its own, run by the customer on the customer's machine.
//!
//! It holds the signing key and a transport key, serves one enrolled operator over a pinned TLS
//! link, takes part in each TLS session with the provider without seeing its plaintext, and signs
//! what the server committed to, with the run it was told and that run's next sequence number.
//!
//! `notary --listen <address> --dir <directory> --operator-transport-public <hex>
//!  [--approved-server <name>] [--root <DER file>]... [--stop-on-stdin-close]`

use anyhow::{Context, Result, anyhow, bail, ensure};
use attest::{
    DEADLINE, MAX_RECV, MAX_SENT, RELEASE, RUN_SEQUENCE, SERVER, binding,
    link::{self, Identity},
    run_sequence,
};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tlsn::{
    Session,
    attestation::{
        Attestation, AttestationConfig, CryptoProvider, Extension,
        request::Request as AttestationRequest,
        signing::{Secp256k1Signer, Signer},
    },
    config::verifier::VerifierConfig,
    connection::{CertBinding, ConnectionInfo, TranscriptLength},
    transcript::{ContentType, TranscriptCommitment},
    verifier::{VerifierCommitStart, VerifierOutput},
    webpki::{CertificateDer, RootCertStore},
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Semaphore, oneshot},
    task::JoinHandle,
};
use tokio_rustls::TlsAcceptor;
use tokio_util::compat::TokioAsyncReadCompatExt;

/// How many connections may be in their handshake at once. When a new one arrives past this, the
/// oldest is closed, so idle strangers cannot keep the enrolled operator out.
const HANDSHAKES: usize = 16;
/// How long a peer has to prove its key. A stranger gets no more of the notary's time than this.
const HANDSHAKE: Duration = Duration::from_secs(5);
/// A link that drops this close to the deadline ended because the operator's own 30 seconds ran
/// out a moment before ours: its clock starts first.
const DEADLINE_MARGIN: Duration = Duration::from_secs(1);
/// The first line of the run sequences. A file without it was not written whole by this program.
const SEQUENCES_HEADER: &str = "hyalos.sequences/1";

/// A reason a session line may give for `refused`.
type Reason = &'static str;

/// How many attestations this notary has signed for each run. Saved before a number is used.
struct Sequences {
    path: PathBuf,
    next: BTreeMap<String, u64>,
}

impl Sequences {
    /// Anything in the file this program would not have written is a refusal to start. So is a
    /// missing file once a signing key exists: starting empty would reuse every run's numbers.
    fn load(path: PathBuf, key_exists: bool) -> Result<Self> {
        let unreadable = "the run sequences cannot be read";
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !key_exists => {
                let fresh = Self { path, next: BTreeMap::new() };
                fresh.save().map_err(|_| anyhow!("cannot keep the run sequences"))?;
                return Ok(fresh);
            }
            Err(_) => bail!("{unreadable}"),
        };
        let mut lines = text.lines();
        ensure!(text.ends_with('\n') && lines.next() == Some(SEQUENCES_HEADER), "{unreadable}");
        // The last line counts the runs, so a file cut short is refused and cannot lower a number.
        let end = lines.next_back().and_then(|line| line.strip_prefix("end ")).context(unreadable)?;
        let mut next = BTreeMap::new();
        for line in lines {
            let (run, written) = line.split_once(' ').context(unreadable)?;
            binding(run, 0).map_err(|_| anyhow!("{unreadable}"))?;
            let count: u64 = written.parse().map_err(|_| anyhow!("{unreadable}"))?;
            ensure!(count > 0 && count.to_string() == written, "{unreadable}");
            ensure!(next.insert(run.to_string(), count).is_none(), "{unreadable}");
        }
        ensure!(end == next.len().to_string(), "{unreadable}");
        Ok(Self { path, next })
    }

    /// Writes a new file, syncs it and renames it over the old one.
    fn save(&self) -> std::io::Result<()> {
        let fresh = self.path.with_extension("new");
        let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&fresh)?;
        writeln!(file, "{SEQUENCES_HEADER}")?;
        for (run, count) in &self.next {
            writeln!(file, "{run} {count}")?;
        }
        writeln!(file, "end {}", self.next.len())?;
        file.sync_all()?;
        fs::rename(&fresh, &self.path)?;
        fs::File::open(self.path.parent().unwrap_or(Path::new(".")))?.sync_all()
    }

    /// The run's next number, durably spent. On a failed save nothing was signed with it.
    fn spend(&mut self, run: &str) -> Option<u64> {
        let number = self.next.get(run).copied().unwrap_or(0);
        self.next.insert(run.to_string(), number.checked_add(1)?);
        if self.save().is_err() {
            if number == 0 {
                self.next.remove(run);
            } else {
                self.next.insert(run.to_string(), number);
            }
            return None;
        }
        Some(number)
    }
}

/// What a session line says. It holds no byte of the exchange and no key.
#[derive(Default)]
struct Line {
    server: Option<String>,
    sent: u64,
    received: u64,
}

struct Notary {
    key: [u8; 32],
    acceptor: TlsAcceptor,
    approved: String,
    roots: RootCertStore,
    sequences: Mutex<Sequences>,
    lines: Mutex<fs::File>,
    open: Arc<Semaphore>,
}

/// Ends the protocol's driver however the session ends.
struct Aborting<T>(JoinHandle<T>);

impl<T> Drop for Aborting<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl Notary {
    fn write_line(&self, line: &Line, outcome: &str) {
        let time = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs());
        let text = format!("{time} {} {} {} {outcome}\n", line.server.as_deref().unwrap_or("none"), line.sent, line.received);
        let mut file = self.lines.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if file.write_all(text.as_bytes()).is_err() {
            eprintln!("cannot write a session line");
        }
    }

    async fn serve(self: Arc<Self>, socket: TcpStream, evicted: oneshot::Receiver<()>) {
        let mut line = Line::default();
        let started = Instant::now();
        let outcome = match tokio::time::timeout(DEADLINE, self.session(socket, evicted, &mut line)).await {
            Ok(Ok((run, sequence))) => format!("attested {run} {sequence}"),
            Ok(Err("dropped")) if started.elapsed() >= DEADLINE - DEADLINE_MARGIN => "refused deadline".to_string(),
            Ok(Err(reason)) => format!("refused {reason}"),
            Err(_) => "refused deadline".to_string(),
        };
        self.write_line(&line, &outcome);
    }

    /// One session, through the states of `design.md` §4. Nothing is signed before the server's
    /// name is verified and the sequence number is saved.
    async fn session(&self, socket: TcpStream, evicted: oneshot::Receiver<()>, line: &mut Line) -> Result<(String, u64), Reason> {
        // A peer that is not the enrolled key ends here, before the protocol starts.
        let accepted = tokio::select! {
            accepted = tokio::time::timeout(HANDSHAKE, self.acceptor.accept(socket)) => accepted,
            _ = evicted => return Err("unenrolled"),
        };
        let mut tls = accepted.map_err(|_| "unenrolled")?.map_err(|_| "unenrolled")?;
        let _open = self.open.clone().try_acquire_owned().map_err(|_| "busy")?;

        let first = link::read_line(&mut tls, RELEASE.len() + 128).await.map_err(|_| "dropped")?;
        let (release, run) = first.split_once(' ').ok_or("dropped")?;
        if release != RELEASE {
            let _ = link::write_line(&mut tls, "release").await;
            let _ = tokio::io::AsyncWriteExt::shutdown(&mut tls).await;
            return Err("release");
        }
        binding(run, 0).map_err(|_| "dropped")?;
        let run = run.to_string();
        link::write_line(&mut tls, "ok").await.map_err(|_| "dropped")?;

        let session = Session::new(tls.compat());
        let (driver, mut handle) = session.split();
        let mut driver = Aborting(tokio::spawn(driver));
        let ran: Result<_, Reason> = async {
            let config = VerifierConfig::builder().root_store(self.roots.clone()).build().map_err(|_| "dropped")?;
            let verifier = match handle.new_verifier(config).map_err(|_| "dropped")?.commit().await.map_err(|_| "dropped")? {
                VerifierCommitStart::Mpc(verifier) => {
                    let asked = verifier.config();
                    if asked.max_sent_data() > MAX_SENT || asked.max_recv_data() > MAX_RECV {
                        let _ = verifier.reject(Some("limits")).await;
                        return Err("limits");
                    }
                    verifier.accept().await.map_err(|_| "dropped")?.run().await.map_err(|_| "dropped")?
                }
                VerifierCommitStart::Proxy(verifier) => {
                    let _ = verifier.reject(Some("expecting MPC-TLS")).await;
                    return Err("dropped");
                }
            };
            let (VerifierOutput { server_name, transcript_commitments, .. }, verifier) = match verifier.verify().await.map_err(|_| "dropped")?.accept().await {
                Ok(accepted) => accepted,
                Err(error) if error.is_io() => return Err("dropped"),
                Err(_) => {
                    // The certificate did not verify. The library has already said yes, so end its
                    // protocol cleanly and keep the link until the operator leaves: a broken link
                    // would read as an unreachable notary, and this is a refused provider.
                    // If the link itself died during the proof, the driver says so: that is a drop.
                    handle.close();
                    return match (&mut driver.0).await {
                        Ok(Ok(mut socket)) => {
                            let _ = link::read_bounded(&mut socket).await;
                            let _ = futures::io::AsyncWriteExt::close(&mut socket).await;
                            Err("server")
                        }
                        _ => Err("dropped"),
                    };
                }
            };
            let tls_transcript = verifier.tls_transcript().clone();
            verifier.close().await.map_err(|_| "dropped")?;
            Ok((server_name, transcript_commitments, tls_transcript))
        }
        .await;
        let (server_name, transcript_commitments, tls_transcript) = match ran {
            Ok(ran) => ran,
            Err(reason) => {
                // When the operator's session with the provider fails, the operator is still
                // deciding what to say. Keep the link until it leaves, or to the deadline, so the
                // close does not read to it as a notary that went away.
                if reason == "dropped" && !driver.0.is_finished() {
                    let _ = (&mut driver.0).await;
                }
                return Err(reason);
            }
        };
        let length = |records: &[tlsn::transcript::Record]| records.iter().filter(|r| matches!(r.typ, ContentType::ApplicationData)).map(|r| r.ciphertext.len()).sum::<usize>();
        let (sent, received) = (length(tls_transcript.sent()), length(tls_transcript.recv()));
        (line.sent, line.received) = (sent as u64, received as u64);
        line.server = server_name.as_ref().map(ToString::to_string);

        handle.close();
        let mut socket = (&mut driver.0).await.map_err(|_| "dropped")?.map_err(|_| "dropped")?;
        let request = link::read_bounded(&mut socket).await.map_err(|_| "dropped")?;

        let decided = if line.server.as_deref() != Some(self.approved.as_str()) {
            Err("server")
        } else if sent > MAX_SENT || received > MAX_RECV {
            Err("limits")
        } else {
            self.sign(&request, &tls_transcript, transcript_commitments, &run, sent as u32, received as u32)
        };
        match decided {
            Ok((sequence, attestation)) => {
                // The number is spent from here on, whether or not this reaches the operator.
                futures::io::AsyncWriteExt::write_all(&mut socket, &attestation).await.map_err(|_| "dropped")?;
                futures::io::AsyncWriteExt::close(&mut socket).await.map_err(|_| "dropped")?;
                Ok((run, sequence))
            }
            Err(reason) => {
                // Closing with no attestation is the refusal.
                let _ = futures::io::AsyncWriteExt::close(&mut socket).await;
                Err(reason)
            }
        }
    }

    /// Checks the request, saves the run's next number, and only then signs.
    fn sign(&self, request: &[u8], transcript: &tlsn::transcript::TlsTranscript, commitments: Vec<TranscriptCommitment>, run: &str, sent: u32, received: u32) -> Result<(u64, Vec<u8>), Reason> {
        let request: AttestationRequest = bincode::deserialize(request).map_err(|_| "dropped")?;
        let mut provider = CryptoProvider::default();
        provider.signer.set_signer(Box::new(Secp256k1Signer::new(&self.key).map_err(|_| "dropped")?));
        let mut config = AttestationConfig::builder();
        config.supported_signature_algs(Vec::from_iter(provider.signer.supported_algs()));
        let config = config.build().map_err(|_| "dropped")?;
        let CertBinding::V1_2(certificate) = transcript.certificate_binding() else {
            return Err("dropped");
        };
        let mut builder = Attestation::builder(&config).accept_request(request).map_err(|_| "dropped")?;
        let sequence = self.sequences.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).spend(run).ok_or("sequence")?;
        builder
            .connection_info(ConnectionInfo { time: transcript.time(), version: transcript.version(), transcript_length: TranscriptLength { sent, received } })
            .server_ephemeral_key(certificate.server_ephemeral_key.clone())
            .transcript_commitments(commitments)
            .extension(Extension { id: RUN_SEQUENCE.to_vec(), value: run_sequence(run, sequence) });
        let attestation = builder.build(&provider).map_err(|_| "dropped")?;
        Ok((sequence, bincode::serialize(&attestation).map_err(|_| "dropped")?))
    }
}

/// The notary's signing key: made once, readable by its owner only.
fn signing_key(path: &Path) -> Result<([u8; 32], String)> {
    let key: [u8; 32] = match link::read_private(path, 32)? {
        Some(bytes) => bytes.try_into().map_err(|_| anyhow!("cannot use the signing key file"))?,
        None => {
            let mut key = [0u8; 32];
            fs::File::open("/dev/urandom").and_then(|mut device| device.read_exact(&mut key)).context("no randomness")?;
            link::keep_private(path, &key)?;
            key
        }
    };
    let signer = Secp256k1Signer::new(&key).map_err(|_| anyhow!("cannot use the signing key file"))?;
    let public = hex::encode(signer.verifying_key().data);
    Ok((key, public))
}

struct Start {
    listen: String,
    dir: PathBuf,
    operator: String,
    approved: String,
    roots: Vec<PathBuf>,
    stop_on_stdin_close: bool,
}

fn arguments() -> Result<Start> {
    let usage = "usage: notary --listen <address> --dir <directory> --operator-transport-public <hex> [--approved-server <name>] [--root <DER file>]... [--stop-on-stdin-close]";
    let (mut listen, mut dir, mut operator, mut approved, mut roots, mut stop) = (None, None, None, None, Vec::new(), false);
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--listen" => listen = Some(args.next().context(usage)?),
            "--dir" => dir = Some(PathBuf::from(args.next().context(usage)?)),
            "--operator-transport-public" => operator = Some(args.next().context(usage)?),
            "--approved-server" => approved = Some(args.next().context(usage)?),
            "--root" => roots.push(PathBuf::from(args.next().context(usage)?)),
            "--stop-on-stdin-close" => stop = true,
            _ => bail!("{usage}"),
        }
    }
    Ok(Start {
        listen: listen.context(usage)?,
        dir: dir.context(usage)?,
        operator: operator.context(usage)?,
        approved: approved.unwrap_or_else(|| SERVER.to_string()),
        roots,
        stop_on_stdin_close: stop,
    })
}

async fn run() -> Result<()> {
    let start = arguments()?;
    fs::DirBuilder::new().recursive(true).mode(0o700).create(&start.dir).context("cannot use the notary's directory")?;
    // Two notaries on one directory would each hand out the same numbers. Held until exit.
    let lock = fs::OpenOptions::new().write(true).create(true).truncate(false).mode(0o600).open(start.dir.join("lock")).context("cannot lock the notary's directory")?;
    // SAFETY: `flock` takes a file descriptor that `lock` keeps open, and touches no memory.
    ensure!(unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&lock), libc::LOCK_EX | libc::LOCK_NB) } == 0, "another notary is using this directory");
    // The sequences are made before the key, so a key never exists without them.
    let signing = start.dir.join("signing.key");
    let sequences = Sequences::load(start.dir.join("sequences"), signing.symlink_metadata().is_ok())?;
    let (key, signing_public) = signing_key(&signing)?;
    let transport = Identity::make_or_load(&start.dir.join("transport.key"))?;
    let mut roots = RootCertStore::mozilla();
    for file in &start.roots {
        roots.roots.push(CertificateDer(fs::read(file).map_err(|_| anyhow!("cannot read a trusted root"))?));
    }
    let lines = fs::OpenOptions::new().append(true).create(true).mode(0o600).open(start.dir.join("sessions.log")).context("cannot keep the session lines")?;
    let acceptor = link::acceptor(&transport, &start.operator)?;
    let listener = TcpListener::bind(&start.listen).await.context("cannot listen on that address")?;

    println!("signing {signing_public}");
    println!("transport {}", transport.public);
    println!("listening {}", listener.local_addr()?);
    std::io::stdout().flush()?;
    if start.stop_on_stdin_close {
        std::thread::spawn(|| {
            let _ = std::io::stdin().read_to_end(&mut Vec::new());
            std::process::exit(0);
        });
    }

    let notary = Arc::new(Notary {
        key,
        acceptor,
        approved: start.approved,
        roots,
        sequences: Mutex::new(sequences),
        lines: Mutex::new(lines),
        open: Arc::new(Semaphore::new(1)),
    });
    let _lock = lock;
    let mut pending: VecDeque<oneshot::Sender<()>> = VecDeque::new();
    loop {
        let Ok((socket, _)) = listener.accept().await else { continue };
        // While a session is open, any other connection is closed at once.
        if notary.open.available_permits() == 0 {
            drop(socket);
            notary.write_line(&Line::default(), "refused busy");
            continue;
        }
        pending.retain(|handshake| !handshake.is_closed());
        if pending.len() >= HANDSHAKES {
            pending.pop_front();
        }
        let (evict, evicted) = oneshot::channel();
        pending.push_back(evict);
        let _ = socket.set_nodelay(true);
        tokio::spawn(notary.clone().serve(socket, evicted));
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    match run().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("refused: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
