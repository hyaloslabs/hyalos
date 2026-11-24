//! The machine runner and its channel (S-04.1, S-04.4 and 6.1 of `docs/plans/hyalos-kernel-core`).
//!
//! The channel tests drive the holder's side (`runner::serve`) from a hand-built peer on a socket
//! pair, so they need no KVM: they show what the holder does with a hostile runner. The runner tests
//! start the real `machine-runner` from the `task-kernel` binary beside this harness, and need KVM;
//! `HYALOS_REQUIRE_KVM` turns a host without it into a failure.

use super::*;
use crate::profile_tests::{kvm_backend, selftest_kvm_backend};
use crate::runner::{self, Reply, Request, Timing};
use std::{
    io::{Read as _, Write as _},
    os::unix::net::UnixStream,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}

fn fixture_inputs() -> Vec<Vec<u8>> {
    read_inputs(
        root()
            .join("evidence/fixtures/issues.tsv")
            .to_str()
            .unwrap(),
    )
    .unwrap()
}

/// What a source was asked to do, shared with the test that owns it.
#[derive(Clone, Default)]
struct Counts {
    infers: Arc<AtomicUsize>,
    effects: Arc<AtomicUsize>,
}

impl Counts {
    fn infers(&self) -> usize {
        self.infers.load(Ordering::SeqCst)
    }
    fn effects(&self) -> usize {
        self.effects.load(Ordering::SeqCst)
    }
}

/// A source that counts what it is asked, answers like the stand-in model, and runs `on_infer`
/// before each answer with the number of the call.
struct Counting {
    counts: Counts,
    on_infer: Box<dyn FnMut(usize)>,
}

impl Counting {
    fn new(counts: &Counts) -> Self {
        Self {
            counts: counts.clone(),
            on_infer: Box::new(|_| {}),
        }
    }
}

impl Source for Counting {
    fn infer(&mut self, _: usize, _: i32, question: &[u8], _: &str) -> Result<Asked, Reason> {
        let call = self.counts.infers.fetch_add(1, Ordering::SeqCst);
        (self.on_infer)(call);
        Ok(Asked {
            label: stub_model(question, 0),
            model: "stub".into(),
            response_id: String::new(),
            attestation: String::new(),
            provider_proofs: Vec::new(),
        })
    }

    fn effect(&mut self, _: usize, _: i32, _: &[u8], _: &str) -> Result<String, Reason> {
        self.counts.effects.fetch_add(1, Ordering::SeqCst);
        Ok(String::new())
    }
}

/// Plays a runner that sends `frames` and then closes its end, against the holder's side of a run
/// with one input. Returns how the holder ended the run and what its source was asked.
fn holder_against(frames: &[Vec<u8>]) -> (Result<(Vec<u8>, u64), Reason>, Counts) {
    holder_against_with(frames, Duration::from_secs(10))
}

fn holder_against_with(
    frames: &[Vec<u8>],
    limit: Duration,
) -> (Result<(Vec<u8>, u64), Reason>, Counts) {
    let (mut holder, mut peer) = UnixStream::pair().unwrap();
    let stream: Vec<u8> = frames.concat();
    // The peer reads the holder's replies too, so neither side's buffer fills.
    let writer = std::thread::spawn(move || {
        let _ = peer.write_all(&stream);
        let _ = peer.shutdown(std::net::Shutdown::Write);
        let mut replies = Vec::new();
        let _ = peer.read_to_end(&mut replies);
    });
    let counts = Counts::default();
    let host = hyalos_audit::Host::new(Counting::new(&counts), vec![b"one input".to_vec()]);
    let outcome = runner::serve(
        &mut holder,
        b"boot",
        host,
        Instant::now() + limit,
        &mut Timing::default(),
    )
    .map(|(_, result, fuel)| (result, fuel));
    drop(holder);
    writer.join().unwrap();
    (outcome, counts)
}

/// A frame with a length field of `len` and `body` after it, whatever the two say.
fn raw(len: u32, body: &[u8]) -> Vec<u8> {
    [&len.to_le_bytes()[..], body].concat()
}

fn effect(body_len: usize) -> Vec<u8> {
    Request::Effect(0, vec![b'x'; body_len]).encode()
}

/// S-04.4.e1: a frame over the runner's limit is refused before its body is read, and no effect
/// follows, though the frame before it asked for one. The largest frame a run needs, an effect body
/// at its 65,536-byte limit, is read and performed.
#[test]
fn core_s_04_4_e1_a_message_over_the_size_limit_is_refused() {
    let pre = Request::PreEffect(0).encode();
    let largest = effect(hyalos_audit::MAX_BODY);
    assert!(largest.len() - 4 <= runner::REQUEST_LIMIT);
    let (outcome, counts) = holder_against(&[pre.clone(), largest]);
    assert_eq!(
        (outcome, counts.effects()),
        (Err(Reason::KernelFault), 1),
        "the largest effect is performed, then the closed channel ends the run"
    );

    let over = raw(
        runner::REQUEST_LIMIT as u32 + 1,
        &[runner::VERSION, runner::EFFECT, 0, 0, 0, 0],
    );
    let (outcome, counts) = holder_against(&[pre, over]);
    assert_eq!((outcome, counts.effects()), (Err(Reason::KernelFault), 0));
    for len in [0, 1, u32::MAX] {
        let (outcome, counts) =
            holder_against(&[raw(len, &[runner::VERSION, runner::INPUT, 0, 0, 0, 0])]);
        assert_eq!(
            (outcome, counts.effects()),
            (Err(Reason::KernelFault), 0),
            "length {len}"
        );
    }
}

/// S-04.4.e1, the runner's side: a `Boot` frame over its 8 MiB limit, or a reply over the heap's
/// size, is refused by the reader before its body is read.
#[test]
fn core_s_04_4_e1_the_runner_refuses_a_boot_or_reply_over_its_limit() {
    for (limit, kind) in [
        (runner::BOOT_LIMIT, runner::BOOT),
        (runner::REPLY_LIMIT, runner::REPLY),
    ] {
        let (mut reader, mut writer) = UnixStream::pair().unwrap();
        writer
            .write_all(&raw(limit as u32 + 1, &[runner::VERSION, kind]))
            .unwrap();
        drop(writer);
        assert_eq!(
            runner::read_frame(&mut reader, limit, None),
            Err(runner::Broken::Fault)
        );
        let (mut reader, mut writer) = UnixStream::pair().unwrap();
        let at_limit = runner::frame(kind, &[&vec![0; limit - 2]]);
        let writing = std::thread::spawn(move || writer.write_all(&at_limit).unwrap());
        let read = runner::read_frame(&mut reader, limit, None);
        writing.join().unwrap();
        assert_eq!(
            read.map(|(k, f)| (k, f.len())),
            Ok((kind, limit - 2)),
            "a frame at the limit is read"
        );
    }
    // The holder never builds a `Boot` frame over the limit: the run is refused without a runner.
    let counts = Counts::default();
    let host = hyalos_audit::Host::new(Counting::new(&counts), vec![]);
    let big = vec![0u8; runner::BOOT_LIMIT];
    assert_eq!(
        runner::run(&big, &[], host, Duration::from_secs(5)).err(),
        Some(Reason::KernelFault)
    );
}

/// S-04.4.e2: a second effect request is refused by `Host`'s single-effect rule, as on the process
/// backend, and a second effect frame after one type check is refused as out of turn.
#[test]
fn core_s_04_4_e2_a_second_effect_request_is_refused() {
    let pre = Request::PreEffect(0).encode();
    let (outcome, counts) = holder_against(&[pre.clone(), effect(1), pre.clone()]);
    assert_eq!((outcome, counts.effects()), (Err(Reason::EffectLimit), 1));
    let (outcome, counts) = holder_against(&[pre, effect(1), effect(1)]);
    assert_eq!((outcome, counts.effects()), (Err(Reason::KernelFault), 1));
}

/// S-04.4: a malformed frame ends the run with `KernelFault`, and no effect follows, though the
/// frame before it opened the way to one.
#[test]
fn core_s_04_4_a_malformed_message_ends_the_run_with_no_effect() {
    let v = runner::VERSION;
    let malformed: Vec<(&str, Vec<u8>)> = vec![
        ("another version", raw(6, &[2, runner::EFFECT, 0, 0, 0, 0])),
        ("type 0", runner::frame(0, &[&[0; 4]])),
        ("type 10", runner::frame(10, &[&[0; 4]])),
        (
            "a Boot from the runner",
            runner::frame(runner::BOOT, &[&[0; 8]]),
        ),
        ("a reply from the runner", Reply::Ok.encode()),
        (
            "a type check with three bytes",
            runner::frame(runner::PRE_EFFECT, &[&[0; 3]]),
        ),
        (
            "a type check with five bytes",
            runner::frame(runner::PRE_EFFECT, &[&[0; 5]]),
        ),
        (
            "an effect with no kind",
            runner::frame(runner::EFFECT, &[&[0; 2]]),
        ),
        (
            "a result with no fuel",
            runner::frame(runner::FINISHED, &[&[0; 7]]),
        ),
        (
            "a reason outside the table",
            runner::frame(runner::FAILED, &[&[runner::REASONS.len() as u8]]),
        ),
        (
            "a reason of two bytes",
            runner::frame(runner::FAILED, &[&[13, 13]]),
        ),
        (
            "a truncated frame",
            raw(70, &[v, runner::EFFECT, 0, 0, 0, 0, b'x']),
        ),
    ];
    for (name, frame) in malformed {
        let (outcome, counts) = holder_against(&[Request::PreEffect(0).encode(), frame]);
        assert_eq!(
            (outcome, counts.effects()),
            (Err(Reason::KernelFault), 0),
            "{name}"
        );
    }
    // A runner may end a run only with a reason the machine gives. The rest are the holder's.
    for reason in runner::REASONS
        .iter()
        .filter(|r| !runner::RUNNER_REASONS.contains(r))
    {
        let (outcome, _) = holder_against(&[Request::Failed(*reason).encode()]);
        assert_eq!(outcome, Err(Reason::KernelFault), "{reason:?}");
    }
    for reason in runner::RUNNER_REASONS {
        assert_eq!(
            holder_against(&[Request::Failed(reason).encode()]).0,
            Err(reason)
        );
    }
    // The holder checks every length against `Host`'s limits again.
    let (outcome, counts) = holder_against(&[
        Request::PreEffect(0).encode(),
        effect(hyalos_audit::MAX_BODY + 1),
    ]);
    assert_eq!(
        (outcome, counts.effects()),
        (Err(Reason::KernelFault), 0),
        "a body over its limit"
    );
    let question = Request::Infer(0, vec![b'q'; hyalos_audit::MAX_QUESTION + 1]).encode();
    let (outcome, counts) = holder_against(&[Request::PreInfer(0).encode(), question]);
    assert_eq!(
        (outcome, counts.infers()),
        (Err(Reason::KernelFault), 0),
        "a question over its limit"
    );
    let result = Request::Finished(vec![0; MAX_RESULT + 1], 1).encode();
    assert_eq!(
        holder_against(&[result]).0,
        Err(Reason::KernelFault),
        "a result over its limit"
    );
    assert_eq!(
        holder_against(&[Request::Finished(vec![7; MAX_RESULT], 9).encode()]).0,
        Ok((vec![7; MAX_RESULT], 9)),
        "a result at its limit"
    );
}

/// S-04.4: a frame out of turn ends the run with `KernelFault` and performs nothing.
#[test]
fn core_s_04_4_an_out_of_order_message_ends_the_run_with_no_effect() {
    let cases: Vec<(&str, Vec<Vec<u8>>)> = vec![
        ("an effect with no type check", vec![effect(1)]),
        (
            "a question with no type check",
            vec![Request::Infer(0, b"q".to_vec()).encode()],
        ),
        (
            "an effect after a question's type check",
            vec![Request::PreInfer(0).encode(), effect(1)],
        ),
        (
            "a question of another type",
            vec![
                Request::PreInfer(0).encode(),
                Request::Infer(1, b"q".to_vec()).encode(),
            ],
        ),
        (
            "an effect of another kind",
            vec![
                Request::PreEffect(0).encode(),
                Request::Effect(1, b"x".to_vec()).encode(),
            ],
        ),
        (
            "an input read between check and effect",
            vec![
                Request::PreEffect(0).encode(),
                Request::Input(0).encode(),
                effect(1),
            ],
        ),
        (
            "two type checks",
            vec![
                Request::PreEffect(0).encode(),
                Request::PreEffect(0).encode(),
                effect(1),
            ],
        ),
        (
            "a result between check and effect",
            vec![
                Request::PreEffect(0).encode(),
                Request::Finished(vec![], 0).encode(),
            ],
        ),
        (
            "a machine fault between check and effect",
            vec![
                Request::PreEffect(0).encode(),
                Request::Failed(Reason::KernelFault).encode(),
            ],
        ),
    ];
    for (name, frames) in cases {
        let (outcome, counts) = holder_against(&frames);
        assert_eq!(
            (outcome, counts.effects(), counts.infers()),
            (Err(Reason::KernelFault), 0, 0),
            "{name}"
        );
    }
    // Between a check and the bytes, the runner may refuse the bytes as `answer` does.
    for reason in [Reason::TooLarge, Reason::BadPointer] {
        let (outcome, counts) = holder_against(&[
            Request::PreEffect(0).encode(),
            Request::Failed(reason).encode(),
        ]);
        assert_eq!((outcome, counts.effects()), (Err(reason), 0));
    }
}

/// S-04.4: a frame the holder's rules refuse, such as an unknown effect kind, is refused with
/// `Host`'s reason and nothing is performed.
#[test]
fn core_s_04_4_the_holder_keeps_hosts_rules() {
    assert_eq!(
        holder_against(&[Request::PreEffect(7).encode(), effect(1)]).0,
        Err(Reason::UnknownEffect)
    );
    assert_eq!(
        holder_against(&[Request::PreInfer(9).encode()]).0,
        Err(Reason::UnknownSchema)
    );
    let (outcome, counts) = holder_against(&[
        Request::Input(0).encode(),
        Request::Input(5).encode(),
        Request::PreInfer(0).encode(),
        Request::Infer(0, b"a bug".to_vec()).encode(),
        Request::Finished(b"done".to_vec(), 3).encode(),
    ]);
    assert_eq!((outcome, counts.infers()), (Ok((b"done".to_vec(), 3)), 1));
}

/// S-04.4: the holder holds the deadline. A runner that says nothing, or drips a frame one byte at
/// a time, ends with `Timeout` at the deadline, not later.
#[test]
fn core_s_04_4_the_holder_ends_a_silent_or_slow_runner_at_the_deadline() {
    let (mut holder, peer) = UnixStream::pair().unwrap();
    let counts = Counts::default();
    let started = Instant::now();
    let host = hyalos_audit::Host::new(Counting::new(&counts), vec![]);
    assert_eq!(
        runner::serve(
            &mut holder,
            b"boot",
            host,
            started + Duration::from_millis(300),
            &mut Timing::default()
        )
        .err(),
        Some(Reason::Timeout)
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    drop(peer);

    let (mut holder, mut peer) = UnixStream::pair().unwrap();
    let drip = std::thread::spawn(move || {
        for byte in Request::PreEffect(0).encode() {
            if peer.write_all(&[byte]).is_err() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        std::thread::sleep(Duration::from_secs(1));
    });
    let started = Instant::now();
    let host = hyalos_audit::Host::new(Counting::new(&counts), vec![]);
    assert_eq!(
        runner::serve(
            &mut holder,
            b"boot",
            host,
            started + Duration::from_millis(350),
            &mut Timing::default()
        )
        .err(),
        Some(Reason::Timeout)
    );
    assert!(
        started.elapsed() < Duration::from_millis(800),
        "the deadline is the whole run's, not each read's"
    );
    drop(holder);
    drip.join().unwrap();
}

/// S-04.4: every reason has one byte in a closed table, whose order is pinned here and not taken
/// from the enum, and every message survives its own encoding.
#[test]
fn core_s_04_4_the_reason_table_is_closed_and_every_message_round_trips() {
    let names: Vec<String> = runner::REASONS.iter().map(|r| format!("{r:?}")).collect();
    assert_eq!(
        names.join(" "),
        "InvalidModule BadImport BadExport OutOfFuel GuestTrap BadPointer TooLarge UnknownSchema UnknownEffect EffectLimit NoResult ReplayDiverged GuestMismatch KernelFault BadHypercall Timeout NoKvm ModelUnavailable ModelRefused NotaryUnreachable NotaryDeadline WrongNotaryKey ReleaseMismatch ProviderRefused NoAuthorisation AttemptsSpent CanaryHeld EffectFailed LedgerFailed"
    );
    // A new reason fails to compile here until it has a byte.
    for reason in runner::REASONS {
        match reason {
            Reason::InvalidModule
            | Reason::BadImport
            | Reason::BadExport
            | Reason::OutOfFuel
            | Reason::GuestTrap
            | Reason::BadPointer
            | Reason::TooLarge
            | Reason::UnknownSchema
            | Reason::UnknownEffect
            | Reason::EffectLimit
            | Reason::NoResult
            | Reason::ReplayDiverged
            | Reason::GuestMismatch
            | Reason::KernelFault
            | Reason::BadHypercall
            | Reason::Timeout
            | Reason::NoKvm
            | Reason::ModelUnavailable
            | Reason::ModelRefused
            | Reason::NotaryUnreachable
            | Reason::NotaryDeadline
            | Reason::WrongNotaryKey
            | Reason::ReleaseMismatch
            | Reason::ProviderRefused
            | Reason::NoAuthorisation
            | Reason::AttemptsSpent
            | Reason::CanaryHeld
            | Reason::EffectFailed
            | Reason::LedgerFailed => {}
        }
    }
    let decode = |bytes: &[u8]| (bytes[5], bytes[6..].to_vec());
    let requests = [
        Request::Input(-1),
        Request::PreInfer(0),
        Request::PreEffect(7),
        Request::Infer(0, b"q".to_vec()),
        Request::Effect(0, vec![]),
        Request::Finished(b"r".to_vec(), u64::MAX),
        Request::Failed(Reason::LedgerFailed),
    ];
    for (at, request) in requests.iter().enumerate() {
        let bytes = request.encode();
        assert_eq!(
            (bytes[4], bytes[5]),
            (runner::VERSION, runner::INPUT + at as u8),
            "{request:?}"
        );
        let (kind, fields) = decode(&bytes);
        assert_eq!(Request::decode(kind, &fields).as_ref(), Some(request));
    }
    let big = runner::REPLY_LIMIT as u64;
    for reply in [
        Reply::Record(3, b"abc".to_vec()),
        Reply::Record(big, vec![]),
        Reply::NoRecord,
        Reply::Limit(4096),
        Reply::Label(3),
        Reply::Ok,
        Reply::Refused(Reason::EffectLimit),
    ] {
        let (kind, fields) = decode(&reply.encode());
        assert_eq!(Reply::decode(kind, &fields), Some(reply));
    }
    // A record's bytes must match its length, unless it is too long for the heap.
    assert_eq!(
        Reply::decode(
            runner::REPLY,
            &[&[0][..], &4u64.to_le_bytes(), b"abc"].concat()
        ),
        None
    );
    assert_eq!(
        Reply::decode(runner::REPLY, &[&[0][..], &3u64.to_le_bytes()].concat()),
        None
    );
}

/// S-04.4, fuzzed: random streams of frames, well formed and not, from a fixed seed. The holder
/// never panics, performs at most one effect, and only after a type check of that kind, and every
/// refusal is a reason a run already gives.
#[test]
fn core_s_04_4_a_fuzzed_channel_never_panics_or_performs_an_unasked_effect() {
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let allowed = [
        &runner::RUNNER_REASONS[..],
        &[
            Reason::UnknownSchema,
            Reason::UnknownEffect,
            Reason::EffectLimit,
        ],
    ]
    .concat();
    let (mut performed, mut finished, mut asked) = (0, 0, 0);
    for round in 0..1500 {
        let frames: Vec<Vec<u8>> = (0..next() % 8)
            .map(|_| {
                let small = (next() % 3) as i32 - 1 + (next() % 2) as i32;
                let bytes = vec![b'x'; [0, 1, 4096, 4097, 65_536, 65_537][(next() % 6) as usize]];
                match next() % 10 {
                    0 => Request::Input(small).encode(),
                    1 => Request::PreInfer(small).encode(),
                    2 => Request::PreEffect(small).encode(),
                    3 => Request::Infer(small, bytes).encode(),
                    4 => Request::Effect(small, bytes).encode(),
                    5 => Request::Finished(bytes, next()).encode(),
                    6 => Request::Failed(runner::REASONS[(next() % 29) as usize]).encode(),
                    7 => {
                        let mut frame = Request::PreEffect(0).encode();
                        let at = (next() as usize) % frame.len();
                        frame[at] ^= 1 << (next() % 8);
                        frame
                    }
                    8 => (0..next() % 16).map(|_| next() as u8).collect(),
                    _ => raw(next() as u32, &[runner::VERSION, (next() % 12) as u8]),
                }
            })
            .collect();
        let (outcome, counts) = holder_against(&frames);
        assert!(counts.effects() <= 1, "round {round}");
        performed += counts.effects();
        asked += usize::from(counts.infers() > 0);
        finished += usize::from(outcome.is_ok());
        if let Err(reason) = outcome {
            assert!(allowed.contains(&reason), "round {round}: {reason:?}");
        }
        if counts.effects() == 1 {
            let opened = frames.windows(2).any(|w| {
                w[0] == Request::PreEffect(0).encode()
                    && w[1].get(4..6) == Some(&[runner::VERSION, runner::EFFECT][..])
            });
            assert!(
                opened,
                "round {round}: an effect with no type check before it"
            );
        }
    }
    // The streams reach the holder's every way out, not only its first check.
    assert!(
        performed > 0 && finished > 0 && asked > 0,
        "{performed} effects, {finished} finished, {asked} asked"
    );
}

/// The entries of `/proc/<pid>/fd`, by where each points.
fn open_files(pid: u32) -> Vec<String> {
    let mut found: Vec<String> = fs::read_dir(format!("/proc/{pid}/fd"))
        .unwrap()
        .map(|e| {
            fs::read_link(e.unwrap().path())
                .unwrap()
                .display()
                .to_string()
        })
        .collect();
    found.sort();
    found
}

/// What a runner holds while its machine waits on a request.
struct Seen {
    files: Vec<String>,
    environ: Vec<u8>,
    cwd: String,
    parent: Option<u32>,
    threads: usize,
}

fn seen(pid: u32) -> Seen {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
    let parent = status
        .lines()
        .find_map(|l| l.strip_prefix("PPid:"))
        .and_then(|p| p.trim().parse().ok());
    Seen {
        files: open_files(pid),
        environ: fs::read(format!("/proc/{pid}/environ")).unwrap(),
        cwd: fs::read_link(format!("/proc/{pid}/cwd"))
            .unwrap()
            .display()
            .to_string(),
        parent,
        threads: fs::read_dir(format!("/proc/{pid}/task")).unwrap().count(),
    }
}

/// S-04.1.e1: while the committed digest guest waits on its first question, its virtual CPU is in
/// a child process, not this one, which has an empty environment, `/` as its directory, and holds
/// nothing but its KVM handles, the channel and the null device on its output and error.
#[test]
fn core_s_04_1_e1_the_vcpu_runs_in_a_child_with_only_its_kvm_handles_and_the_channel_open() {
    let Some(Backend::Kvm { kernel, .. }) = kvm_backend("core_s_04_1_e1") else {
        return;
    };
    let guest = fs::read(root().join("guest/digest.wasm")).unwrap();
    let runner::Runner { child, mut stream } = runner::start(&[]).unwrap();
    let pid = child.id();
    let counts = Counts::default();
    let found: Arc<std::sync::Mutex<Option<Seen>>> = Arc::default();
    let mut source = Counting::new(&counts);
    let into = found.clone();
    source.on_infer = Box::new(move |call| {
        if call == 0 {
            *into.lock().unwrap() = Some(seen(pid));
        }
    });
    let boot = runner::boot(&kernel, &guest).unwrap();
    let outcome = runner::serve(
        &mut stream,
        &boot,
        hyalos_audit::Host::new(source, fixture_inputs()),
        Instant::now() + Duration::from_secs(30),
        &mut Timing::default(),
    );
    runner::Runner { child, stream }.end();
    let (_, result, _) = outcome.unwrap();
    assert!(!result.is_empty());
    let seen = found.lock().unwrap().take().unwrap();
    assert_ne!(pid, std::process::id());
    assert_eq!(
        seen.parent,
        Some(std::process::id()),
        "the runner is this process's child"
    );
    assert!(seen.environ.is_empty(), "the runner's environment is empty");
    assert_eq!(seen.cwd, "/");
    assert_eq!(
        seen.threads, 1,
        "one thread, the one running the virtual CPU"
    );
    let socket = seen
        .files
        .iter()
        .find(|f| f.starts_with("socket:"))
        .unwrap()
        .clone();
    let mut want = vec![
        "/dev/kvm".to_string(),
        "/dev/null".into(),
        "/dev/null".into(),
        "anon_inode:kvm-vcpu:0".into(),
        "anon_inode:kvm-vm".into(),
        socket.clone(),
        socket,
    ];
    want.sort();
    assert_eq!(
        seen.files, want,
        "standard input and its copy are the one channel"
    );
}

/// S-04.1: every KVM run goes through the runner. The KVM backend calls `runner::run`, and only
/// the `machine-runner` command builds a machine for a run: `kvm::machine_run` has one caller, and
/// the only other machine in the monitor is the host probe's fixed sequences.
#[test]
fn core_s_04_1_every_kvm_run_executes_in_a_machine_runner() {
    let main = include_str!("main.rs");
    let kvm = include_str!("kvm.rs");
    let runner_source = include_str!("runner.rs");
    assert!(
        main.contains("Backend::Kvm { kernel, limit } => runner::run(kernel, guest, host, *limit)")
    );
    let calls = [
        main,
        kvm,
        include_str!("probe_host.rs"),
        include_str!("live.rs"),
        include_str!("ledger.rs"),
    ]
    .iter()
    .map(|s| s.matches("machine_run(").count())
    .sum::<usize>();
    assert_eq!(
        (calls, runner_source.matches("kvm::machine_run(").count()),
        (0, 1),
        "the runner's command is its one caller"
    );
    assert_eq!(
        kvm.matches(".run()").count(),
        2,
        "the runner's machine and the host probe's"
    );
    assert_eq!(
        kvm.matches("Ticker::start()").count(),
        1,
        "only the host probe keeps a signal timer"
    );
}

/// S-04.1.e2: the runner is killed while the guest waits on its third question. The holder sees
/// the channel close and ends the run with `KernelFault`, and the effect the guest would have sent
/// after its questions never happens.
#[test]
fn core_s_04_1_e2_a_runner_that_exits_mid_run_ends_with_a_refusal_and_no_effect() {
    let Some(Backend::Kvm { kernel, .. }) = kvm_backend("core_s_04_1_e2") else {
        return;
    };
    let guest = fs::read(root().join("guest/digest.wasm")).unwrap();
    let runner::Runner { child, mut stream } = runner::start(&[]).unwrap();
    let child: Arc<std::sync::Mutex<Child>> = Arc::new(std::sync::Mutex::new(child));
    let counts = Counts::default();
    let mut source = Counting::new(&counts);
    let victim = child.clone();
    source.on_infer = Box::new(move |call| {
        if call == 2 {
            let mut child = victim.lock().unwrap();
            child.kill().unwrap();
            child.wait().unwrap();
        }
    });
    let boot = runner::boot(&kernel, &guest).unwrap();
    let started = Instant::now();
    let outcome = runner::serve(
        &mut stream,
        &boot,
        hyalos_audit::Host::new(source, fixture_inputs()),
        started + Duration::from_secs(30),
        &mut Timing::default(),
    );
    assert_eq!(outcome.err(), Some(Reason::KernelFault));
    assert_eq!((counts.infers(), counts.effects()), (3, 0));
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the holder does not wait for the deadline"
    );
    // The same guest with nothing killed asks 40 questions and sends its one effect.
    let counts = Counts::default();
    let Ok((host, _, _)) = runner::run(
        &kernel,
        &guest,
        hyalos_audit::Host::new(Counting::new(&counts), fixture_inputs()),
        Duration::from_secs(30),
    ) else {
        panic!("the control run failed")
    };
    assert_eq!(
        (counts.infers(), counts.effects(), host.effects.len()),
        (40, 1, 1)
    );
}

/// S-04.1.e2, a runner that closes the channel between an effect's type check and its bytes.
#[test]
fn core_s_04_1_e2_a_channel_that_closes_mid_run_performs_no_effect() {
    let (outcome, counts) =
        holder_against(&[Request::Input(0).encode(), Request::PreEffect(0).encode()]);
    assert_eq!((outcome, counts.effects()), (Err(Reason::KernelFault), 0));
}

/// S-04.1: the deadline reaches a kernel that spins, now that the holder holds it, under the test
/// harness too (Slice 2 could only leave this case to the `attacks` command).
#[test]
fn core_s_04_1_a_kernel_that_spins_is_stopped_at_the_deadline() {
    if kvm_backend("core_s_04_1 spin").is_none() {
        return;
    }
    let (_, spin, want) = hostile_kernels()
        .into_iter()
        .find(|(name, _, _)| *name == "spin forever")
        .unwrap();
    assert_eq!(want, Reason::Timeout);
    let started = Instant::now();
    let hostile = Backend::Kvm {
        kernel: spin,
        limit: Duration::from_secs(1),
    };
    assert_eq!(
        execute(&hostile, &[], vec![], Mode::Record { seed: 0 }).err(),
        Some(Reason::Timeout)
    );
    let took = started.elapsed();
    assert!(
        took >= Duration::from_secs(1) && took < Duration::from_secs(3),
        "{took:?}"
    );
}

/// S-04.1: `machine-runner --check` starts a runner and opens KVM, and a `live` run asks it
/// before its chain is touched. On this host KVM opens, so the run passes the check and stops at
/// the next step, the missing home; on a host without KVM it is refused by the check instead.
#[test]
fn core_s_04_1_a_live_run_checks_the_runner_before_its_chain() {
    let usable = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/kvm")
        .is_ok();
    assert_eq!(runner::check().is_ok(), usable);
    let dir = env::temp_dir().join(format!("hyalos-runner-check-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let program = env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("task-kernel");
    let output = Command::new(&program)
        .arg("live")
        .arg(root().join("guest/digest.wasm"))
        .arg(root().join("evidence/fixtures/issues.tsv"))
        .arg(dir.join("record.json"))
        .arg("octo/real")
        .arg("1")
        .current_dir(&dir)
        .env_clear()
        .env("HYALOS_KERNEL", root().join("guest/digest.wasm"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let want = if usable {
        "refused: no home"
    } else {
        "refused: the machine runner cannot open KVM"
    };
    assert!(said == want, "the live run did not stop where expected");
    assert!(!dir.join("ledger").exists());
    let _ = fs::remove_dir_all(&dir);
    // The command takes nothing but `--check` or `--trace`.
    for args in [&["--other"][..], &["--check", "x"]] {
        let status = Command::new(&program)
            .arg("machine-runner")
            .args(args)
            .env_clear()
            .stdin(Stdio::null())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(2));
    }
}

/// S-04.4: a tracing runner's standard error never reaches the holder's. The holder prints the
/// runner's timings only from the exact line the runner prints, parsed into numbers, and anything
/// else, such as a terminal escape or a second line, prints nothing of it.
#[test]
fn core_s_04_4_the_holder_reprints_only_a_well_formed_trace_line() {
    let line = b"trace ms: memory+load 0.95, vm setup 0.71, boot to first call 2.03\n";
    assert_eq!(runner::runner_trace(line), Some([0.95, 0.71, 2.03]));
    for hostile in [
        &b"trace ms: memory+load 0.95, vm setup 0.71, boot to first call 2.03"[..],
        b"trace ms: memory+load 0.95, vm setup 0.71, boot to first call 2.03\n\x1b]0;owned\x07\n",
        b"trace ms: memory+load \x1b[2J, vm setup 0.71, boot to first call 2.03\n",
        b"trace ms: memory+load -1, vm setup 0.71, boot to first call 2.03\n",
        b"trace ms: memory+load 1e9, vm setup 0.71, boot to first call 2.03\n",
        b"trace ms: memory+load , vm setup 0.71, boot to first call 2.03\n",
        b"trace ms: memory+load 0.95, vm setup 0.71, boot to first call 2.03, extra 1\n",
        b"\xff\n",
        b"",
    ] {
        assert_eq!(
            runner::runner_trace(hostile),
            None,
            "{:?}",
            String::from_utf8_lossy(hostile)
        );
    }

    // A real tracing runner's line is read back from its own socket and parses.
    let Some(Backend::Kvm { kernel, .. }) = kvm_backend("core_s_04_4 trace") else {
        return;
    };
    let guest = fs::read(root().join("guest/digest.wasm")).unwrap();
    let (mut started, mut traced) = runner::start_traced().unwrap();
    let counts = Counts::default();
    let outcome = runner::serve(
        &mut started.stream,
        &runner::boot(&kernel, &guest).unwrap(),
        hyalos_audit::Host::new(Counting::new(&counts), fixture_inputs()),
        Instant::now() + Duration::from_secs(30),
        &mut Timing::default(),
    );
    started.end();
    assert!(outcome.is_ok());
    let [load, setup, boot] = runner::read_trace(&mut traced).unwrap();
    assert!(load > 0.0 && setup > 0.0 && boot > 0.0);
}

/// The holder waits for killed runners to be reaped before it exits, but not for ever: a reaper
/// that is stuck is left after the bound, and one that ends in time is joined.
#[test]
fn runner_waiting_for_reapers_is_bounded() {
    let (release, stuck) = std::sync::mpsc::channel::<()>();
    runner::add_reaper(std::thread::spawn(move || {
        let _ = stuck.recv();
    }));
    runner::add_reaper(std::thread::spawn(|| {}));
    let started = Instant::now();
    runner::wait_for_runners_within(Duration::from_millis(100));
    let waited = started.elapsed();
    assert!(
        waited >= Duration::from_millis(100),
        "it waited for the stuck reaper: {waited:?}"
    );
    assert!(
        waited < Duration::from_secs(2),
        "then it returned: {waited:?}"
    );
    release.send(()).unwrap();
}

/// 6.1: every line of the `attacks` command's output is the previous commit's, where the machine
/// ran in the monitor's own process. The three host-dependent probes print the line that commit
/// printed for this host's measurement.
#[test]
fn core_6_1_the_attacks_command_prints_the_previous_commits_lines() {
    let (
        Some(Backend::Kvm { kernel, .. }),
        Some(Backend::Kvm {
            kernel: selftest, ..
        }),
    ) = (kvm_backend("core_6_1"), selftest_kvm_backend("core_6_1"))
    else {
        return;
    };
    let measured = crate::cpuid_tests::measured_host("core_6_1");
    let dir = env::temp_dir().join(format!("hyalos-attacks-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("kernel"), &kernel).unwrap();
    fs::write(dir.join("selftest"), &selftest).unwrap();
    let program = env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("task-kernel");
    let output = Command::new(program)
        .arg("attacks")
        .arg(root().join("guest/digest.wasm"))
        .arg(root().join("evidence/fixtures/issues.tsv"))
        .env_clear()
        .env("HYALOS_KERNEL", dir.join("kernel"))
        .env("HYALOS_SELFTEST_KERNEL", dir.join("selftest"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let _ = fs::remove_dir_all(&dir);
    assert!(output.status.success());
    let want: Vec<String> = PREVIOUS_ATTACKS
        .lines()
        .map(|line| {
            let host_dependent = HOST_DEPENDENT.iter().find(|(probe, _)| line == format!("PASS {probe}: Err(KernelFault)"));
            match host_dependent {
                Some((probe, instruction)) if measured.iter().any(|(name, blocked)| name == instruction && !blocked) => {
                    format!("REACHABLE {probe}: Err(BadHypercall) (the host probe measured it reachable, ADR-009)")
                }
                _ => line.to_string(),
            }
        })
        .collect();
    let got: Vec<&str> = std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .collect();
    assert_eq!(got, want);
}

/// `task-kernel attacks` with the kernel and the self-test kernel, at the commit before this slice
/// (`4ee6273`), on a host whose KVM traps all three randomness instructions.
const PREVIOUS_ATTACKS: &str = "\
PASS guest: open a file (WASI import): Err(BadImport)
PASS guest: read the clock (WASI import): Err(BadImport)
PASS guest: open a socket (invented import): Err(BadImport)
PASS guest: import the host's memory: Err(BadImport)
PASS guest: use a float (NaN nondeterminism): Err(InvalidModule)
PASS guest: use SIMD: Err(InvalidModule)
PASS guest: use threads (atomics): Err(InvalidModule)
PASS guest: run before the kernel calls it (start): Err(InvalidModule)
PASS guest: loop forever: Err(OutOfFuel)
PASS guest: grow memory past the limit: Err(GuestTrap)
PASS guest: recurse without bound: Err(GuestTrap)
PASS guest: read outside its memory: Err(BadPointer)
PASS guest: ask an empty question from outside its memory: Err(BadPointer)
PASS guest: oversized question: Err(TooLarge)
PASS guest: ask an unapproved question type: Err(UnknownSchema)
PASS guest: perform an unapproved effect: Err(UnknownEffect)
PASS guest: perform a second effect: Err(EffectLimit)
PASS guest: exit without a result: Err(NoResult)
PASS kernel: halt: Err(KernelFault)
PASS kernel: spin forever: Err(Timeout)
PASS kernel: crash (illegal instruction): Err(KernelFault)
PASS kernel: read memory it was not given: Err(KernelFault)
PASS kernel: write to another port: Err(BadHypercall)
PASS kernel: read a device port (serial): Err(BadHypercall)
PASS kernel: send a request from outside memory: Err(BadPointer)
PASS kernel: send an unknown request: Err(BadHypercall)
PASS kernel: send a request from its own code: Err(BadPointer)
PASS kernel: have the monitor overwrite kernel code: Err(BadPointer)
PASS kernel: have the monitor overwrite the page tables: Err(BadPointer)
PASS kernel: have the monitor overwrite the signed guest: Err(BadPointer)
PASS kernel: have the monitor overwrite the stack: Err(BadPointer)
PASS kernel: send the page tables out as an effect: Err(BadPointer)
PASS kernel: send kernel code out as a result: Err(BadPointer)
PASS kernel: send an effect from outside memory: Err(BadPointer)
PASS kernel: perform a second effect: Err(EffectLimit)
PASS kernel: report a result from outside memory: Err(BadPointer)
PASS memory: nothing (control: must survive): Err(BadHypercall)
PASS memory: change its own code: Err(KernelFault)
PASS memory: run bytes it wrote: Err(KernelFault)
PASS memory: change its page tables: Err(KernelFault)
PASS memory: run off the stack: Err(KernelFault)
PASS memory: follow a null pointer: Err(KernelFault)
PASS memory: change the signed guest: Err(KernelFault)
PASS memory: read past its memory: Err(KernelFault)
PASS privilege: read the time counter (RDTSC): Err(KernelFault)
PASS privilege: read the time counter (RDTSCP): Err(KernelFault)
PASS privilege: clear CR4.TSD, then read the time counter: Err(KernelFault)
PASS privilege: load other page tables (CR3): Err(KernelFault)
PASS privilege: turn write protect off (CR0): Err(KernelFault)
PASS privilege: load a descriptor table (LGDT): Err(KernelFault)
PASS privilege: load an interrupt table (LIDT): Err(KernelFault)
PASS privilege: write a model-specific register (WRMSR): Err(KernelFault)
PASS privilege: halt (HLT): Err(KernelFault)
PASS randomness: hardware randomness (RDRAND): Err(KernelFault)
PASS randomness: read the processor ID (RDPID): Err(KernelFault)
PASS randomness: hardware seed (RDSEED): Err(KernelFault)
PASS tamper: change one model answer: Err(ReplayDiverged)
PASS tamper: change the sent message: Err(ReplayDiverged)
PASS tamper: change one input: Err(ReplayDiverged)
PASS tamper: change the result: Err(ReplayDiverged)
PASS tamper: hide the sent message: Err(ReplayDiverged)
PASS tamper: change the work done: Err(ReplayDiverged)
PASS tamper: swap the signed guest: Err(GuestMismatch)
";
