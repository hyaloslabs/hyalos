//! hyalOS kernel v0: a throwaway spike. Not part of the workspace or the trusted core.
//!
//! A guest is a WebAssembly module run by an interpreter (no JIT). Its whole interface is four host
//! calls: read, infer, effect, return. Anything else is refused before it runs. Every run produces a
//! transcript, and `replay` re-executes the guest from that transcript and demands identical bytes.
//!
//! Two backends run the same guest under the same rules: an ordinary process, and a bare-metal
//! kernel booted on KVM (`kvm.rs`, chosen by setting `HYALOS_KERNEL` to the kernel image). A KVM
//! run's machine is owned by a separate process, the machine runner (`runner.rs`).

mod kvm;
mod ledger;
mod live;
mod probe_host;
mod runner;

#[allow(unused_imports)]
pub(crate) use hyalos_audit::{
    Answer, AuditInputs, Asked, Attempt, CANARY_WINDOW_SECONDS, CURRENT_VERSION, EffectRecord, HIDDEN_BYTES_LIMIT, LABELS, LogSource, MAX_RESULT, PlainFiles, ProviderProof, Reason, ReplaySource, Source, Transcript, Verifier,
    attested_report, audit_record, audit_report, check_answer, claim_of, current_task, hex, read_transcript, sha, task_for, task_of, task_v2, task_v4, unhex, utc, witnessed,
};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    process::ExitCode,
    time::{Duration, Instant},
};

/// A run's host, with this program's sources of answers.
pub(crate) type Host = hyalos_audit::Host<Mode>;

pub(crate) enum Mode {
    Record { seed: u64 },
    Live(live::Live),
    Replay { answers: Vec<Answer>, effects: Vec<EffectRecord> },
}

impl Source for Mode {
    fn infer(&mut self, at: usize, schema: i32, question: &[u8], digest: &str) -> Result<Asked, Reason> {
        match self {
            Mode::Record { seed } => Ok(Asked { label: stub_model(question, *seed), model: "stub".to_string(), response_id: String::new(), attestation: String::new(), provider_proofs: Vec::new() }),
            Mode::Live(live) => {
                let (label, model, response_id, attestation) = live.infer(question)?;
                Ok(Asked { label, model, response_id, attestation, provider_proofs: Vec::new() })
            }
            Mode::Replay { answers, .. } => ReplaySource::answer(answers, at, schema, digest),
        }
    }

    fn effect(&mut self, at: usize, kind: i32, body: &[u8], body_hex: &str) -> Result<String, Reason> {
        match self {
            Mode::Record { .. } => Ok(String::new()),
            Mode::Live(live) => live.notify(body),
            // A replay performs nothing: it only checks the task would have sent the same bytes.
            Mode::Replay { effects, .. } => ReplaySource::sent(effects, at, kind, body_hex),
        }
    }
}

#[derive(Clone)]
enum Backend {
    Process,
    Kvm { kernel: Vec<u8>, limit: Duration },
}

/// A stand-in for a model. It is the only nondeterministic thing in a run: a nonzero seed changes
/// some answers, as a real model would between runs. It sits outside the guest.
fn stub_model(question: &[u8], seed: u64) -> i32 {
    let text = String::from_utf8_lossy(question).to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| text.contains(w));
    let label = if has(&["security", "secret", "cve", "escape", "leak", "vulnerab"]) {
        2
    } else if has(&["fix", "bug", "fail", "broken", "error", "crash", "race"]) {
        0
    } else if has(&["add", "feat", "support", "new ", "allow"]) {
        1
    } else {
        3
    };
    let mut mix = Sha256::new();
    mix.update(seed.to_le_bytes());
    mix.update(question);
    let digest = mix.finalize();
    if seed != 0 && digest[0] % 4 == 0 { (label + 1 + i32::from(digest[1] % 3)) % LABELS } else { label }
}

/// A new run, at the version new runs are written in.
fn execute(backend: &Backend, guest: &[u8], inputs: Vec<Vec<u8>>, mode: Mode) -> Result<Transcript, Reason> {
    execute_as(CURRENT_VERSION, backend, guest, inputs, mode)
}

/// A run written at `version`, so a replay rebuilds the bytes of the version it was given.
fn execute_as(version: u32, backend: &Backend, guest: &[u8], inputs: Vec<Vec<u8>>, mode: Mode) -> Result<Transcript, Reason> {
    let named = (version >= 4).then_some(hyalos_profile::CURRENT.name);
    hyalos_audit::execute_with(version, named, guest, inputs, mode, |guest, host, profile| on_backend(backend, guest, host, profile))
}

/// Runs a guest on this backend.
fn on_backend(backend: &Backend, guest: &[u8], host: Host, profile: &'static hyalos_profile::Profile) -> Result<(Host, Vec<u8>, u64), Reason> {
    match backend {
        Backend::Process => hyalos_audit::in_process(guest, host, profile),
        // The kernel image is built with this one profile, so it cannot run under another.
        Backend::Kvm { .. } if profile.name != hyalos_profile::PROFILE_1.name => Err(Reason::ReplayDiverged),
        // The machine runs in its own process, the machine runner (`runner.rs`).
        Backend::Kvm { kernel, limit } => runner::run(kernel, guest, host, *limit),
    }
}

fn replay(backend: &Backend, guest: &[u8], recorded: &Transcript) -> Result<(), Reason> {
    hyalos_audit::replay_with(guest, recorded, |answers, effects| Mode::Replay { answers, effects }, |guest, host, profile| on_backend(backend, guest, host, profile))
}

fn read_inputs(path: &str) -> Result<Vec<Vec<u8>>, String> {
    let text = fs::read(path).map_err(|_| "cannot read inputs".to_string())?;
    Ok(text.split(|b| *b == b'\n').filter(|l| !l.is_empty()).map(<[u8]>::to_vec).collect())
}

const IMPORTS: &str = r#"
  (import "hyalos" "read" (func $read (param i32 i32 i32) (result i32)))
  (import "hyalos" "infer" (func $infer (param i32 i32 i32) (result i32)))
  (import "hyalos" "effect" (func $effect (param i32 i32 i32) (result i32)))
  (import "hyalos" "return" (func $ret (param i32 i32)))
"#;

fn with_run(body: &str) -> String {
    format!(r#"(module {IMPORTS} (memory (export "memory") 1) (func (export "run") {body}))"#)
}

fn attacks() -> Vec<(&'static str, String, Reason)> {
    let ret = "(call $ret (i32.const 0) (i32.const 1))";
    vec![
        ("open a file (WASI import)", r#"(module (import "wasi_snapshot_preview1" "path_open" (func (param i32))) (memory (export "memory") 1) (func (export "run")))"#.into(), Reason::BadImport),
        ("read the clock (WASI import)", r#"(module (import "wasi_snapshot_preview1" "clock_time_get" (func (param i32 i64 i32) (result i32))) (memory (export "memory") 1) (func (export "run")))"#.into(), Reason::BadImport),
        ("open a socket (invented import)", r#"(module (import "hyalos" "socket" (func)) (memory (export "memory") 1) (func (export "run")))"#.into(), Reason::BadImport),
        ("import the host's memory", r#"(module (import "hyalos" "memory" (memory 1)) (export "memory" (memory 0)) (func (export "run")))"#.into(), Reason::BadImport),
        ("use a float (NaN nondeterminism)", with_run("(drop (f64.div (f64.const 0) (f64.const 0)))"), Reason::InvalidModule),
        ("use SIMD", with_run("(drop (v128.const i32x4 0 0 0 0))"), Reason::InvalidModule),
        ("use threads (atomics)", with_run("(drop (i32.atomic.load (i32.const 0)))"), Reason::InvalidModule),
        ("run before the kernel calls it (start)", format!(r#"(module {IMPORTS} (memory (export "memory") 1) (func $s) (start $s) (func (export "run")))"#), Reason::InvalidModule),
        ("loop forever", with_run("(loop $l (br $l))"), Reason::OutOfFuel),
        ("grow memory past the limit", with_run(&format!("(drop (memory.grow (i32.const 1000))) {ret}")), Reason::GuestTrap),
        ("recurse without bound", format!(r#"(module {IMPORTS} (memory (export "memory") 1) (func $r (call $r)) (func (export "run") (call $r)))"#), Reason::GuestTrap),
        ("read outside its memory", with_run(&format!("(drop (call $infer (i32.const 0) (i32.const 65535) (i32.const 4000))) {ret}")), Reason::BadPointer),
        ("ask an empty question from outside its memory", with_run(&format!("(drop (call $infer (i32.const 0) (i32.const 70000) (i32.const 0))) {ret}")), Reason::BadPointer),
        ("oversized question", with_run(&format!("(drop (call $infer (i32.const 0) (i32.const 0) (i32.const 5000))) {ret}")), Reason::TooLarge),
        ("ask an unapproved question type", with_run(&format!("(drop (call $infer (i32.const 9) (i32.const 0) (i32.const 1))) {ret}")), Reason::UnknownSchema),
        ("perform an unapproved effect", with_run(&format!("(drop (call $effect (i32.const 7) (i32.const 0) (i32.const 1))) {ret}")), Reason::UnknownEffect),
        ("perform a second effect", with_run(&format!("(drop (call $effect (i32.const 0) (i32.const 0) (i32.const 1))) (drop (call $effect (i32.const 0) (i32.const 0) (i32.const 1))) {ret}")), Reason::EffectLimit),
        ("exit without a result", with_run(""), Reason::NoResult),
    ]
}

/// Hostile kernels, as raw x86-64 machine code: what a guest that took over the kernel could try.
fn hostile_kernels() -> Vec<(&'static str, Vec<u8>, Reason)> {
    // Writes one 64-bit field of a request block at 0x800000, the start of the heap.
    let set = |offset: u8, value: u32| [&[0x48, 0xc7, 0x04, 0x25, offset, 0x00, 0x80, 0x00][..], &value.to_le_bytes()].concat();
    const BLOCK: u32 = 0x80_0000;
    // Asks the monitor to copy input 0 to `target`, or to send 16 bytes from `source`.
    let read_to = |target: u32| [set(0, 1), set(0x08, 0), set(0x10, target), set(0x18, 4096), set(0x20, 4096)].concat();
    // Sends the request block at `address` through the kernel's one port.
    let call = |address: u32| [&[0xb8][..], &address.to_le_bytes(), &[0x66, 0xba, 0x00, 0x10, 0xef]].concat();
    let effect = |pointer: u32| [set(0, 3), set(0x10, pointer), set(0x18, 16), set(0x20, 1)].concat();
    vec![
        ("halt", vec![0xf4], Reason::KernelFault),
        ("spin forever", vec![0xeb, 0xfe], Reason::Timeout),
        ("crash (illegal instruction)", vec![0x0f, 0x0b], Reason::KernelFault),
        ("read memory it was not given", vec![0x48, 0x8b, 0x04, 0x25, 0x00, 0x00, 0x00, 0x40], Reason::KernelFault),
        ("write to another port", vec![0x66, 0xba, 0x80, 0x00, 0xef], Reason::BadHypercall),
        ("read a device port (serial)", vec![0x66, 0xba, 0xf8, 0x03, 0xed], Reason::BadHypercall),
        ("send a request from outside memory", call(0xffff_fff0), Reason::BadPointer),
        ("send an unknown request", [set(0, 9), call(BLOCK)].concat(), Reason::BadHypercall),
        ("send a request from its own code", call(0x10_0000), Reason::BadPointer),
        ("have the monitor overwrite kernel code", [read_to(0x10_0000), call(BLOCK)].concat(), Reason::BadPointer),
        ("have the monitor overwrite the page tables", [read_to(0x1000), call(BLOCK)].concat(), Reason::BadPointer),
        ("have the monitor overwrite the signed guest", [read_to(0x70_0000), call(BLOCK)].concat(), Reason::BadPointer),
        ("have the monitor overwrite the stack", [read_to(0x1ff_0000), call(BLOCK)].concat(), Reason::BadPointer),
        ("send the page tables out as an effect", [effect(0x1000), call(BLOCK)].concat(), Reason::BadPointer),
        ("send kernel code out as a result", [set(0, 4), set(0x08, 0x10_0000), set(0x10, 16), set(0x18, 1), call(BLOCK)].concat(), Reason::BadPointer),
        ("send an effect from outside memory", [effect(0x7fff_ffff), call(BLOCK)].concat(), Reason::BadPointer),
        ("perform a second effect", [effect(BLOCK), call(BLOCK), vec![0xef]].concat(), Reason::EffectLimit),
        ("report a result from outside memory", [set(0, 4), set(0x08, 0x7fff_ffff), set(0x10, 8), set(0x18, 1), call(BLOCK)].concat(), Reason::BadPointer),
    ]
}

/// The self-test kernel's probes, in the order of the input byte that chooses each
/// (`kernel-image/src/main.rs`, `probe`), and how each must end. The control does nothing and
/// reaches the unknown request, which shows the machine booted. The rest must fault first.
const PROBES: &[(&str, Reason)] = &[
    ("memory: nothing (control: must survive)", Reason::BadHypercall),
    ("memory: change its own code", Reason::KernelFault),
    ("memory: run bytes it wrote", Reason::KernelFault),
    ("memory: change its page tables", Reason::KernelFault),
    ("memory: run off the stack", Reason::KernelFault),
    ("memory: follow a null pointer", Reason::KernelFault),
    ("memory: change the signed guest", Reason::KernelFault),
    ("memory: read past its memory", Reason::KernelFault),
    ("privilege: read the time counter (RDTSC)", Reason::KernelFault),
    ("privilege: read the time counter (RDTSCP)", Reason::KernelFault),
    ("privilege: clear CR4.TSD, then read the time counter", Reason::KernelFault),
    ("privilege: load other page tables (CR3)", Reason::KernelFault),
    ("privilege: turn write protect off (CR0)", Reason::KernelFault),
    ("privilege: load a descriptor table (LGDT)", Reason::KernelFault),
    ("privilege: load an interrupt table (LIDT)", Reason::KernelFault),
    ("privilege: write a model-specific register (WRMSR)", Reason::KernelFault),
    ("privilege: halt (HLT)", Reason::KernelFault),
    ("randomness: hardware randomness (RDRAND)", Reason::KernelFault),
    ("randomness: read the processor ID (RDPID)", Reason::KernelFault),
    ("randomness: hardware seed (RDSEED)", Reason::KernelFault),
];

/// The probes that fault only where the host's KVM can trap them (ADR-002 and ADR-009 of
/// `docs/plans/hyalos-kernel-core`), each with the instruction the host probe measures for it.
/// Elsewhere they run and reach the unknown request, a stated limit.
const HOST_DEPENDENT: &[(&str, &str)] = &[
    ("randomness: hardware randomness (RDRAND)", "RDRAND"),
    ("randomness: read the processor ID (RDPID)", "RDPID"),
    ("randomness: hardware seed (RDSEED)", "RDSEED"),
];

/// Whether the host probe measured a host-dependent probe's instruction as blocked on this host.
/// `None` for any other probe, and when the host could not be measured, so nothing is claimed.
fn blocked_here(name: &str, measured: Option<&[(&str, bool)]>) -> Option<bool> {
    let (_, instruction) = HOST_DEPENDENT.iter().find(|(probe, _)| *probe == name)?;
    measured?.iter().find(|(measured, _)| measured == instruction).map(|&(_, blocked)| blocked)
}

/// How a probe ended, judged on a host.
#[derive(Debug, PartialEq)]
enum Verdict {
    /// It ended as `PROBES` says.
    Pass,
    /// A host-dependent probe ran on a host the host probe did not measure as blocking it: the
    /// stated limit, not a pass.
    Reachable,
    Fail,
}

/// `blocked` is the host probe's measurement of the probe's instruction (`blocked_here`).
fn judge(name: &str, got: Result<(), Reason>, want: Reason, blocked: Option<bool>) -> Verdict {
    if got == Err(want) {
        Verdict::Pass
    } else if HOST_DEPENDENT.iter().any(|(probe, _)| *probe == name) && blocked != Some(true) && got == Err(Reason::BadHypercall) {
        Verdict::Reachable
    } else {
        Verdict::Fail
    }
}

fn run_attacks(backend: &Backend, guest: &[u8], inputs: &[Vec<u8>]) -> Result<bool, String> {
    let mut ok = true;
    let mut check = |name: &str, got: Result<(), Reason>, want: Reason| {
        let pass = got == Err(want);
        ok &= pass;
        println!("{} {name}: {got:?}", if pass { "PASS" } else { "FAIL" });
    };
    for (name, source, want) in attacks() {
        let wasm = wat::parse_str(&source).map_err(|e| format!("{name}: {e}"))?;
        check(&format!("guest: {name}"), execute(backend, &wasm, vec![], Mode::Record { seed: 0 }).map(|_| ()), want);
    }
    if matches!(backend, Backend::Kvm { .. }) {
        for (name, kernel, want) in hostile_kernels() {
            let hostile = Backend::Kvm { kernel, limit: Duration::from_secs(1) };
            check(&format!("kernel: {name}"), execute(&hostile, &[], inputs.to_vec(), Mode::Record { seed: 0 }).map(|_| ()), want);
        }
    }
    if let Ok(path) = env::var("HYALOS_SELFTEST_KERNEL") {
        let kernel = fs::read(path).map_err(|_| "cannot read the self-test kernel")?;
        let probing = Backend::Kvm { kernel, limit: Duration::from_secs(2) };
        let measured = probe_host::on_host(probe_host::KVM_DEVICE).ok();
        for (index, &(name, want)) in PROBES.iter().enumerate() {
            let got = execute(&probing, &[index as u8], vec![], Mode::Record { seed: 0 }).map(|_| ());
            if judge(name, got, want, blocked_here(name, measured.as_deref())) == Verdict::Reachable {
                let why = if measured.is_some() { "the host probe measured it reachable" } else { "the host probe could not test this host" };
                println!("REACHABLE {name}: {got:?} ({why}, ADR-009)");
            } else {
                check(name, got, want);
            }
        }
    }
    let base = execute(backend, guest, inputs.to_vec(), Mode::Record { seed: 7 }).map_err(|r| format!("{r:?}"))?;
    let mut tampers: Vec<(&str, Transcript)> = vec![];
    let mut t = base.clone();
    if let Some(a) = t.answers.first_mut() {
        a.label = (a.label + 1) % LABELS;
    }
    tampers.push(("change one model answer", t));
    let mut t = base.clone();
    if let Some(e) = t.effects.first_mut() {
        e.body_hex = format!("{}00", e.body_hex);
    }
    tampers.push(("change the sent message", t));
    let mut t = base.clone();
    if let Some(i) = t.inputs_hex.first_mut() {
        *i = format!("{i}21");
    }
    tampers.push(("change one input", t));
    let mut t = base.clone();
    t.result_hex = format!("{}00", t.result_hex);
    tampers.push(("change the result", t));
    let mut t = base.clone();
    t.effects.clear();
    tampers.push(("hide the sent message", t));
    let mut t = base.clone();
    t.fuel_used += 1;
    tampers.push(("change the work done", t));
    for (name, t) in tampers {
        check(&format!("tamper: {name}"), replay(backend, guest, &t), Reason::ReplayDiverged);
    }
    let mut other = guest.to_vec();
    other.extend_from_slice(&[0, 2, 1, b'x']); // a custom section: same behaviour, different bytes
    check("tamper: swap the signed guest", replay(backend, &other, &base), Reason::GuestMismatch);
    Ok(ok)
}

/// One declared run: its intent link, its execution, and a result link that says how it ended.
/// A record is written only for a run that was not refused.
fn declared_run(
    append: &mut dyn FnMut(serde_json::Value) -> Result<(usize, String), Reason>,
    backend: &Backend,
    guest: &[u8],
    inputs: Vec<Vec<u8>>,
    mut mode: Mode,
    salt: &str,
    record: &str,
    operator: &str,
    signed_time: &dyn Fn(usize) -> Option<u64>,
) -> Result<(Result<Transcript, Reason>, usize), String> {
    let task = current_task(&sha(guest), &inputs).ok_or("the guest's hash is unreadable")?;
    let intent = serde_json::json!({"v": CURRENT_VERSION, "kind": "intent", "task": task, "guest_sha256": sha(guest), "operator": operator});
    let (intent_seq, intent_hash) = append(intent).map_err(|r| format!("{r:?}"))?;
    // The run is declared. From here every attested call names it.
    if let Mode::Live(live) = &mut mode {
        live.bind(&intent_hash, signed_time(intent_seq));
    }
    // With no notary the guest does not start, so a task that asks nothing cannot act either.
    let no_notary = matches!(&mode, Mode::Live(live) if !live.has_notary());
    let outcome = if no_notary { Err(Reason::NotaryUnreachable) } else { execute(backend, guest, inputs, mode) };
    let (state, commitment) = match &outcome {
        Ok(t) => ("ok".to_string(), sha([salt.as_bytes(), &serde_json::to_vec(t).map_err(|_| "encode")?].concat().as_slice())),
        Err(reason) => (format!("refused:{reason:?}"), String::new()),
    };
    let result = serde_json::json!({"v": CURRENT_VERSION, "kind": "result", "intent": intent_hash, "outcome": state, "transcript": commitment});
    // The record and salt are on disk before the public log commits to them, and take their
    // names only after it has.
    let (pending_record, pending_salt) = (format!("{record}.pending"), format!("{record}.salt.pending"));
    if let Ok(t) = &outcome {
        write_synced(&pending_record, &serde_json::to_vec_pretty(t).map_err(|_| "encode")?)?;
        write_synced(&pending_salt, salt.as_bytes())?;
    }
    // A failed append may still have reached the public log, so the pending files are kept.
    let (seq, _) = append(result).map_err(|r| format!("{r:?}"))?;
    if outcome.is_ok() {
        fs::rename(&pending_record, record).map_err(|_| "cannot write")?;
        fs::rename(&pending_salt, format!("{record}.salt")).map_err(|_| "cannot write")?;
    }
    Ok((outcome, seq))
}

fn write_synced(path: &str, bytes: &[u8]) -> Result<(), String> {
    let mut file = fs::File::create(path).map_err(|_| "cannot write")?;
    std::io::Write::write_all(&mut file, bytes).map_err(|_| "cannot write")?;
    file.sync_all().map_err(|_| "cannot write".to_string())
}

/// `[ledger dir] [--operator-public <file>] --notary-public <file> [--guest <file>] [--legacy-v1]`. There is no
/// setting that skips the attestations: without a trusted notary key the audit does not start.
fn audit_inputs(rest: &[String]) -> Result<AuditInputs, String> {
    let (mut dir, mut operator, mut notary, mut guest, mut legacy) = (None, None, None, None, false);
    let mut words = rest.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--operator-public" => operator = words.next(),
            "--notary-public" => notary = words.next(),
            "--guest" => guest = words.next(),
            "--legacy-v1" => legacy = true,
            flag if flag.starts_with("--") => return Err("unknown audit option".into()),
            _ if dir.is_none() => dir = Some(word),
            _ => return Err("too many audit arguments".into()),
        }
    }
    let notary = notary.ok_or("the notary's public key is not given: --notary-public <file>")?;
    Ok(AuditInputs { dir: dir.cloned(), operator: operator.cloned(), notary: notary_key(notary)?, legacy, guest: guest.cloned() })
}

/// The notary's signing key from its file.
fn notary_key(file: &str) -> Result<String, String> {
    hyalos_audit::notary_key_text(&fs::read_to_string(file).map_err(|_| "the notary's public key is not a readable file")?)
}

/// The backend for the commands that run a guest. The audit never calls this, so it opens no
/// kernel file.
fn load_backend() -> Result<Backend, String> {
    match env::var("HYALOS_KERNEL") {
        Ok(path) => Ok(Backend::Kvm { kernel: fs::read(path).map_err(|_| "cannot read kernel")?, limit: Duration::from_secs(30) }),
        Err(_) => Ok(Backend::Process),
    }
}

fn main_inner() -> Result<bool, String> {
    let args: Vec<String> = env::args().collect();
    let arg = |i: usize| args.get(i).map(String::as_str).ok_or_else(|| "usage: run|replay|task|attacks|bench|probe-host ...".to_string());
    match arg(1)? {
        "run" => {
            let backend = load_backend()?;
            let guest = fs::read(arg(2)?).map_err(|_| "cannot read guest")?;
            let inputs = read_inputs(arg(3)?)?;
            let seed = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(0);
            let t = execute(&backend, &guest, inputs, Mode::Record { seed }).map_err(|r| format!("{r:?}"))?;
            fs::write(arg(4)?, serde_json::to_vec_pretty(&t).map_err(|_| "encode")?).map_err(|_| "cannot write")?;
            println!("ok: {} inputs, {} inferences, {} effect, fuel {}", t.inputs_hex.len(), t.answers.len(), t.effects.len(), t.fuel_used);
            for e in &t.effects {
                println!("{}", String::from_utf8_lossy(&unhex(&e.body_hex).unwrap_or_default()));
            }
            Ok(true)
        }
        "live" => {
            let backend = load_backend()?;
            // A real repository is reached only from a guest inside the machine. On the process
            // backend the guest would run beside the chain and the tools, so nothing is written.
            if matches!(backend, Backend::Process) && arg(5)? != "stub" {
                return Err("a live run on a real repository needs the KVM kernel: set HYALOS_KERNEL".into());
            }
            let guest = fs::read(arg(2)?).map_err(|_| "cannot read guest")?;
            let inputs = read_inputs(arg(3)?)?;
            let issue = arg(6)?.parse().map_err(|_| "bad issue number")?;
            // A second run on one path would overwrite the first run's evidence.
            if !live::record_is_fresh(arg(4)?) {
                return Err("the record path is already in use".into());
            }
            // A host that cannot start the machine runner refuses here, before any chain link.
            if matches!(backend, Backend::Kvm { .. }) {
                runner::check()?;
            }
            // `stub` as the repository runs the same committed flow with the stand-in model and no
            // real notification, so the chain can be exercised without a credential.
            let mode = if arg(5)? == "stub" {
                Mode::Record { seed: issue }
            } else {
                // Every model call goes through the customer's notary, and its presentation is
                // kept beside the record. With no notary configured, the run is refused.
                Mode::Live(live::Live::new(arg(5)?, issue, live::Attest::from_env(arg(4)?)).ok_or("bad repository name")?)
            };
            // Room for every call to take its whole 35-second limit.
            let limit = Duration::from_secs(600u64.max(inputs.len() as u64 * 35 + 60));
            let backend = match backend {
                Backend::Kvm { kernel, .. } => Backend::Kvm { kernel, limit },
                other => other,
            };
            // Commit to the run before it happens, and to its outcome after, in the public chain.
            let home = env::var("HOME").map_err(|_| "no home")?;
            let ledger = ledger::Ledger::open("ledger", &home).ok_or("cannot open the ledger")?;
            // Exactly 32 bytes: reading the device to its end never finishes and eats all memory.
            let mut salt = [0u8; 32];
            fs::File::open("/dev/urandom")
                .and_then(|mut device| std::io::Read::read_exact(&mut device, &mut salt))
                .map_err(|_| "no randomness")?;
            let salt = hex(&salt);
            let operator = ledger.fingerprint().ok_or("cannot read the operator's public key")?;
            let (outcome, seq) = declared_run(&mut |link| ledger.append(link), &backend, &guest, inputs, mode, &salt, arg(4)?, &operator, &|seq| ledger.signed_time(seq))?;
            let stamped = ledger.timestamp(seq).is_some();
            let t = outcome.map_err(|r| format!("{r:?}"))?;
            println!("committed: link {seq} in the public log, Bitcoin timestamp requested: {stamped}");
            let model = t.answers.first().map_or("", |a| a.model.as_str());
            println!("ok: {} inputs, {} inferences by {model}, fuel {}", t.inputs_hex.len(), t.answers.len(), t.fuel_used);
            for e in &t.effects {
                println!("sent: {}", e.receipt);
            }
            Ok(true)
        }
        "audit" => {
            let inputs = audit_inputs(&args[3..])?;
            // The links directory defaults to `ledger` here. The customer's program has no default.
            let inputs = AuditInputs { dir: Some(inputs.dir.unwrap_or_else(|| "ledger".to_string())), ..inputs };
            // A panic in the presentation parser is a refusal, and its text could name what it read.
            std::panic::set_hook(Box::new(|_| {}));
            let log = LogSource::from_env().map_err(|failure| failure.message)?;
            println!("{}", audit_record(arg(2)?, &inputs, log, &Verifier::from_env().map_err(|failure| failure.message)?, &PlainFiles).map_err(|failure| failure.message)?);
            Ok(true)
        }
        "attested" => {
            let inputs = audit_inputs(&args[3..])?;
            if inputs.dir.is_some() || inputs.operator.is_some() || inputs.legacy || inputs.guest.is_some() {
                return Err("attested takes no ledger: --notary-public <file>".into());
            }
            std::panic::set_hook(Box::new(|_| {}));
            let t = read_transcript(&fs::read(arg(2)?).map_err(|_| "cannot read")?)?;
            let answers = witnessed(&t, arg(2)?, &inputs.notary, &Verifier::from_env().map_err(|failure| failure.message)?, None, &PlainFiles).map_err(|failure| failure.message)?;
            println!("{}", answers.lines.join("\n"));
            println!("not checked here: the run's links, its window and the ticket's task. The audit checks them");
            println!("{}", attested_report(answers.lines.len(), answers.test_settings));
            Ok(true)
        }
        "forge" => {
            // A test of the auditor: the operator logs a second result for an earlier run.
            let t = read_transcript(&fs::read(arg(2)?).map_err(|_| "cannot read")?)?;
            let home = env::var("HOME").map_err(|_| "no home")?;
            let ledger = ledger::Ledger::open("ledger", &home).ok_or("cannot open the ledger")?;
            let salt = fs::read(format!("{}.salt", arg(3)?)).map_err(|_| "no salt")?;
            fs::write(format!("{}.salt", arg(2)?), &salt).map_err(|_| "cannot write")?;
            let commitment = sha([salt.as_slice(), &serde_json::to_vec(&t).map_err(|_| "encode")?].concat().as_slice());
            let links = ledger.links();
            let last = links.iter().rev().find(|l| l.windows(15).any(|w| w == b"\"kind\":\"intent\"")).ok_or("no intent")?;
            let intent = sha(last);
            let version = serde_json::from_slice::<serde_json::Value>(last).map_err(|_| "cannot parse")?["v"].clone();
            let forged = serde_json::json!({"v": version, "kind": "result", "intent": intent, "outcome": "ok", "transcript": commitment});
            ledger.append(forged).map_err(|r| format!("{r:?}"))?;
            println!("forged a second result for the last run");
            Ok(true)
        }
        "fuzz" => {
            let inputs = read_inputs(arg(2)?)?;
            let rounds = arg(3)?.parse().map_err(|_| "bad count")?;
            let host = || Host::new(Mode::Record { seed: 1 }, inputs.clone());
            let (replied, refused) = kvm::fuzz(host, rounds).ok_or("no memory")?;
            println!("{rounds} random requests: {replied} answered, {refused} refused, no crash");
            Ok(true)
        }
        "replay" => {
            let backend = load_backend()?;
            let guest = fs::read(arg(2)?).map_err(|_| "cannot read guest")?;
            let t = read_transcript(&fs::read(arg(3)?).map_err(|_| "cannot read")?)?;
            replay(&backend, &guest, &t).map_err(|r| format!("{r:?}"))?;
            println!("replay identical");
            Ok(true)
        }
        "task" => {
            // Reads a guest and its inputs only: no key, no network.
            let guest = fs::read(arg(2)?).map_err(|_| "cannot read guest")?;
            let inputs = read_inputs(arg(3)?)?;
            println!("{}", current_task(&sha(&guest), &inputs).ok_or("the guest's hash is unreadable")?);
            Ok(true)
        }
        "attacks" => {
            let backend = load_backend()?;
            let guest = fs::read(arg(2)?).map_err(|_| "cannot read guest")?;
            run_attacks(&backend, &guest, &read_inputs(arg(3)?)?)
        }
        "bench" => {
            let backend = load_backend()?;
            let guest = fs::read(arg(2)?).map_err(|_| "cannot read guest")?;
            let inputs = read_inputs(arg(3)?)?;
            let n: u32 = arg(4)?.parse().map_err(|_| "bad count")?;
            let start = Instant::now();
            let mut inferences = 0;
            for _ in 0..n {
                inferences = execute(&backend, &guest, inputs.clone(), Mode::Record { seed: 0 }).map_err(|r| format!("{r:?}"))?.answers.len();
            }
            let per_run = start.elapsed().as_secs_f64() / f64::from(n);
            println!("{n} runs, {inferences} inferences each, {:.1} us per run including start-up, {:.1} us per inference", per_run * 1e6, per_run * 1e6 / inferences.max(1) as f64);
            Ok(true)
        }
        _ => Err("usage: run|replay|task|attacks|bench|probe-host ...".into()),
    }
}

fn main() -> ExitCode {
    // The host probe has three endings, not two (S-03.3).
    if env::args().nth(1).as_deref() == Some("probe-host") {
        return ExitCode::from(probe_host::command());
    }
    // The machine runner, started by this program for each KVM run (`runner.rs`). Not listed in
    // the usage: only this program starts it.
    if env::args().nth(1).as_deref() == Some("machine-runner") {
        let (check, trace) = match (env::args().nth(2).as_deref(), env::args().count()) {
            (None, _) => (false, false),
            (Some("--check"), 3) => (true, false),
            (Some("--trace"), 3) => (false, true),
            _ => return ExitCode::from(2),
        };
        return ExitCode::from(runner::command(check, trace));
    }
    let outcome = main_inner();
    runner::wait_for_runners();
    match outcome {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("refused: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod abi_tests;

#[cfg(test)]
mod live_tests;

#[cfg(test)]
mod ring_tests;

#[cfg(test)]
mod cpuid_tests;

#[cfg(test)]
mod profile_tests;

#[cfg(test)]
mod probe_host_tests;

#[cfg(test)]
mod runner_tests;

#[cfg(test)]
mod report_tests {
    use super::*;

    #[test]
    fn s_04_5_a_passing_attestation_check_names_the_hidden_bytes_limit() {
        for test_settings in [false, true] {
            let report = attested_report(3, test_settings);
            assert!(report.contains("attested: 3 of 3"), "{report}");
            assert_eq!(report.lines().last(), Some(HIDDEN_BYTES_LIMIT));
            assert_eq!(report.starts_with("test trust settings: "), test_settings);
        }
    }
}
