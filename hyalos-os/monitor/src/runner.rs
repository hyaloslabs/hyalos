//! The machine runner and its channel, `hyalos.runner/1` (ADR-003 and `design.md` §2 of
//! `docs/plans/hyalos-kernel-core`).
//!
//! Every KVM run happens in a separate process, the machine runner: this program again, started
//! with the hidden `machine-runner` command, an empty environment, `/` as its directory, standard
//! output and error null, and one end of a Unix socket pair as its standard input. It opens KVM,
//! confines itself with Landlock, reads one `Boot` frame, builds the machine, installs its seccomp
//! allow-list and only then runs it, sending each of the kernel's requests to the holder and writing
//! back the reply. It holds no credential, no file of the operator's and no tool, and once sealed it
//! can open no file, use no socket, start no program and signal no process (ADR-0132). A runner that
//! cannot confine itself never runs a guest instruction.
//!
//! The holder is the process that started it. It keeps `Host`, the deadline, the model calls, the
//! effect and the chain, and trusts nothing the runner sends: every frame is bounded, versioned
//! and must come in its turn, every length is checked again against `Host`'s limits, and the
//! single-effect rule is `Host`'s own. A frame that breaks any of this, and a runner that dies or
//! closes the channel, ends the run with `KernelFault`; the deadline ends it with `Timeout`. Both
//! are reasons a run already gave, so no record changes shape.
//!
//! A frame is a 4-byte little-endian length, counting every byte after it, then the version byte,
//! the type byte and the fields.

use crate::{
    MAX_RESULT, Reason,
    kvm::{self, Calls, GUEST_MAX, RECORD_MAX},
};
use hyalos_audit::{Host, Source};
use std::{
    io::{ErrorKind, Read, Write},
    os::{
        fd::{AsFd, AsRawFd, OwnedFd, RawFd},
        unix::net::UnixStream,
    },
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

/// The version byte of `hyalos.runner/1`.
pub(crate) const VERSION: u8 = 1;

/// The most bytes after the length field, by direction: the `Boot` frame, a reply (the heap's
/// size, so any record the kernel could hold fits), and anything the runner sends (the largest
/// effect body and a header).
pub(crate) const BOOT_LIMIT: usize = 8 << 20;
pub(crate) const REPLY_LIMIT: usize = (16 << 20) + 64;
pub(crate) const REQUEST_LIMIT: usize = (64 << 10) + 64;

/// The type bytes, in `design.md`'s order.
pub(crate) const BOOT: u8 = 1;
pub(crate) const REPLY: u8 = 2;
pub(crate) const INPUT: u8 = 3;
pub(crate) const PRE_INFER: u8 = 4;
pub(crate) const PRE_EFFECT: u8 = 5;
pub(crate) const INFER: u8 = 6;
pub(crate) const EFFECT: u8 = 7;
pub(crate) const FINISHED: u8 = 8;
pub(crate) const FAILED: u8 = 9;

/// Every reason, by the byte that carries it on the channel. The order is this table's own, not
/// the `Reason` enum's, so reordering the enum changes nothing here. A new reason goes at the end.
pub(crate) const REASONS: [Reason; 29] = [
    Reason::InvalidModule,
    Reason::BadImport,
    Reason::BadExport,
    Reason::OutOfFuel,
    Reason::GuestTrap,
    Reason::BadPointer,
    Reason::TooLarge,
    Reason::UnknownSchema,
    Reason::UnknownEffect,
    Reason::EffectLimit,
    Reason::NoResult,
    Reason::ReplayDiverged,
    Reason::GuestMismatch,
    Reason::KernelFault,
    Reason::BadHypercall,
    Reason::Timeout,
    Reason::NoKvm,
    Reason::ModelUnavailable,
    Reason::ModelRefused,
    Reason::NotaryUnreachable,
    Reason::NotaryDeadline,
    Reason::WrongNotaryKey,
    Reason::ReleaseMismatch,
    Reason::ProviderRefused,
    Reason::NoAuthorisation,
    Reason::AttemptsSpent,
    Reason::CanaryHeld,
    Reason::EffectFailed,
    Reason::LedgerFailed,
];

/// The reasons a runner can end a run with: what the machine and the kernel's requests give. Any
/// other reason in a `Failed` frame is a broken protocol, since only the holder decides the rest.
pub(crate) const RUNNER_REASONS: [Reason; 11] = [
    Reason::InvalidModule,
    Reason::BadImport,
    Reason::BadExport,
    Reason::OutOfFuel,
    Reason::GuestTrap,
    Reason::NoResult,
    Reason::BadPointer,
    Reason::TooLarge,
    Reason::KernelFault,
    Reason::BadHypercall,
    Reason::NoKvm,
];

fn reason_byte(reason: Reason) -> u8 {
    REASONS
        .iter()
        .position(|r| *r == reason)
        .map_or(u8::MAX, |at| at as u8)
}

fn reason_of(byte: u8) -> Option<Reason> {
    REASONS.get(usize::from(byte)).copied()
}

/// One frame's bytes, length field included.
pub(crate) fn frame(kind: u8, fields: &[&[u8]]) -> Vec<u8> {
    let len: usize = 2 + fields.iter().map(|f| f.len()).sum::<usize>();
    let mut out = Vec::with_capacity(4 + len);
    out.extend_from_slice(&(len as u32).to_le_bytes());
    out.extend_from_slice(&[VERSION, kind]);
    for f in fields {
        out.extend_from_slice(f);
    }
    out
}

/// The `Boot` frame: the kernel's length, the kernel and the guest. `None` when it is over its limit.
pub(crate) fn boot(kernel: &[u8], guest: &[u8]) -> Option<Vec<u8>> {
    let len = u32::try_from(kernel.len()).ok()?;
    (2 + 4 + kernel.len() + guest.len() <= BOOT_LIMIT)
        .then(|| frame(BOOT, &[&len.to_le_bytes(), kernel, guest]))
}

fn split_boot(fields: &[u8]) -> Option<(&[u8], &[u8])> {
    let len = u32::from_le_bytes(fields.get(..4)?.try_into().ok()?) as usize;
    let rest = &fields[4..];
    (len <= rest.len()).then(|| rest.split_at(len))
}

/// What the runner sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Request {
    Input(i32),
    PreInfer(i32),
    PreEffect(i32),
    Infer(i32, Vec<u8>),
    Effect(i32, Vec<u8>),
    Finished(Vec<u8>, u64),
    Failed(Reason),
}

fn word(bytes: &[u8]) -> Option<i32> {
    Some(i32::from_le_bytes(bytes.get(..4)?.try_into().ok()?))
}

fn exactly_word(bytes: &[u8]) -> Option<i32> {
    (bytes.len() == 4).then(|| word(bytes))?
}

impl Request {
    pub(crate) fn encode(&self) -> Vec<u8> {
        match self {
            Request::Input(index) => frame(INPUT, &[&index.to_le_bytes()]),
            Request::PreInfer(schema) => frame(PRE_INFER, &[&schema.to_le_bytes()]),
            Request::PreEffect(kind) => frame(PRE_EFFECT, &[&kind.to_le_bytes()]),
            Request::Infer(schema, question) => frame(INFER, &[&schema.to_le_bytes(), question]),
            Request::Effect(kind, body) => frame(EFFECT, &[&kind.to_le_bytes(), body]),
            Request::Finished(result, fuel) => frame(FINISHED, &[&fuel.to_le_bytes(), result]),
            Request::Failed(reason) => frame(FAILED, &[&[reason_byte(*reason)]]),
        }
    }

    /// `None` for an unknown type or fields of the wrong shape.
    pub(crate) fn decode(kind: u8, fields: &[u8]) -> Option<Self> {
        Some(match kind {
            INPUT => Request::Input(exactly_word(fields)?),
            PRE_INFER => Request::PreInfer(exactly_word(fields)?),
            PRE_EFFECT => Request::PreEffect(exactly_word(fields)?),
            INFER => Request::Infer(word(fields)?, fields[4..].to_vec()),
            EFFECT => Request::Effect(word(fields)?, fields[4..].to_vec()),
            FINISHED => Request::Finished(
                fields.get(8..)?.to_vec(),
                u64::from_le_bytes(fields.get(..8)?.try_into().ok()?),
            ),
            FAILED => match fields {
                [byte] => Request::Failed(reason_of(*byte)?),
                _ => return None,
            },
            _ => return None,
        })
    }
}

/// What the holder answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reply {
    /// A record's length, and its bytes unless it is longer than the heap.
    Record(u64, Vec<u8>),
    NoRecord,
    Limit(u64),
    Label(i32),
    Ok,
    Refused(Reason),
}

impl Reply {
    /// The reply for a record. A record longer than the heap could never be written into the
    /// kernel's memory, so only its length is sent, which is all `answer` needs to refuse it.
    fn record(record: &[u8]) -> Self {
        let len = record.len() as u64;
        Reply::Record(
            len,
            if len > RECORD_MAX {
                Vec::new()
            } else {
                record.to_vec()
            },
        )
    }

    pub(crate) fn encode(&self) -> Vec<u8> {
        match self {
            Reply::Record(len, bytes) => frame(REPLY, &[&[0], &len.to_le_bytes(), bytes]),
            Reply::NoRecord => frame(REPLY, &[&[1]]),
            Reply::Limit(max) => frame(REPLY, &[&[2], &max.to_le_bytes()]),
            Reply::Label(label) => frame(REPLY, &[&[3], &label.to_le_bytes()]),
            Reply::Ok => frame(REPLY, &[&[4]]),
            Reply::Refused(reason) => frame(REPLY, &[&[5, reason_byte(*reason)]]),
        }
    }

    pub(crate) fn decode(kind: u8, fields: &[u8]) -> Option<Self> {
        if kind != REPLY {
            return None;
        }
        let (&tag, rest) = fields.split_first()?;
        Some(match (tag, rest) {
            (0, _) => {
                let len = u64::from_le_bytes(rest.get(..8)?.try_into().ok()?);
                let bytes = &rest[8..];
                let whole = bytes.len() as u64 == len || (bytes.is_empty() && len > RECORD_MAX);
                whole.then(|| Reply::Record(len, bytes.to_vec()))?
            }
            (1, []) => Reply::NoRecord,
            (2, _) => Reply::Limit(u64::from_le_bytes(rest.try_into().ok()?)),
            (3, _) => Reply::Label(exactly_word(rest)?),
            (4, []) => Reply::Ok,
            (5, [byte]) => Reply::Refused(reason_of(*byte)?),
            _ => return None,
        })
    }
}

/// Why a frame could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Broken {
    /// The deadline passed first.
    Late,
    /// The channel closed, failed, or carried a frame over its limit or of another version.
    Fault,
}

impl Broken {
    fn reason(self) -> Reason {
        match self {
            Broken::Late => Reason::Timeout,
            Broken::Fault => Reason::KernelFault,
        }
    }
}

/// Fills `buf`, waiting no later than `deadline` when there is one.
fn read_full(
    stream: &mut UnixStream,
    buf: &mut [u8],
    deadline: Option<Instant>,
) -> Result<(), Broken> {
    let mut got = 0;
    while got < buf.len() {
        if let Some(deadline) = deadline {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Broken::Late);
            }
            stream
                .set_read_timeout(Some(left))
                .map_err(|_| Broken::Fault)?;
        }
        match stream.read(&mut buf[got..]) {
            Ok(0) => return Err(Broken::Fault),
            Ok(n) => got += n,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            // The timeout: the loop sees whether the deadline has passed.
            Err(error)
                if deadline.is_some()
                    && matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => return Err(Broken::Fault),
        }
    }
    Ok(())
}

/// Reads one frame of at most `limit` bytes after its length, and returns its type and fields.
/// Nothing past the length field is read from a frame over the limit.
pub(crate) fn read_frame(
    stream: &mut UnixStream,
    limit: usize,
    deadline: Option<Instant>,
) -> Result<(u8, Vec<u8>), Broken> {
    let mut len = [0u8; 4];
    read_full(stream, &mut len, deadline)?;
    let len = u32::from_le_bytes(len) as usize;
    if !(2..=limit).contains(&len) {
        return Err(Broken::Fault);
    }
    let mut body = vec![0u8; len];
    read_full(stream, &mut body, deadline)?;
    if body[0] != VERSION {
        return Err(Broken::Fault);
    }
    let fields = body.split_off(2);
    Ok((body[1], fields))
}

fn write_frame(stream: &mut UnixStream, bytes: &[u8], deadline: Instant) -> Result<(), Reason> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(Reason::Timeout);
    }
    stream
        .set_write_timeout(Some(left))
        .map_err(|_| Reason::KernelFault)?;
    stream.write_all(bytes).map_err(|error| {
        if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) {
            Reason::Timeout
        } else {
            Reason::KernelFault
        }
    })
}

/// Whose turn it is on the channel.
#[derive(Clone, Copy)]
enum Turn {
    /// The runner may send a request, `Finished` or `Failed`.
    Request,
    /// The question of this type, at most this long, or the runner's own refusal of its bytes.
    Infer(i32, usize),
    /// The effect of this kind, at most this long, or the runner's own refusal of its bytes.
    Effect(i32, usize),
}

/// How the holder's side of a run went, beside its outcome, for `HYALOS_TRACE`.
#[derive(Default)]
pub(crate) struct Timing {
    pub(crate) first: Option<Instant>,
    pub(crate) requests: u32,
    pub(crate) in_host: Duration,
}

/// The holder's side of one run: sends `boot`, then answers the runner through `host` until the
/// run ends, no later than `deadline`. The runner is trusted with nothing: what it sends is
/// checked here again, in the order `kvm.rs`'s `answer` asks, so every reason stays the same.
pub(crate) fn serve<S: Source>(
    stream: &mut UnixStream,
    boot: &[u8],
    mut host: Host<S>,
    deadline: Instant,
    timing: &mut Timing,
) -> Result<(Host<S>, Vec<u8>, u64), Reason> {
    // A runner that refused before reading says why in its first frame, so a failed write is not
    // the end: the next read tells.
    if write_frame(stream, boot, deadline) == Err(Reason::Timeout) {
        return Err(Reason::Timeout);
    }
    let mut turn = Turn::Request;
    loop {
        let (kind, fields) =
            read_frame(stream, REQUEST_LIMIT, Some(deadline)).map_err(Broken::reason)?;
        timing.first.get_or_insert_with(Instant::now);
        timing.requests += 1;
        let request = Request::decode(kind, &fields).ok_or(Reason::KernelFault)?;
        let before = Instant::now();
        let reply = match (turn, request) {
            (Turn::Request, Request::Input(index)) => {
                host.input(index).map_or(Reply::NoRecord, Reply::record)
            }
            (Turn::Request, Request::PreInfer(schema)) => match host.pre_infer(schema) {
                Ok(max) => {
                    turn = Turn::Infer(schema, max);
                    Reply::Limit(max as u64)
                }
                Err(reason) => return refuse(stream, reason, deadline),
            },
            (Turn::Request, Request::PreEffect(kind)) => match host.pre_effect(kind) {
                Ok(max) => {
                    turn = Turn::Effect(kind, max);
                    Reply::Limit(max as u64)
                }
                Err(reason) => return refuse(stream, reason, deadline),
            },
            (Turn::Infer(schema, max), Request::Infer(asked, question))
                if asked == schema && question.len() <= max =>
            {
                turn = Turn::Request;
                match host.infer(schema, &question) {
                    Ok(label) => Reply::Label(label),
                    Err(reason) => return refuse(stream, reason, deadline),
                }
            }
            (Turn::Effect(kind, max), Request::Effect(asked, body))
                if asked == kind && body.len() <= max =>
            {
                turn = Turn::Request;
                match host.effect(kind, &body) {
                    Ok(()) => Reply::Ok,
                    Err(reason) => return refuse(stream, reason, deadline),
                }
            }
            (Turn::Request, Request::Finished(result, fuel)) if result.len() <= MAX_RESULT => {
                return Ok((host, result, fuel));
            }
            (Turn::Request, Request::Failed(reason)) if RUNNER_REASONS.contains(&reason) => {
                return Err(reason);
            }
            // Between a type check and the bytes, the runner may only refuse the bytes themselves.
            (
                Turn::Infer(..) | Turn::Effect(..),
                Request::Failed(reason @ (Reason::TooLarge | Reason::BadPointer)),
            ) => return Err(reason),
            _ => return Err(Reason::KernelFault),
        };
        timing.in_host += before.elapsed();
        write_frame(stream, &reply.encode(), deadline)?;
    }
}

/// Tells the runner the run is refused, and ends it with that reason.
fn refuse<T>(stream: &mut UnixStream, reason: Reason, deadline: Instant) -> Result<T, Reason> {
    let _ = write_frame(stream, &Reply::Refused(reason).encode(), deadline);
    Err(reason)
}

/// A started machine runner and the holder's end of its channel.
pub(crate) struct Runner {
    pub(crate) child: Child,
    pub(crate) stream: UnixStream,
}

/// This program, which holds the `machine-runner` command. A test harness is not this program, so
/// its tests use the `task-kernel` binary built beside it. No environment variable chooses it.
fn program() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    if cfg!(test) {
        Some(exe.parent()?.parent()?.join("task-kernel"))
    } else {
        Some(exe)
    }
}

/// Starts a machine runner with `args` after the command. It inherits nothing but its channel.
pub(crate) fn start(args: &[&str]) -> Option<Runner> {
    spawn(args, Stdio::null())
}

fn spawn(args: &[&str], stderr: Stdio) -> Option<Runner> {
    let (holder, runner) = UnixStream::pair().ok()?;
    let child = Command::new(program()?)
        .arg("machine-runner")
        .args(args)
        .env_clear()
        .current_dir("/")
        .stdin(Stdio::from(OwnedFd::from(runner)))
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
        .ok()?;
    Some(Runner {
        child,
        stream: holder,
    })
}

/// Starts a machine runner with `--trace`, whose standard error is a socket of its own. Nothing
/// it writes there reaches this program's output: `read_trace` reads its timings back.
pub(crate) fn start_traced() -> Option<(Runner, UnixStream)> {
    let (holder, runner) = UnixStream::pair().ok()?;
    let args = [&["--trace"][..], &injected()].concat();
    let started = spawn(&args, Stdio::from(OwnedFd::from(runner)))?;
    Some((started, holder))
}

/// The most bytes read back from a tracing runner's standard error.
const TRACE_MAX: usize = 256;

/// The runner's three timings, read without waiting from its trace socket, when what it wrote is
/// exactly the one line `kvm::machine_run` prints. Anything else, from a runner the holder trusts
/// with nothing, gives `None`, so no byte of it is printed.
pub(crate) fn read_trace(stream: &mut UnixStream) -> Option<[f64; 3]> {
    stream.set_nonblocking(true).ok()?;
    let mut got = Vec::new();
    let mut buf = [0u8; TRACE_MAX];
    while got.len() < TRACE_MAX {
        match stream.read(&mut buf[..TRACE_MAX - got.len()]) {
            Ok(0) | Err(_) => break,
            Ok(n) => got.extend_from_slice(&buf[..n]),
        }
    }
    runner_trace(&got)
}

/// Parses `trace ms: memory+load <ms>, vm setup <ms>, boot to first call <ms>` and its newline.
pub(crate) fn runner_trace(bytes: &[u8]) -> Option<[f64; 3]> {
    let line = std::str::from_utf8(bytes).ok()?.strip_suffix('\n')?;
    let rest = line.strip_prefix("trace ms: memory+load ")?;
    let (load, rest) = rest.split_once(", vm setup ")?;
    let (setup, boot) = rest.split_once(", boot to first call ")?;
    let ms = |field: &str| {
        let digits = !field.is_empty() && field.bytes().all(|b| b.is_ascii_digit() || b == b'.');
        digits.then(|| field.parse::<f64>().ok()).flatten()
    };
    Some([ms(load)?, ms(setup)?, ms(boot)?])
}

/// The threads reaping runners that have been killed, for `wait_for_runners`.
static REAPERS: std::sync::Mutex<Vec<std::thread::JoinHandle<()>>> =
    std::sync::Mutex::new(Vec::new());

/// How long `wait_for_runners` waits for every runner to be gone. A teardown takes tens of
/// milliseconds; a runner that outlives this is left to init, which reaps it once this exits.
const REAP_BOUND: Duration = Duration::from_secs(5);

impl Runner {
    /// Kills the runner and reaps it on another thread: tearing a machine down takes the host
    /// tens of milliseconds, and the run's outcome does not wait for it.
    pub(crate) fn end(mut self) {
        let _ = self.child.kill();
        drop(self.stream);
        add_reaper(std::thread::spawn(move || {
            let _ = self.child.wait();
        }));
    }
}

/// Waits until every runner this program started is gone, so the program does not exit while one
/// of its machines is still being torn down, but no longer than `REAP_BOUND`: a teardown that is
/// stuck must not hang the program after its outcome is decided.
pub(crate) fn wait_for_runners() {
    wait_for_runners_within(REAP_BOUND);
}

pub(crate) fn wait_for_runners_within(bound: Duration) {
    let until = Instant::now() + bound;
    let reapers = REAPERS
        .lock()
        .map(|mut reapers| std::mem::take(&mut *reapers))
        .unwrap_or_default();
    for reaper in reapers {
        while !reaper.is_finished() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(1));
        }
        if reaper.is_finished() {
            let _ = reaper.join();
        }
    }
}

/// Adds a reaper to the ones `wait_for_runners` waits for.
pub(crate) fn add_reaper(reaper: std::thread::JoinHandle<()>) {
    if let Ok(mut reapers) = REAPERS.lock() {
        reapers.push(reaper);
    }
}

/// One KVM run, on a machine runner (the holder's whole side). The guest-size check comes first,
/// as it did when the monitor ran the machine itself.
pub(crate) fn run<S: Source>(
    kernel: &[u8],
    guest: &[u8],
    host: Host<S>,
    limit: Duration,
) -> Result<(Host<S>, Vec<u8>, u64), Reason> {
    let trace = std::env::var_os("HYALOS_TRACE").is_some();
    let t0 = Instant::now();
    let deadline = t0 + limit;
    if guest.len() as u64 > GUEST_MAX {
        return Err(Reason::TooLarge);
    }
    let boot = boot(kernel, guest).ok_or(Reason::KernelFault)?;
    let (mut runner, mut traced) = if trace {
        let (runner, traced) = start_traced().ok_or(Reason::KernelFault)?;
        (runner, Some(traced))
    } else {
        (start(&injected()).ok_or(Reason::KernelFault)?, None)
    };
    let started = t0.elapsed();
    let mut timing = Timing::default();
    let outcome = serve(&mut runner.stream, &boot, host, deadline, &mut timing);
    runner.end();
    if let (Some(traced), true) = (traced.as_mut(), outcome.is_ok()) {
        // The runner wrote its line before its first request, so it is there to read now.
        if let Some([load, setup, boot]) = read_trace(traced) {
            eprintln!(
                "trace ms: memory+load {load:.2}, vm setup {setup:.2}, boot to first call {boot:.2}"
            );
        }
        let ms = |d: Duration| d.as_secs_f64() * 1e3;
        let first = timing.first.map_or(Duration::ZERO, |first| first - t0);
        eprintln!(
            "trace ms: runner started {:.2}, start to first call {:.2}, calls {:.2} ({} requests, {:.2} in host), total {:.2}",
            ms(started),
            ms(first),
            ms(t0.elapsed() - first),
            timing.requests,
            ms(timing.in_host),
            ms(t0.elapsed())
        );
    }
    outcome
}

/// Whether a machine runner starts here, opens KVM and confines itself with both Landlock and its
/// seccomp filter (`machine-runner --check`). A `live` run asks before its intent link is written,
/// so a host that cannot run the machine, or cannot confine it, writes no link (S-04.3).
pub(crate) fn check() -> Result<(), &'static str> {
    let args = [&["--check"][..], &injected()].concat();
    let mut runner = start(&args).ok_or("the machine runner cannot be started")?;
    let started = Instant::now();
    loop {
        match runner.child.try_wait() {
            Ok(Some(status)) => {
                return match status.code() {
                    Some(0) => Ok(()),
                    Some(NO_KVM) => Err("the machine runner cannot open KVM"),
                    Some(NO_LANDLOCK) => Err(NO_LANDLOCK_SAID),
                    Some(NO_SECCOMP) => Err(NO_SECCOMP_SAID),
                    _ => Err("the machine runner failed its check"),
                };
            }
            Ok(None) if started.elapsed() < Duration::from_secs(10) => {
                std::thread::sleep(Duration::from_millis(2))
            }
            _ => {
                runner.end();
                return Err("the machine runner failed its check");
            }
        }
    }
}

/// What a run on a host without Landlock, or whose seccomp filter does not install, is refused
/// with. Nothing runs unconfined instead.
pub(crate) const NO_LANDLOCK_SAID: &str = "the machine runner cannot be confined: this kernel has no Landlock, or it is turned off. Use Linux 5.13 or later with landlock in the kernel's lsm= list";
pub(crate) const NO_SECCOMP_SAID: &str =
    "the machine runner cannot be confined: its seccomp filter did not install";

/// `machine-runner`'s exit codes besides `0`.
pub(crate) const NO_KVM: i32 = 1;
pub(crate) const BROKEN: i32 = 2;
pub(crate) const NO_LANDLOCK: i32 = 3;
pub(crate) const NO_SECCOMP: i32 = 4;

/// The arguments the holder adds to every runner it starts: none, except in a build with the
/// `test-switches` feature, where `HYALOS_TEST_NO_CONFINEMENT` (`landlock` or `seccomp`) makes the
/// runner fail that confinement, as a host without it would (S-04.3).
#[cfg(not(feature = "test-switches"))]
fn injected() -> Vec<&'static str> {
    Vec::new()
}

#[cfg(feature = "test-switches")]
fn injected() -> Vec<&'static str> {
    match std::env::var("HYALOS_TEST_NO_CONFINEMENT").as_deref() {
        Ok("landlock") => vec!["--pretend-no-landlock"],
        Ok("seccomp") => vec!["--pretend-no-seccomp"],
        _ => Vec::new(),
    }
}

/// The runner's side of the channel: each request is one frame to the holder and one reply back.
struct Channel(UnixStream);

impl Channel {
    fn ask(&mut self, request: Request) -> Result<Reply, Reason> {
        self.0
            .write_all(&request.encode())
            .map_err(|_| Reason::KernelFault)?;
        let (kind, fields) =
            read_frame(&mut self.0, REPLY_LIMIT, None).map_err(|_| Reason::KernelFault)?;
        match Reply::decode(kind, &fields).ok_or(Reason::KernelFault)? {
            Reply::Refused(reason) => Err(reason),
            reply => Ok(reply),
        }
    }

    fn limit(&mut self, request: Request) -> Result<usize, Reason> {
        match self.ask(request)? {
            Reply::Limit(max) => usize::try_from(max).map_err(|_| Reason::KernelFault),
            _ => Err(Reason::KernelFault),
        }
    }
}

impl Calls for Channel {
    fn input(&mut self, index: i32) -> Result<Option<(u64, Vec<u8>)>, Reason> {
        match self.ask(Request::Input(index))? {
            Reply::Record(len, bytes) => Ok(Some((len, bytes))),
            Reply::NoRecord => Ok(None),
            _ => Err(Reason::KernelFault),
        }
    }

    fn pre_infer(&mut self, schema: i32) -> Result<usize, Reason> {
        self.limit(Request::PreInfer(schema))
    }

    fn infer(&mut self, schema: i32, question: &[u8]) -> Result<i32, Reason> {
        match self.ask(Request::Infer(schema, question.to_vec()))? {
            Reply::Label(label) => Ok(label),
            _ => Err(Reason::KernelFault),
        }
    }

    fn pre_effect(&mut self, kind: i32) -> Result<usize, Reason> {
        self.limit(Request::PreEffect(kind))
    }

    fn effect(&mut self, kind: i32, body: &[u8]) -> Result<(), Reason> {
        match self.ask(Request::Effect(kind, body.to_vec()))? {
            Reply::Ok => Ok(()),
            _ => Err(Reason::KernelFault),
        }
    }
}

/// Closes every descriptor above standard error. The holder opens its own with close-on-exec, but
/// whatever started the holder may have left it others without (a CI agent's or a jobserver's
/// pipes, a shell's redirection), and a process inherits those through every `exec`. The runner
/// holds nothing but its channel and KVM, so it drops them before it opens anything. `false` when
/// the host cannot close them (`close_range` needs Linux 5.9), and the runner then refuses.
fn close_inherited() -> bool {
    // SAFETY: a plain system call. Nothing in this process owns a descriptor above 2 yet: the
    // runtime opens none, and this runs before the runner opens KVM or copies its channel.
    unsafe { libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 0u32) == 0 }
}

/// How `machine-runner` was asked to run.
#[derive(Default)]
pub(crate) struct Options {
    check: bool,
    trace: bool,
    /// The confinement this runner fails on purpose, as a host without it would (S-04.3).
    #[cfg(feature = "test-switches")]
    unconfined: Option<Confinement>,
    /// What this runner attempts once sealed, in place of running the machine (S-04.2).
    #[cfg(feature = "test-switches")]
    hostile: Option<hostile::Act>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Confinement {
    Landlock,
    Seccomp,
}

impl Options {
    /// The arguments after `machine-runner`: nothing, `--check` or `--trace`, and in a build with
    /// the `test-switches` feature the test-only arguments after them. `None` for anything else.
    pub(crate) fn parse(args: &[String]) -> Option<Self> {
        let mut options = Options::default();
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let rest = match args.as_slice() {
            ["--check", rest @ ..] => {
                options.check = true;
                rest
            }
            ["--trace", rest @ ..] => {
                options.trace = true;
                rest
            }
            rest => rest,
        };
        #[cfg(feature = "test-switches")]
        let rest = options.test_switches(&rest)?;
        rest.is_empty().then_some(options)
    }

    #[cfg(feature = "test-switches")]
    fn test_switches<'a>(&mut self, mut rest: &'a [&'a str]) -> Option<&'a [&'a str]> {
        loop {
            match rest {
                ["--pretend-no-landlock", more @ ..] if self.unconfined.is_none() => {
                    self.unconfined = Some(Confinement::Landlock);
                    rest = more;
                }
                ["--pretend-no-seccomp", more @ ..] if self.unconfined.is_none() => {
                    self.unconfined = Some(Confinement::Seccomp);
                    rest = more;
                }
                ["--hostile", act, more @ ..] if self.hostile.is_none() && !self.check => {
                    self.hostile = Some(hostile::Act::named(act)?);
                    rest = more;
                }
                _ => return Some(rest),
            }
        }
    }

    /// Whether this runner fails `confinement` on purpose. Never, without `test-switches`.
    fn fails(&self, confinement: Confinement) -> bool {
        #[cfg(feature = "test-switches")]
        return self.unconfined == Some(confinement);
        #[cfg(not(feature = "test-switches"))]
        {
            let _ = confinement;
            false
        }
    }
}

/// Gives up every file right this kernel's Landlock can take away, with no rule granting any back,
/// and, where the kernel has them, TCP, signals to other processes and abstract sockets. The
/// first version's file rights are required; `false` when this kernel cannot enforce them.
/// Descriptors already open, KVM and the channel, keep working.
fn landlock(fail: bool) -> bool {
    use landlock::{
        ABI, Access, AccessFs, AccessNet, CompatLevel, Compatible, Ruleset, RulesetAttr,
        RulesetStatus, Scope,
    };
    if fail {
        // A ruleset that handles nothing is refused by the library, as an absent Landlock is.
        return Ruleset::default().create().is_ok_and(|_| false);
    }
    let restricted = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(ABI::V1))
        .and_then(|r| {
            r.set_compatibility(CompatLevel::BestEffort)
                .handle_access(AccessFs::from_all(ABI::V6))
        })
        .and_then(|r| r.handle_access(AccessNet::from_all(ABI::V6)))
        .and_then(|r| r.scope(Scope::from_all(ABI::V6)))
        .and_then(|r| r.create())
        .and_then(|r| r.restrict_self());
    matches!(restricted, Ok(status) if status.ruleset != RulesetStatus::NotEnforced)
}

/// `KVM_RUN`, `_IO(KVMIO, 0x80)`: the one request the sealed runner may make of KVM.
const KVM_RUN: u64 = 0xae80;

/// Every system call the sealed runner may make, and why (`design.md` §2 of
/// `docs/plans/hyalos-kernel-core`, the seccomp allow-list, and ADR-0132). Any other, and any of
/// these outside its restriction, kills the runner with `SIGSYS`. The list is what a traced runner
/// made once sealed, on every path its tests and the `attacks` command take. A new entry needs a
/// reason that holds: none may reach a file, a network, another process or executable memory.
pub(crate) const ALLOWED: [(&str, i64); 9] = [
    // Only `KVM_RUN`, only on the machine's virtual CPU.
    ("ioctl", libc::SYS_ioctl),
    // Only on the channel, a connected socket, and `sendto` with no address: how the standard
    // library reads and writes a Unix stream.
    ("recvfrom", libc::SYS_recvfrom),
    ("sendto", libc::SYS_sendto),
    // Only standard error, and only under `--trace`, for its one timing line.
    ("write", libc::SYS_write),
    // Memory for the frames: only anonymous, never executable.
    ("mmap", libc::SYS_mmap),
    ("munmap", libc::SYS_munmap),
    ("brk", libc::SYS_brk),
    // Tearing the machine down closes its descriptors; then the runner exits.
    ("close", libc::SYS_close),
    ("exit_group", libc::SYS_exit_group),
];

/// The seccomp program for a runner whose channel is `channel` and virtual CPU `vcpu`.
fn filter(channel: RawFd, vcpu: RawFd, trace: bool) -> Option<seccompiler::BpfProgram> {
    use seccompiler::{
        SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
        SeccompRule, TargetArch,
    };
    let when = |arg: u8, len: SeccompCmpArgLen, op: SeccompCmpOp, value: u64| {
        SeccompCondition::new(arg, len, op, value).ok()
    };
    let fd = |arg: u8, fd: RawFd| {
        when(
            arg,
            SeccompCmpArgLen::Dword,
            SeccompCmpOp::Eq,
            u64::from(fd as u32),
        )
    };
    let rule = |conditions: Vec<Option<SeccompCondition>>| {
        SeccompRule::new(conditions.into_iter().collect::<Option<_>>()?).ok()
    };
    let (exec, anonymous) = (libc::PROT_EXEC as u64, libc::MAP_ANONYMOUS as u64);
    let mut rules = std::collections::BTreeMap::new();
    for (name, number) in ALLOWED {
        let conditions = match name {
            "ioctl" => vec![rule(vec![
                fd(0, vcpu),
                when(1, SeccompCmpArgLen::Dword, SeccompCmpOp::Eq, KVM_RUN),
            ])?],
            "recvfrom" => vec![rule(vec![fd(0, channel)])?],
            "sendto" => vec![rule(vec![
                fd(0, channel),
                when(4, SeccompCmpArgLen::Qword, SeccompCmpOp::Eq, 0),
            ])?],
            "write" if trace => vec![rule(vec![fd(0, 2)])?],
            // An empty list would allow it unconditionally.
            "write" => continue,
            "mmap" => vec![rule(vec![
                when(2, SeccompCmpArgLen::Qword, SeccompCmpOp::MaskedEq(exec), 0),
                when(
                    3,
                    SeccompCmpArgLen::Qword,
                    SeccompCmpOp::MaskedEq(anonymous),
                    anonymous,
                ),
            ])?],
            _ => Vec::new(),
        };
        rules.insert(number, conditions);
    }
    let filter = SeccompFilter::new(
        rules,
        SeccompAction::KillProcess,
        SeccompAction::Allow,
        TargetArch::x86_64,
    )
    .ok()?;
    filter.try_into().ok()
}

/// Installs the seccomp filter. `fail` installs a program the kernel refuses instead, as a host
/// whose seccomp cannot take the filter would (S-04.3).
fn seal(channel: RawFd, vcpu: RawFd, trace: bool, fail: bool) -> bool {
    let program = if fail {
        // An instruction code no BPF machine has.
        vec![seccompiler::sock_filter {
            code: 0xffff,
            jt: 0,
            jf: 0,
            k: 0,
        }]
    } else {
        match filter(channel, vcpu, trace) {
            Some(program) => program,
            None => return false,
        }
    };
    seccompiler::apply_filter(&program).is_ok()
}

/// `task-kernel machine-runner [--check | --trace]`, the runner's whole life (ADR-0132). It closes
/// what it inherited and opens KVM before any byte of the run arrives, gives up its file rights
/// with Landlock, then reads `Boot`, builds the machine, installs its seccomp filter, and only then
/// runs it and sends how it ended. With `--check` it exits after both confinements hold: `0`, or
/// `NO_KVM`, `NO_LANDLOCK` or `NO_SECCOMP`. `--trace` prints the machine's timings, for
/// `HYALOS_TRACE`. Any other failure exits `BROKEN`, and a run whose runner cannot confine itself
/// ends in `KernelFault` before any guest instruction runs.
pub(crate) fn command(options: &Options) -> u8 {
    if !close_inherited() {
        return BROKEN as u8;
    }
    let kvm = kvm_ioctls::Kvm::new();
    if options.check && kvm.is_err() {
        return NO_KVM as u8;
    }
    let Ok(channel) = std::io::stdin().as_fd().try_clone_to_owned() else {
        return BROKEN as u8;
    };
    let mut channel = Channel(UnixStream::from(channel));
    let fd = channel.0.as_raw_fd();
    let seccomp_fails = options.fails(Confinement::Seccomp);
    // Confined before any byte of the run arrives.
    if !landlock(options.fails(Confinement::Landlock)) {
        if !options.check {
            let _ = channel
                .0
                .write_all(&Request::Failed(Reason::KernelFault).encode());
        }
        return NO_LANDLOCK as u8;
    }
    if options.check {
        // The filter the run would install, which leaves this runner nothing but its exit.
        return if seal(fd, -1, false, seccomp_fails) {
            0
        } else {
            NO_SECCOMP as u8
        };
    }
    let Ok((BOOT, fields)) = read_frame(&mut channel.0, BOOT_LIMIT, None) else {
        return BROKEN as u8;
    };
    let Some((kernel, guest)) = split_boot(&fields) else {
        return BROKEN as u8;
    };
    let trace = options.trace;
    kvm::machine_run(
        kvm.ok(),
        kernel,
        guest,
        &mut channel,
        trace,
        |vcpu| {
            let sealed = || seal(fd, vcpu.as_raw_fd(), trace, seccomp_fails);
            // A hostile runner attempts what it may not once sealed, and never runs the machine.
            #[cfg(feature = "test-switches")]
            if let Some(act) = options.hostile {
                hostile::attempt(act, guest, sealed);
            }
            sealed()
        },
        |channel, outcome| {
            let ended = match outcome {
                Ok((result, fuel)) => Request::Finished(result, fuel),
                Err(reason) => Request::Failed(reason),
            };
            let _ = channel.0.write_all(&ended.encode());
        },
    );
    0
}

/// The hostile runner (S-04.2), in a build with the `test-switches` feature only: a runner taken
/// over by code that, once the machine is built and the runner sealed, attempts one thing it may
/// not, at the target the `Boot` frame's guest bytes name. It never runs the machine. Killed by
/// `SIGSYS` is the expected end; an attempt that returns exits `SURVIVED`.
#[cfg(feature = "test-switches")]
pub(crate) mod hostile {
    use std::{
        ffi::OsStr,
        os::unix::{ffi::OsStrExt, net::UnixStream, process::CommandExt},
        process::Command,
    };

    /// The exit code of a hostile runner whose attempt returned instead of killing it.
    pub(crate) const SURVIVED: i32 = 6;
    /// The exit code of a hostile runner whose attempt Landlock refused.
    pub(crate) const DENIED: i32 = 5;
    /// The exit code of a hostile runner that could not seal itself.
    pub(crate) const UNSEALED: i32 = 7;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Act {
        /// Open the directory the target names, such as the operator's home.
        Open,
        /// Create the file the target names.
        Create,
        /// Connect to the Unix socket the target names.
        Connect,
        /// Start `/bin/sh` to create the file the target names.
        Spawn,
        /// Replace itself with `/bin/sh`, which would create the file the target names.
        Exec,
        /// Kill the process whose number the target is.
        Signal,
        /// Map memory it could execute.
        MapExecutable,
        /// Create the file the target names under Landlock alone, before the seccomp filter.
        CreateUnderLandlock,
    }

    pub(crate) const ACTS: [(&str, Act); 8] = [
        ("open", Act::Open),
        ("create", Act::Create),
        ("connect", Act::Connect),
        ("spawn", Act::Spawn),
        ("exec", Act::Exec),
        ("signal", Act::Signal),
        ("map-executable", Act::MapExecutable),
        ("create-under-landlock", Act::CreateUnderLandlock),
    ];

    impl Act {
        pub(crate) fn named(name: &str) -> Option<Self> {
            ACTS.iter().find(|(n, _)| *n == name).map(|(_, act)| *act)
        }
    }

    /// Makes the attempt and exits. Everything the attempt needs is prepared before `seal`, so the
    /// attempt's first system call is the one under test.
    pub(crate) fn attempt(act: Act, target: &[u8], seal: impl FnOnce() -> bool) -> ! {
        let path = OsStr::from_bytes(target);
        if act == Act::CreateUnderLandlock {
            let created = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path);
            std::process::exit(match created {
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => DENIED,
                _ => SURVIVED,
            });
        }
        let mut shell = Command::new("/bin/sh");
        shell.arg("-c").arg(": > \"$0\"").arg(path);
        let pid: Option<libc::pid_t> = std::str::from_utf8(target)
            .ok()
            .and_then(|t| t.parse().ok());
        if !seal() {
            std::process::exit(UNSEALED);
        }
        match act {
            Act::Open => drop(std::fs::File::open(path)),
            Act::Create => drop(std::fs::File::create(path)),
            Act::Connect => drop(UnixStream::connect(path)),
            Act::Spawn => drop(shell.spawn()),
            Act::Exec => drop(shell.exec()),
            Act::Signal => {
                if let Some(pid) = pid {
                    // SAFETY: a plain system call with plain arguments; this is the attempt.
                    unsafe { libc::kill(pid, libc::SIGKILL) };
                }
            }
            Act::MapExecutable => {
                let (read_exec, private) = (
                    libc::PROT_READ | libc::PROT_EXEC,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                );
                // SAFETY: a fresh anonymous mapping that nothing uses; this is the attempt.
                unsafe { libc::mmap(std::ptr::null_mut(), 4096, read_exec, private, -1, 0) };
            }
            Act::CreateUnderLandlock => {}
        }
        std::process::exit(SURVIVED)
    }
}
