//! The notary: a program of its own, run by the customer on the customer's machine.
//!
//! It holds the signing key and a transport key, serves one enrolled operator over a pinned TLS
//! link, takes part in each TLS session with the provider without seeing its plaintext, and signs
//! what the server committed to, with the run it was told and that run's next sequence number.
//!
//! `notary --listen <address> --dir <directory> --operator-transport-public <hex>
//!  [--approved-server <name>] [--root <DER file>]... [--stop-on-stdin-close]`
//!
//! `notary report --dir <directory> [--wall <file>] [--real-wall <file>]` reads what a serving
//! notary recorded and measures nothing itself. Each wall-time file holds one call's seconds per
//! line: `--wall` for the reported sessions, `--real-wall` from a run against the real provider.
//!
//! `notary authorize --dir <directory> --task <hash> --attempts <n> [--again]` is the customer's
//! decision: this exact task may have this many attempts. It prints the authorisation's identifier.
//!
//! `notary tickets --dir <directory>` prints each authorisation and what has been spent of it.

use anyhow::{Context, Result, anyhow, bail, ensure};
use attest::{
    CANARY, DEADLINE, MAX_RECV, MAX_SENT, RELEASE, RUN_SEQUENCE, SERVER, TICKET, Ticket, binding, canary,
    link::{self, Identity},
    run_sequence,
};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context as TaskContext, Poll},
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
    io::{AsyncRead, AsyncWrite, ReadBuf},
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
const SEQUENCES_HEADER: &str = "hyalos.sequences/2";
/// The first line of run sequences from before tickets. Rewritten as version 2 at start.
const SEQUENCES_V1_HEADER: &str = "hyalos.sequences/1";
/// The first line of an authorisation file.
const AUTHORISATION_HEADER: &str = "hyalos.authorisation/1";
/// Longer than any authorisation file this program writes.
const AUTHORISATION_LIMIT: u64 = 256;
/// The most attempts one authorisation may give.
const MAX_ATTEMPTS: u32 = 100;
/// The first line of the usage file.
const USAGE_HEADER: &str = "hyalos.usage/1";
/// Below this many attested sessions the report says its figures are not yet a measurement.
const MEASURED_FROM: u64 = 40;

/// A reason a session line may give for `refused`.
type Reason = &'static str;

/// A number in its one written form.
fn exact<T: std::str::FromStr + ToString>(text: &str) -> Option<T> {
    text.parse::<T>().ok().filter(|number| number.to_string() == text)
}

/// What one run holds: its session count and, once it has one, its authorisation and ticket.
#[derive(Clone)]
struct RunEntry {
    count: u64,
    ticket: Option<(String, u32)>,
}

/// Reads run sequences of either version, and gives the version. Anything else is a refusal.
fn parse_sequences(text: &str) -> Result<(u8, BTreeMap<String, RunEntry>)> {
    let unreadable = "the run sequences cannot be read";
    let mut lines = text.lines();
    ensure!(text.ends_with('\n'), "{unreadable}");
    let version = match lines.next() {
        Some(SEQUENCES_V1_HEADER) => 1,
        Some(SEQUENCES_HEADER) => 2,
        _ => bail!("{unreadable}"),
    };
    // The last line counts the runs, so a file cut short is refused and cannot lower a number.
    let end = lines.next_back().and_then(|line| line.strip_prefix("end ")).context(unreadable)?;
    let mut next = BTreeMap::new();
    let mut tickets: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for line in lines {
        let fields: Vec<&str> = line.split(' ').collect();
        let ticket = match (version, fields.as_slice()) {
            (1, [_, _]) | (2, [_, _, "-", "-"]) => None,
            (2, [_, _, authorisation, ticket]) => {
                binding(authorisation, 0).map_err(|_| anyhow!("{unreadable}"))?;
                let ticket: u32 = exact(ticket).context(unreadable)?;
                tickets.entry(authorisation.to_string()).or_default().push(ticket);
                Some((authorisation.to_string(), ticket))
            }
            _ => bail!("{unreadable}"),
        };
        binding(fields[0], 0).map_err(|_| anyhow!("{unreadable}"))?;
        let count: u64 = exact(fields[1]).context(unreadable)?;
        ensure!(count > 0, "{unreadable}");
        ensure!(next.insert(fields[0].to_string(), RunEntry { count, ticket }).is_none(), "{unreadable}");
    }
    ensure!(end == next.len().to_string(), "{unreadable}");
    // An authorisation's tickets are exactly 0 to k, so none was skipped or given twice.
    for held in tickets.values_mut() {
        held.sort_unstable();
        ensure!(held.iter().enumerate().all(|(at, ticket)| *ticket as usize == at), "{unreadable}");
    }
    Ok((version, next))
}

/// How many sessions this notary has opened for each run (ADR-014). Saved before a number is used.
struct Sequences {
    path: PathBuf,
    next: BTreeMap<String, RunEntry>,
}

impl Sequences {
    /// Anything in the file this program would not have written is a refusal to start. So is a
    /// missing file once a signing key exists: starting empty would reuse every run's numbers.
    /// A version 1 file is rewritten as version 2 here, before anything is served.
    fn load(path: PathBuf, key_exists: bool) -> Result<Self> {
        let unreadable = "the run sequences cannot be read";
        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !key_exists => {
                ensure!(authorisation_names(&dir)?.is_empty(), "{unreadable}");
                let fresh = Self { path, next: BTreeMap::new() };
                fresh.save().map_err(|_| anyhow!("cannot keep the run sequences"))?;
                return Ok(fresh);
            }
            Err(_) => bail!("{unreadable}"),
        };
        let (version, next) = parse_sequences(&text)?;
        let loaded = Self { path, next };
        if version == 1 {
            // A restored old file would hand back spent tickets (ADR-009).
            ensure!(authorisation_names(&dir)?.is_empty(), "the run sequences are version 1 and an authorisation exists");
            loaded.save().map_err(|_| anyhow!("cannot keep the run sequences"))?;
        }
        Ok(loaded)
    }

    /// Writes a new file, syncs it and renames it over the old one.
    fn save(&self) -> std::io::Result<()> {
        let fresh = self.path.with_extension("new");
        let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&fresh)?;
        writeln!(file, "{SEQUENCES_HEADER}")?;
        for (run, entry) in &self.next {
            match &entry.ticket {
                Some((authorisation, ticket)) => writeln!(file, "{run} {} {authorisation} {ticket}", entry.count)?,
                None => writeln!(file, "{run} {} - -", entry.count)?,
            }
        }
        writeln!(file, "end {}", self.next.len())?;
        file.sync_all()?;
        fs::rename(&fresh, &self.path)?;
        fs::File::open(self.path.parent().unwrap_or(Path::new(".")))?.sync_all()
    }

    /// The run's next number and its ticket, saved in one write before the operator can see any
    /// reply. In order: the run's own authorisation, the newest round, then the budget. On a failed
    /// save the run's whole entry is restored and nothing is signed with it.
    fn spend(&mut self, run: &str, authorisation: &str) -> Result<(u64, Ticket), Reason> {
        let dir = self.path.parent().unwrap_or(Path::new(".")).to_path_buf();
        binding(authorisation, 0).map_err(|_| "authorisation")?;
        let held = read_authorisation(&dir, authorisation).map_err(|_| "authorisation")?;
        let before = self.next.get(run).cloned();
        let ticket = match &before {
            Some(RunEntry { ticket: Some((own, ticket)), .. }) if own == authorisation => *ticket,
            // Another authorisation's run, or a run from before tickets.
            Some(_) => return Err("authorisation"),
            None => {
                let all = read_authorisations(&dir).map_err(|_| "authorisation")?;
                let newest = all.values().filter(|other| other.task == held.task).map(|other| other.round).max();
                if newest != Some(held.round) {
                    return Err("authorisation");
                }
                let used = self.next.values().filter(|entry| entry.ticket.as_ref().is_some_and(|(own, _)| own == authorisation)).count();
                let used = u32::try_from(used).map_err(|_| "spent")?;
                if used >= held.attempts {
                    return Err("spent");
                }
                used
            }
        };
        let number = before.as_ref().map_or(0, |entry| entry.count);
        let count = number.checked_add(1).ok_or("sequence")?;
        self.next.insert(run.to_string(), RunEntry { count, ticket: Some((authorisation.to_string(), ticket)) });
        if self.save().is_err() {
            match before {
                Some(entry) => self.next.insert(run.to_string(), entry),
                None => self.next.remove(run),
            };
            return Err("sequence");
        }
        Ok((number, Ticket { authorisation: authorisation.to_string(), task: held.task, budget: held.attempts, round: held.round, prior: held.prior, ticket }))
    }
}

/// The customer's decision for one task: how many attempts, and what came before it.
struct Authorisation {
    task: String,
    attempts: u32,
    round: u32,
    prior: u32,
}

impl Authorisation {
    fn text(&self) -> String {
        format!("{AUTHORISATION_HEADER}\ntask {}\nattempts {}\nround {}\nprior {}\nend\n", self.task, self.attempts, self.round, self.prior)
    }

    /// Only the exact text `text` writes is an authorisation.
    fn parse(text: &str) -> Option<Self> {
        let mut lines = text.strip_suffix('\n')?.split('\n');
        (lines.next()? == AUTHORISATION_HEADER).then_some(())?;
        let task = lines.next()?.strip_prefix("task ")?.to_string();
        binding(&task, 0).ok()?;
        let attempts: u32 = exact(lines.next()?.strip_prefix("attempts ")?)?;
        let round: u32 = exact(lines.next()?.strip_prefix("round ")?)?;
        let prior: u32 = exact(lines.next()?.strip_prefix("prior ")?)?;
        (lines.next()? == "end" && lines.next().is_none() && (1..=MAX_ATTEMPTS).contains(&attempts)).then_some(Self { task, attempts, round, prior })
    }
}

/// The names in the authorisations directory that are authorisations. A leftover temporary file
/// is not one. No directory means none.
fn authorisation_names(dir: &Path) -> Result<Vec<String>> {
    let entries = match fs::read_dir(dir.join("authorisations")) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => bail!("the authorisations cannot be read"),
    };
    let mut names = Vec::new();
    for entry in entries {
        let name = entry.map_err(|_| anyhow!("the authorisations cannot be read"))?.file_name();
        if let Some(name) = name.to_str().filter(|name| binding(name, 0).is_ok()) {
            names.push(name.to_string());
        }
    }
    Ok(names)
}

/// One authorisation, by a name already known to be 64 lowercase hex characters.
fn read_authorisation(dir: &Path, name: &str) -> Result<Authorisation> {
    let unreadable = "an authorisation cannot be read";
    let bytes = link::read_private(&dir.join("authorisations").join(name), AUTHORISATION_LIMIT).map_err(|_| anyhow!(unreadable))?.context(unreadable)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| anyhow!(unreadable))?;
    Authorisation::parse(text).context(unreadable)
}

/// Every authorisation. One that cannot be read refuses them all.
fn read_authorisations(dir: &Path) -> Result<BTreeMap<String, Authorisation>> {
    let mut all = BTreeMap::new();
    for name in authorisation_names(dir)? {
        let held = read_authorisation(dir, &name)?;
        all.insert(name, held);
    }
    Ok(all)
}

/// Held while authorisations are made, and while a starting notary reads its state. It is not the
/// serving lock, so the customer can authorise while the notary serves.
fn authorisations_lock(dir: &Path) -> Result<fs::File> {
    let lock = fs::OpenOptions::new().write(true).create(true).truncate(false).mode(0o600).open(dir.join("authorisations.lock")).context("cannot lock the authorisations")?;
    loop {
        // SAFETY: `flock` takes a file descriptor that `lock` keeps open, and touches no memory.
        if unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&lock), libc::LOCK_EX) } == 0 {
            return Ok(lock);
        }
        ensure!(std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted, "cannot lock the authorisations");
    }
}

fn authorize(mut args: impl Iterator<Item = String>) -> Result<()> {
    let usage = "usage: notary authorize --dir <directory> --task <hash> --attempts <n> [--again]";
    let (mut dir, mut task, mut attempts, mut again) = (None, None, None, false);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--dir" => ensure!(dir.replace(PathBuf::from(args.next().context(usage)?)).is_none(), "{usage}"),
            "--task" => ensure!(task.replace(args.next().context(usage)?).is_none(), "{usage}"),
            "--attempts" => ensure!(attempts.replace(args.next().context(usage)?).is_none(), "{usage}"),
            "--again" => again = true,
            _ => bail!("{usage}"),
        }
    }
    let (dir, task, attempts) = (dir.context(usage)?, task.context(usage)?, attempts.context(usage)?);
    binding(&task, 0).map_err(|_| anyhow!("the task is not 64 lowercase hex characters"))?;
    let attempts = exact::<u32>(&attempts).filter(|n| (1..=MAX_ATTEMPTS).contains(n)).context("the attempts must be a number from 1 to 100")?;
    // Only that the key exists: this command never reads it.
    ensure!(dir.join("signing.key").symlink_metadata().is_ok(), "this directory has no notary signing key");
    let _lock = authorisations_lock(&dir)?;
    let text = fs::read_to_string(dir.join("sequences")).map_err(|_| anyhow!("the run sequences cannot be read"))?;
    ensure!(parse_sequences(&text)?.0 == 2, "the run sequences are version 1: start the notary once to move them to version 2");
    let earlier: Vec<Authorisation> = read_authorisations(&dir)?.into_values().filter(|held| held.task == task).collect();
    ensure!(earlier.is_empty() || again, "this task is already authorised: --again makes a new round");
    let round = match earlier.iter().map(|held| held.round).max() {
        Some(highest) => highest.checked_add(1).context("the round would not fit")?,
        None => 0,
    };
    let prior = u32::try_from(earlier.iter().map(|held| u64::from(held.attempts)).sum::<u64>()).map_err(|_| anyhow!("the prior total would not fit"))?;
    let held = dir.join("authorisations");
    match fs::DirBuilder::new().mode(0o700).create(&held) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => bail!("cannot keep the authorisations"),
    }
    let about = held.symlink_metadata().map_err(|_| anyhow!("cannot keep the authorisations"))?;
    ensure!(about.is_dir() && std::os::unix::fs::PermissionsExt::mode(&about.permissions()) & 0o077 == 0, "the authorisations directory's access is too open");
    let mut identifier = [0u8; 32];
    // SAFETY: the kernel writes at most `identifier.len()` bytes into the array it is given.
    let read = unsafe { libc::getrandom(identifier.as_mut_ptr().cast(), identifier.len(), 0) };
    ensure!(usize::try_from(read).ok() == Some(identifier.len()), "no randomness");
    let identifier = hex::encode(identifier);
    let made = Authorisation { task, attempts, round, prior };
    link::keep_private(&held.join(&identifier), made.text().as_bytes()).map_err(|_| anyhow!("cannot keep the authorisation"))?;
    println!("{identifier}");
    Ok(())
}

/// Prints every authorisation and what has been spent of it, or nothing at all. Reads no key and
/// takes no lock, so it can run while the notary serves.
fn tickets(mut args: impl Iterator<Item = String>) -> Result<()> {
    let usage = "usage: notary tickets --dir <directory>";
    ensure!(args.next().as_deref() == Some("--dir"), "{usage}");
    let dir = PathBuf::from(args.next().context(usage)?);
    ensure!(args.next().is_none(), "{usage}");
    let unreadable = "the run sequences cannot be read";
    let text = fs::read_to_string(dir.join("sequences")).map_err(|_| anyhow!(unreadable))?;
    let (version, runs) = parse_sequences(&text)?;
    let all = read_authorisations(&dir)?;
    ensure!(version == 2 || all.is_empty(), "the run sequences are version 1 and an authorisation exists");
    let mut spent: BTreeMap<&str, u32> = BTreeMap::new();
    for (authorisation, _) in runs.values().filter_map(|entry| entry.ticket.as_ref()) {
        ensure!(all.contains_key(authorisation), "a run holds a ticket of an authorisation that is not here");
        *spent.entry(authorisation).or_default() += 1;
    }
    let mut lines: Vec<(&str, u32, String)> = Vec::new();
    for (identifier, held) in &all {
        let used = spent.get(identifier.as_str()).copied().unwrap_or(0);
        ensure!(used <= held.attempts, "an authorisation has more tickets spent than it gave");
        let line = format!("{identifier} task {} round {} budget {} prior {} spent {used} of {}", held.task, held.round, held.attempts, held.prior, held.attempts);
        lines.push((&held.task, held.round, line));
    }
    lines.sort();
    for (_, _, line) in lines {
        println!("{line}");
    }
    Ok(())
}

/// What a session line says. It holds no byte of the exchange and no key.
#[derive(Default)]
struct Line {
    server: Option<String>,
    sent: u64,
    received: u64,
    /// Whether the peer proved the enrolled key, so its link bytes count. Never written to a line.
    enrolled: bool,
}

/// The usage file's counters, for one life of the serving process. Numbers only.
#[derive(Default)]
struct Usage {
    attested: u64,
    refused: u64,
    link_sent: u64,
    link_received: u64,
}

impl Usage {
    /// Rewrites the file whole, with this process's CPU time and peak memory so far.
    fn save(&self, path: &Path) -> std::io::Result<()> {
        // SAFETY: `getrusage` only writes the struct it is given, which is zeroed and owned here.
        let mut own: libc::rusage = unsafe { std::mem::zeroed() };
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut own) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let micros = |time: libc::timeval| (time.tv_sec as u64).saturating_mul(1_000_000).saturating_add(time.tv_usec as u64);
        let cpu = micros(own.ru_utime).saturating_add(micros(own.ru_stime));
        let text = format!(
            "{USAGE_HEADER}\nattested {}\nrefused {}\ncpu_micros {cpu}\npeak_kb {}\nlink_sent {}\nlink_received {}\nend\n",
            self.attested, self.refused, own.ru_maxrss, self.link_sent, self.link_received
        );
        let fresh = path.with_extension("new");
        let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&fresh)?;
        file.write_all(text.as_bytes())?;
        fs::rename(&fresh, path)
    }
}

/// The session lines and the usage file move together, under one lock.
struct Books {
    lines: fs::File,
    usage: Usage,
    usage_path: PathBuf,
}

/// Counts the bytes crossing the link to the operator, and sees nothing else of them.
struct Counted {
    inner: TcpStream,
    read: Arc<AtomicU64>,
    written: Arc<AtomicU64>,
}

impl AsyncRead for Counted {
    fn poll_read(mut self: Pin<&mut Self>, context: &mut TaskContext<'_>, buffer: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        let before = buffer.filled().len();
        let polled = Pin::new(&mut self.inner).poll_read(context, buffer);
        if matches!(polled, Poll::Ready(Ok(()))) {
            self.read.fetch_add((buffer.filled().len() - before) as u64, Ordering::Relaxed);
        }
        polled
    }
}

impl AsyncWrite for Counted {
    fn poll_write(mut self: Pin<&mut Self>, context: &mut TaskContext<'_>, bytes: &[u8]) -> Poll<std::io::Result<usize>> {
        let polled = Pin::new(&mut self.inner).poll_write(context, bytes);
        if let Poll::Ready(Ok(count)) = polled {
            self.written.fetch_add(count as u64, Ordering::Relaxed);
        }
        polled
    }

    fn poll_flush(mut self: Pin<&mut Self>, context: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(context)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(context)
    }
}

struct Notary {
    key: [u8; 32],
    acceptor: TlsAcceptor,
    approved: String,
    roots: RootCertStore,
    /// Where the customer's provider check keeps its state.
    dir: PathBuf,
    sequences: Mutex<Sequences>,
    books: Mutex<Books>,
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
    /// Writes the session line, then the usage file that counts it. `link` is what the notary sent
    /// and received on this connection, zero for a peer that never proved the enrolled key.
    fn write_line(&self, line: &Line, outcome: &str, link: (u64, u64)) {
        let time = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs());
        let text = format!("{time} {} {} {} {outcome}\n", line.server.as_deref().unwrap_or("none"), line.sent, line.received);
        let mut books = self.books.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if books.lines.write_all(text.as_bytes()).is_err() {
            eprintln!("cannot write a session line");
        }
        let usage = &mut books.usage;
        if outcome.starts_with("attested ") {
            usage.attested += 1;
        } else {
            usage.refused += 1;
        }
        usage.link_sent = usage.link_sent.saturating_add(link.0);
        usage.link_received = usage.link_received.saturating_add(link.1);
        // A file that falls behind the lines makes the report refuse, so this cannot misreport.
        if books.usage.save(&books.usage_path).is_err() {
            eprintln!("cannot write the usage file");
        }
    }

    async fn serve(self: Arc<Self>, socket: TcpStream, evicted: oneshot::Receiver<()>) {
        let mut line = Line::default();
        let started = Instant::now();
        let (read, written) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
        let socket = Counted { inner: socket, read: read.clone(), written: written.clone() };
        let outcome = match tokio::time::timeout(DEADLINE, self.session(socket, evicted, &mut line)).await {
            Ok(Ok((run, sequence))) => format!("attested {run} {sequence}"),
            Ok(Err("dropped")) if started.elapsed() >= DEADLINE - DEADLINE_MARGIN => "refused deadline".to_string(),
            Ok(Err(reason)) => format!("refused {reason}"),
            Err(_) => "refused deadline".to_string(),
        };
        let link = if line.enrolled { (written.load(Ordering::Relaxed), read.load(Ordering::Relaxed)) } else { (0, 0) };
        self.write_line(&line, &outcome, link);
    }

    /// One session, through the states of `design.md` §4 as ADR-014 amends them. The sequence number
    /// is saved when the session opens, and nothing is signed before the server's name is verified.
    async fn session(&self, socket: Counted, evicted: oneshot::Receiver<()>, line: &mut Line) -> Result<(String, u64), Reason> {
        // A peer that is not the enrolled key ends here, before the protocol starts.
        let accepted = tokio::select! {
            accepted = tokio::time::timeout(HANDSHAKE, self.acceptor.accept(socket)) => accepted,
            _ = evicted => return Err("unenrolled"),
        };
        let mut tls = accepted.map_err(|_| "unenrolled")?.map_err(|_| "unenrolled")?;
        line.enrolled = true;
        let _open = self.open.clone().try_acquire_owned().map_err(|_| "busy")?;

        let first = link::read_line(&mut tls, RELEASE.len() + 130).await.map_err(|_| "dropped")?;
        // The release first, so an older tool's two-field line hears `release`.
        let (release, rest) = first.split_once(' ').ok_or("dropped")?;
        if release != RELEASE {
            let _ = link::write_line(&mut tls, "release").await;
            let _ = tokio::io::AsyncWriteExt::shutdown(&mut tls).await;
            return Err("release");
        }
        let (run, authorisation) = rest.split_once(' ').unwrap_or((rest, ""));
        binding(run, 0).map_err(|_| "dropped")?;
        let run = run.to_string();
        // Before anything is spent, so a held notary costs the customer nothing. The pass read here
        // is the one signed.
        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok().map(|since| since.as_secs());
        let Some(pass) = now.and_then(|now| canary::fresh_pass(&self.dir, now)) else {
            let _ = link::write_line(&mut tls, "canary").await;
            let _ = tokio::io::AsyncWriteExt::shutdown(&mut tls).await;
            return Err("canary");
        };
        // Spent before the operator can see any reply, so leaving a session it dislikes cannot give
        // the number or the ticket back (ADR-014).
        let spent = self.sequences.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).spend(&run, authorisation);
        let (sequence, ticket) = match spent {
            Ok(spent) => spent,
            Err(word) => {
                let _ = link::write_line(&mut tls, word).await;
                let _ = tokio::io::AsyncWriteExt::shutdown(&mut tls).await;
                return Err(word);
            }
        };
        link::write_line(&mut tls, "ok").await.map_err(|_| "dropped")?;

        let session = Session::new(tls.compat());
        let (driver, mut handle) = session.split();
        // The driver alone reads the link. When it fails, a verifier waiting on the handle is not
        // always told and would hold the session to the deadline (#453), so a failed driver ends
        // the protocol here. A driver that ends cleanly sends nothing: that follows a close.
        let (failed, link_failed) = oneshot::channel();
        let mut driver = Aborting(tokio::spawn(async move {
            let ended = driver.await;
            if ended.is_err() {
                let _ = failed.send(());
            }
            ended
        }));
        let protocol = async {
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
        };
        let ran: Result<_, Reason> = tokio::select! {
            ran = protocol => ran,
            Ok(()) = link_failed => Err("dropped"),
        };
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
            self.sign(&request, &tls_transcript, transcript_commitments, &run, sequence, &ticket, pass, sent as u32, received as u32)
        };
        match decided {
            Ok((sequence, attestation)) => {
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

    /// Checks the request and signs it under the number this session spent when it opened.
    #[allow(clippy::too_many_arguments)]
    fn sign(&self, request: &[u8], transcript: &tlsn::transcript::TlsTranscript, commitments: Vec<TranscriptCommitment>, run: &str, sequence: u64, ticket: &Ticket, pass: u64, sent: u32, received: u32) -> Result<(u64, Vec<u8>), Reason> {
        // Judged again on the time being signed, so the notary never signs what the audit refuses.
        #[cfg(feature = "forge-ticket")]
        let session = crate::forged_session_time(transcript.time());
        #[cfg(not(feature = "forge-ticket"))]
        let session = transcript.time();
        if !canary::fresh(pass, session) {
            return Err("canary");
        }
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
        builder
            .connection_info(ConnectionInfo { time: transcript.time(), version: transcript.version(), transcript_length: TranscriptLength { sent, received } })
            .server_ephemeral_key(certificate.server_ephemeral_key.clone())
            .transcript_commitments(commitments)
            .extension(Extension { id: RUN_SEQUENCE.to_vec(), value: run_sequence(run, sequence) });
        // The hook exists only in `forging-notary`, so this program does not compile with the feature.
        #[cfg(feature = "forge-ticket")]
        let tickets = crate::forged_ticket(ticket, sequence);
        #[cfg(not(feature = "forge-ticket"))]
        let tickets = [ticket.bytes()];
        for value in tickets {
            builder.extension(Extension { id: TICKET.to_vec(), value });
        }
        #[cfg(feature = "forge-ticket")]
        let canaries = crate::forged_canary(pass, transcript.time());
        #[cfg(not(feature = "forge-ticket"))]
        let canaries = [pass.to_be_bytes().to_vec()];
        for value in canaries {
            builder.extension(Extension { id: CANARY.to_vec(), value });
        }
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

/// The usage file, read strictly: anything this program would not have written is a refusal.
fn read_usage(path: &Path) -> Result<(Usage, u64, u64)> {
    let unreadable = "the usage file cannot be read";
    let text = fs::read_to_string(path).map_err(|_| anyhow!(unreadable))?;
    let mut lines = text.lines();
    ensure!(text.ends_with('\n') && lines.next() == Some(USAGE_HEADER), unreadable);
    let mut field = |name: &str| -> Result<u64> {
        let value = lines.next().and_then(|line| line.strip_prefix(name)).and_then(|rest| rest.strip_prefix(' ')).context(unreadable)?;
        let number: u64 = value.parse().map_err(|_| anyhow!(unreadable))?;
        ensure!(number.to_string() == value, unreadable);
        Ok(number)
    };
    let (attested, refused, cpu, peak, link_sent, link_received) = (field("attested")?, field("refused")?, field("cpu_micros")?, field("peak_kb")?, field("link_sent")?, field("link_received")?);
    ensure!(lines.next() == Some("end") && lines.next().is_none(), unreadable);
    Ok((Usage { attested, refused, link_sent, link_received }, cpu, peak))
}

/// Whether a session line is attested, or `None` for a line this program would not have written.
fn line_attested(line: &str) -> Option<bool> {
    let fields: Vec<&str> = line.split(' ').collect();
    let numbers = fields.len() >= 6 && [0, 2, 3].iter().all(|&at| fields[at].parse::<u64>().is_ok());
    match (numbers, fields.get(4).copied(), fields.len()) {
        (true, Some("attested"), 7) => (binding(fields[5], 0).is_ok() && fields[6].parse::<u64>().is_ok()).then_some(true),
        (true, Some("refused"), 6) => Some(false),
        _ => None,
    }
}

/// Checks that the newest lines are the sessions the usage file counts.
fn check_lines(path: &Path, usage: &Usage) -> Result<()> {
    let disagree = "the session lines and the usage file disagree";
    let file = fs::File::open(path).map_err(|_| anyhow!("the session lines cannot be read"))?;
    let wanted = usize::try_from(usage.attested.checked_add(usage.refused).context(disagree)?).map_err(|_| anyhow!(disagree))?;
    // Anyone who reaches the port can grow this file, so only the newest lines are held.
    let mut newest = VecDeque::new();
    for line in std::io::BufRead::lines(std::io::BufReader::new(file)) {
        let line = line.map_err(|_| anyhow!("the session lines cannot be read"))?;
        newest.push_back(line_attested(&line).context("the session lines cannot be read")?);
        if newest.len() > wanted {
            newest.pop_front();
        }
    }
    ensure!(newest.len() == wanted && newest.iter().filter(|&&attested| attested).count() as u64 == usage.attested, disagree);
    Ok(())
}

/// The mean of a file of one call's seconds per line, and how many calls it holds.
fn wall_times(path: &Path) -> Result<(f64, usize)> {
    let unreadable = "a wall-time file cannot be read";
    let text = fs::read_to_string(path).map_err(|_| anyhow!(unreadable))?;
    let mut times = Vec::new();
    for line in text.lines() {
        let seconds: f64 = line.parse().map_err(|_| anyhow!(unreadable))?;
        ensure!(seconds.is_finite() && seconds > 0.0, unreadable);
        times.push(seconds);
    }
    ensure!(!times.is_empty(), unreadable);
    Ok((times.iter().sum::<f64>() / times.len() as f64, times.len()))
}

fn report(mut args: impl Iterator<Item = String>) -> Result<()> {
    let usage_text = "usage: notary report --dir <directory> [--wall <file>] [--real-wall <file>]";
    let (mut dir, mut wall, mut real) = (None, None, None);
    while let Some(flag) = args.next() {
        let slot = match flag.as_str() {
            "--dir" => &mut dir,
            "--wall" => &mut wall,
            "--real-wall" => &mut real,
            _ => bail!("{usage_text}"),
        };
        ensure!(slot.replace(PathBuf::from(args.next().context(usage_text)?)).is_none(), "{usage_text}");
    }
    let dir = dir.context(usage_text)?;
    let (usage, cpu, peak) = read_usage(&dir.join("usage"))?;
    check_lines(&dir.join("sessions.log"), &usage)?;
    println!("sessions: {} attested, {} refused, in one life of the serving process", usage.attested, usage.refused);
    if usage.attested < MEASURED_FROM {
        println!("not yet a measurement: the figures need {MEASURED_FROM} attested sessions");
        return Ok(());
    }
    let each = usage.attested as f64;
    println!("notary CPU: {:.3} s per attestation", cpu as f64 / 1_000_000.0 / each);
    println!("notary peak memory: {peak} KB, one figure for the process");
    println!("link bytes per attestation: {:.0} sent by the notary, {:.0} received", usage.link_sent as f64 / each, usage.link_received as f64 / each);
    match wall {
        Some(file) => {
            let (mean, calls) = wall_times(&file)?;
            ensure!(calls as u64 == usage.attested, "the wall-time file does not cover the reported sessions");
            println!("operator's wall time, without the real provider: {mean:.3} s per call over {calls} calls");
        }
        None => println!("operator's wall time, without the real provider: not supplied"),
    }
    match real {
        Some(file) => {
            let (mean, calls) = wall_times(&file)?;
            println!("operator's wall time, with the real provider: {mean:.3} s per call over {calls} call(s)");
        }
        None => println!("operator's wall time, with the real provider: not supplied"),
    }
    Ok(())
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
    // Under the authorisations' lock, so none can appear while a version 1 file is being moved.
    let authorising = authorisations_lock(&start.dir)?;
    let sequences = Sequences::load(start.dir.join("sequences"), signing.symlink_metadata().is_ok())?;
    drop(authorising);
    let (key, signing_public) = signing_key(&signing)?;
    let transport = Identity::make_or_load(&start.dir.join("transport.key"))?;
    let mut roots = RootCertStore::mozilla();
    for file in &start.roots {
        roots.roots.push(CertificateDer(fs::read(file).map_err(|_| anyhow!("cannot read a trusted root"))?));
    }
    let lines = fs::OpenOptions::new().append(true).create(true).mode(0o600).open(start.dir.join("sessions.log")).context("cannot keep the session lines")?;
    // A new life starts its counters at zero, because CPU time and peak memory start there too.
    let usage_path = start.dir.join("usage");
    let usage = Usage::default();
    usage.save(&usage_path).context("cannot keep the usage file")?;
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
        dir: start.dir,
        sequences: Mutex::new(sequences),
        books: Mutex::new(Books { lines, usage, usage_path }),
        open: Arc::new(Semaphore::new(1)),
    });
    let _lock = lock;
    let mut pending: VecDeque<oneshot::Sender<()>> = VecDeque::new();
    loop {
        let Ok((socket, _)) = listener.accept().await else { continue };
        // While a session is open, any other connection is closed at once.
        if notary.open.available_permits() == 0 {
            drop(socket);
            notary.write_line(&Line::default(), "refused busy", (0, 0));
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
pub(crate) async fn main() -> std::process::ExitCode {
    let mut args = std::env::args().skip(1).peekable();
    let done = match args.peek().map(String::as_str) {
        Some("report") => {
            args.next();
            report(args)
        }
        Some("authorize") => {
            args.next();
            authorize(args)
        }
        Some("tickets") => {
            args.next();
            tickets(args)
        }
        _ => run().await,
    };
    match done {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("refused: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
