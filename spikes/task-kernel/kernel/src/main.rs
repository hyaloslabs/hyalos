//! hyalOS kernel v0: a throwaway spike. Not part of the workspace or the trusted core.
//!
//! A guest is a WebAssembly module run by an interpreter (no JIT). Its whole interface is four host
//! calls: read, infer, effect, return. Anything else is refused before it runs. Every run produces a
//! transcript, and `replay` re-executes the guest from that transcript and demands identical bytes.
//!
//! Two backends run the same guest under the same rules: an ordinary process, and a bare-metal
//! kernel booted on KVM (`kvm.rs`, chosen by setting `HYALOS_KERNEL` to the kernel image).

mod kvm;
mod ledger;
mod live;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    process::ExitCode,
    time::{Duration, Instant},
};
use wasmi::{
    Caller, Config, Engine, Error, Extern, ExternType, Linker, Memory, Module, Store, StoreLimits,
    StoreLimitsBuilder, TrapCode,
};

const FUEL: u64 = 200_000_000;
const MAX_MEMORY: usize = 4 * 1024 * 1024;
const MAX_QUESTION: usize = 4 * 1024;
const MAX_BODY: usize = 64 * 1024;
pub(crate) const MAX_RESULT: usize = 4 * 1024;
const MAX_EFFECTS: usize = 1;
const LABELS: i32 = 4; // schema 0: 0 bug, 1 feature, 2 security, 3 other
const NOTIFY: i32 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reason {
    InvalidModule,
    BadImport,
    BadExport,
    OutOfFuel,
    GuestTrap,
    BadPointer,
    TooLarge,
    UnknownSchema,
    UnknownEffect,
    EffectLimit,
    NoResult,
    ReplayDiverged,
    GuestMismatch,
    KernelFault,
    BadHypercall,
    Timeout,
    NoKvm,
    ModelUnavailable,
    ModelRefused,
    NotaryUnreachable,
    NotaryDeadline,
    WrongNotaryKey,
    ReleaseMismatch,
    ProviderRefused,
    EffectFailed,
    LedgerFailed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    schema: i32,
    question_sha256: String,
    label: i32,
    model: String,
    response_id: String,
    /// The hash of a notary's presentation of this answer, when the call was attested.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    attestation: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EffectRecord {
    kind: i32,
    body_hex: String,
    receipt: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Transcript {
    version: u32,
    guest_sha256: String,
    inputs_hex: Vec<String>,
    answers: Vec<Answer>,
    effects: Vec<EffectRecord>,
    result_hex: String,
    fuel_used: u64,
}

enum Mode {
    Record { seed: u64 },
    Live(live::Live),
    Replay { answers: Vec<Answer>, effects: Vec<EffectRecord> },
}

#[derive(Clone)]
enum Backend {
    Process,
    Kvm { kernel: Vec<u8>, limit: Duration },
}

/// What a run may read, ask and do. Both backends answer the guest's calls only through this.
pub(crate) struct Host {
    mode: Mode,
    inputs: Vec<Vec<u8>>,
    answers: Vec<Answer>,
    effects: Vec<EffectRecord>,
}

impl Host {
    pub(crate) fn input(&self, index: i32) -> Option<&[u8]> {
        self.inputs.get(usize::try_from(index).ok()?).map(Vec::as_slice)
    }

    /// Checks a question's type before its bytes are read, and gives the most bytes it may have.
    pub(crate) fn pre_infer(&self, schema: i32) -> Result<usize, Reason> {
        if schema == 0 { Ok(MAX_QUESTION) } else { Err(Reason::UnknownSchema) }
    }

    pub(crate) fn infer(&mut self, schema: i32, question: &[u8]) -> Result<i32, Reason> {
        let digest = sha(question);
        let at = self.answers.len();
        let (label, model, response_id, attestation) = match &mut self.mode {
            Mode::Record { seed } => (stub_model(question, *seed), "stub".to_string(), String::new(), String::new()),
            Mode::Live(live) => live.infer(question)?,
            Mode::Replay { answers, .. } => match answers.get(at) {
                Some(a) if a.schema == schema && a.question_sha256 == digest && (0..LABELS).contains(&a.label) => {
                    (a.label, a.model.clone(), a.response_id.clone(), a.attestation.clone())
                }
                _ => return Err(Reason::ReplayDiverged),
            },
        };
        self.answers.push(Answer { schema, question_sha256: digest, label, model, response_id, attestation });
        Ok(label)
    }

    pub(crate) fn pre_effect(&self, kind: i32) -> Result<usize, Reason> {
        if kind != NOTIFY {
            return Err(Reason::UnknownEffect);
        }
        if self.effects.len() >= MAX_EFFECTS { Err(Reason::EffectLimit) } else { Ok(MAX_BODY) }
    }

    pub(crate) fn effect(&mut self, kind: i32, body: &[u8]) -> Result<(), Reason> {
        let body_hex = hex(body);
        let receipt = match &self.mode {
            Mode::Record { .. } => String::new(),
            Mode::Live(live) => live.notify(body)?,
            // A replay performs nothing: it only checks the task would have sent the same bytes.
            Mode::Replay { effects, .. } => match effects.get(self.effects.len()) {
                Some(e) if e.kind == kind && e.body_hex == body_hex => e.receipt.clone(),
                _ => return Err(Reason::ReplayDiverged),
            },
        };
        self.effects.push(EffectRecord { kind, body_hex, receipt });
        Ok(())
    }
}

struct Process {
    host: Host,
    result: Option<Vec<u8>>,
    refused: Option<Reason>,
    limits: StoreLimits,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok()).collect()
}

pub(crate) fn sha(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn refuse(caller: &mut Caller<'_, Process>, reason: Reason) -> Error {
    caller.data_mut().refused = Some(reason);
    Error::new("refused")
}

fn memory(caller: &mut Caller<'_, Process>) -> Result<Memory, Error> {
    match caller.get_export("memory") {
        Some(Extern::Memory(memory)) => Ok(memory),
        _ => Err(refuse(caller, Reason::BadExport)),
    }
}

fn guest_bytes(caller: &mut Caller<'_, Process>, ptr: i32, len: i32, max: usize) -> Result<Vec<u8>, Error> {
    let (ptr, len) = (ptr as u32 as usize, len as u32 as usize);
    if len > max {
        return Err(refuse(caller, Reason::TooLarge));
    }
    let memory = memory(caller)?;
    let data = memory.data(&*caller);
    match ptr.checked_add(len).and_then(|end| data.get(ptr..end)) {
        Some(bytes) => Ok(bytes.to_vec()),
        None => Err(refuse(caller, Reason::BadPointer)),
    }
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

fn linker(engine: &Engine) -> Result<Linker<Process>, Reason> {
    let mut linker = Linker::new(engine);
    let bad = |_| Reason::InvalidModule;
    linker
        .func_wrap("hyalos", "read", |mut c: Caller<'_, Process>, index: i32, ptr: i32, cap: i32| -> Result<i32, Error> {
            let Some(record) = c.data().host.input(index).map(<[u8]>::to_vec) else {
                return Ok(-1);
            };
            if (cap as u32 as usize) < record.len() {
                return Ok(-2);
            }
            let memory = memory(&mut c)?;
            if memory.write(&mut c, ptr as u32 as usize, &record).is_err() {
                return Err(refuse(&mut c, Reason::BadPointer));
            }
            Ok(record.len() as i32)
        })
        .map_err(bad)?;
    linker
        .func_wrap("hyalos", "infer", |mut c: Caller<'_, Process>, schema: i32, ptr: i32, len: i32| -> Result<i32, Error> {
            let max = c.data().host.pre_infer(schema).map_err(|r| refuse(&mut c, r))?;
            let question = guest_bytes(&mut c, ptr, len, max)?;
            c.data_mut().host.infer(schema, &question).map_err(|r| refuse(&mut c, r))
        })
        .map_err(bad)?;
    linker
        .func_wrap("hyalos", "effect", |mut c: Caller<'_, Process>, kind: i32, ptr: i32, len: i32| -> Result<i32, Error> {
            let max = c.data().host.pre_effect(kind).map_err(|r| refuse(&mut c, r))?;
            let body = guest_bytes(&mut c, ptr, len, max)?;
            c.data_mut().host.effect(kind, &body).map_err(|r| refuse(&mut c, r))?;
            Ok(0)
        })
        .map_err(bad)?;
    linker
        .func_wrap("hyalos", "return", |mut c: Caller<'_, Process>, ptr: i32, len: i32| -> Result<(), Error> {
            let result = guest_bytes(&mut c, ptr, len, MAX_RESULT)?;
            c.data_mut().result = Some(result);
            Err(Error::i32_exit(0))
        })
        .map_err(bad)?;
    Ok(linker)
}

fn engine() -> Engine {
    let mut config = Config::default();
    config
        .consume_fuel(true)
        .floats(false)
        .allow_start_fn(false)
        .ignore_custom_sections(true)
        .wasm_memory64(false)
        .wasm_multi_memory(false)
        .wasm_tail_call(false)
        .wasm_wide_arithmetic(false)
        .wasm_custom_page_sizes(false);
    Engine::new(&config)
}

fn in_process(guest: &[u8], host: Host) -> Result<(Host, Vec<u8>, u64), Reason> {
    let engine = engine();
    let module = Module::new(&engine, guest).map_err(|_| Reason::InvalidModule)?;
    for import in module.imports() {
        let allowed = import.module() == "hyalos"
            && ["read", "infer", "effect", "return"].contains(&import.name())
            && matches!(import.ty(), ExternType::Func(_));
        if !allowed {
            return Err(Reason::BadImport);
        }
    }
    let has = |name: &str, func: bool| {
        module.exports().any(|e| {
            e.name() == name
                && if func { matches!(e.ty(), ExternType::Func(_)) } else { matches!(e.ty(), ExternType::Memory(_)) }
        })
    };
    if !has("run", true) || !has("memory", false) {
        return Err(Reason::BadExport);
    }
    let limits = StoreLimitsBuilder::new()
        .memory_size(MAX_MEMORY)
        .memories(1)
        .tables(1)
        .table_elements(4096)
        .instances(1)
        .trap_on_grow_failure(true)
        .build();
    let mut store = Store::new(&engine, Process { host, result: None, refused: None, limits });
    store.limiter(|p| &mut p.limits);
    store.set_fuel(FUEL).map_err(|_| Reason::InvalidModule)?;
    let linker = linker(&engine)?;
    let instance = linker.instantiate_and_start(&mut store, &module).map_err(|_| Reason::BadImport)?;
    let run = instance.get_typed_func::<(), ()>(&store, "run").map_err(|_| Reason::BadExport)?;
    let outcome = run.call(&mut store, ());
    if let Some(reason) = store.data().refused {
        return Err(reason);
    }
    match outcome {
        Ok(()) => return Err(Reason::NoResult),
        Err(error) if error.i32_exit_status() == Some(0) => {}
        Err(error) if error.as_trap_code() == Some(TrapCode::OutOfFuel) => return Err(Reason::OutOfFuel),
        Err(_) => return Err(Reason::GuestTrap),
    }
    let fuel_used = FUEL - store.get_fuel().map_err(|_| Reason::GuestTrap)?;
    let process = store.into_data();
    Ok((process.host, process.result.ok_or(Reason::NoResult)?, fuel_used))
}

fn execute(backend: &Backend, guest: &[u8], inputs: Vec<Vec<u8>>, mode: Mode) -> Result<Transcript, Reason> {
    let inputs_hex = inputs.iter().map(|i| hex(i)).collect();
    let host = Host { mode, inputs, answers: vec![], effects: vec![] };
    let (host, result, fuel_used) = match backend {
        Backend::Process => in_process(guest, host)?,
        Backend::Kvm { kernel, limit } => kvm::run(kernel, guest, host, *limit)?,
    };
    Ok(Transcript {
        version: 1,
        guest_sha256: sha(guest),
        inputs_hex,
        answers: host.answers,
        effects: host.effects,
        result_hex: hex(&result),
        fuel_used,
    })
}

fn replay(backend: &Backend, guest: &[u8], recorded: &Transcript) -> Result<(), Reason> {
    if sha(guest) != recorded.guest_sha256 {
        return Err(Reason::GuestMismatch);
    }
    let inputs = recorded.inputs_hex.iter().map(|i| unhex(i)).collect::<Option<Vec<_>>>().ok_or(Reason::ReplayDiverged)?;
    let mode = Mode::Replay { answers: recorded.answers.clone(), effects: recorded.effects.clone() };
    let again = execute(backend, guest, inputs, mode)?;
    let same = serde_json::to_vec(&again).map_err(|_| Reason::ReplayDiverged)?
        == serde_json::to_vec(recorded).map_err(|_| Reason::ReplayDiverged)?;
    if same { Ok(()) } else { Err(Reason::ReplayDiverged) }
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
        let probes = ["nothing (control: must survive)", "change its own code", "run bytes it wrote", "change its page tables", "run off the stack", "follow a null pointer", "change the signed guest", "read past its memory"];
        for (index, name) in probes.iter().enumerate() {
            let want = if index == 0 { Reason::BadHypercall } else { Reason::KernelFault };
            check(&format!("memory: {name}"), execute(&probing, &[index as u8], vec![], Mode::Record { seed: 0 }).map(|_| ()), want);
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

/// The fifth limit of S-04.5 (ADR-012): verify never sees the bytes a request hides.
const HIDDEN_BYTES_LIMIT: &str = "not proven: the request's hidden bytes are not shown, so the call may have used another credential or carried an added header";

/// What a passing attestation check reports. The limit is part of it, so it cannot be left out.
fn attested_report(answers: usize, test_settings: bool) -> String {
    // A check made against a stand-in says so, and so cannot be read as one about the real provider.
    let mark = if test_settings { "test trust settings: " } else { "" };
    format!("{mark}attested: {answers} of {answers} model answers match what the notary saw\n{HIDDEN_BYTES_LIMIT}")
}

/// The settings that point verify at a stand-in. They are the only part of this process's
/// environment it is given, so it reads no home directory and no credential.
const TRUST_SETTINGS: [&str; 3] = ["HYALOS_TRUST_SERVER_NAME", "HYALOS_TRUST_ROOT", "HYALOS_TRUST_PROVIDER_ADDRESS"];

/// The verify program the auditor built, and the trust settings it runs under.
struct Verifier {
    program: String,
    trust: Vec<(String, std::ffi::OsString)>,
}

impl Verifier {
    fn from_env(program: &str) -> Self {
        let trust = TRUST_SETTINGS.iter().filter_map(|name| Some((name.to_string(), env::var_os(name)?))).collect();
        Self { program: program.to_string(), trust }
    }
}

/// Holds one answer to what its attestation says. `window` is the run's two signed times, when a
/// chain was checked. Each refusal names its own check.
fn check_answer(seen: &serde_json::Value, answer: &Answer, index: usize, run: &str, window: Option<(u64, u64)>) -> Result<(), String> {
    let fail = |what: &str| Err(format!("answer {index}: {what}"));
    let position = index as u64;
    if seen["intent_hash"] != run {
        return fail("the binding names another run");
    }
    if seen["position"].as_u64() != Some(position) {
        return fail("the binding names another position");
    }
    if seen["run"] != run {
        return fail("the notary was told another run");
    }
    if seen["sequence"].as_u64() != Some(position) {
        return fail("the sequence number is not the answer's position, so an attestation is unaccounted for");
    }
    if let Some((intent_time, result_time)) = window {
        let Some(time) = seen["time"].as_u64() else { return fail("the session has no time") };
        // Whole seconds cannot order a session inside the intent's own second, so it is refused (ADR-008).
        if time <= intent_time {
            return fail("the session is not later than the run's intent");
        }
        if time > result_time {
            return fail("the session is later than the run's result");
        }
    }
    let same = seen["question_sha256"] == answer.question_sha256.as_str()
        && seen["label"] == answer.label
        && seen["model"] == answer.model.as_str()
        && seen["response_id"] == answer.response_id.as_str();
    if !same {
        return fail("the notary saw a different question or answer than the record claims");
    }
    Ok(())
}

/// What the per-answer checks found.
struct Witnessed {
    lines: Vec<String>,
    test_settings: bool,
}

/// Checks that every model answer in a record is the one the trusted notary saw the provider's
/// server give, for this run and position. It needs no network and no credential. With `chain`,
/// the run and its window come from the verified log. Without it, every answer must name one run.
fn witnessed(t: &Transcript, record: &str, notary: &str, verifier: &Verifier, chain: Option<(&str, (u64, u64))>) -> Result<Witnessed, String> {
    if t.answers.is_empty() {
        return Err("this record has no attested answers".into());
    }
    let mut run = chain.map(|(intent, _)| intent.to_string());
    let (mut lines, mut test_settings) = (Vec::new(), false);
    for (index, answer) in t.answers.iter().enumerate() {
        let file = format!("{record}.attest/{index:03}.presentation");
        let bytes = fs::read(&file).map_err(|_| format!("answer {index}: no attestation"))?;
        if answer.attestation.is_empty() || sha(&bytes) != answer.attestation {
            return Err(format!("answer {index}: the attestation is not the one the record names"));
        }
        let output = std::process::Command::new(&verifier.program)
            .args(["verify", &file, notary])
            .env_clear()
            .envs(verifier.trust.iter().map(|(name, value)| (name, value)))
            .output()
            .map_err(|_| "cannot run the verify program")?;
        if !output.status.success() {
            // Only compared, never shown: the program's text stays out of the report.
            let untrusted = String::from_utf8_lossy(&output.stderr).contains("not the trusted one");
            return Err(format!("answer {index}: {}", if untrusted { "not attested by the trusted notary" } else { "the attestation does not verify" }));
        }
        let seen: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|_| "cannot read the verdict")?;
        let named = run.get_or_insert_with(|| seen["intent_hash"].as_str().unwrap_or_default().to_string());
        check_answer(&seen, answer, index, named, chain.map(|(_, window)| window))?;
        test_settings |= !seen["trust"].is_null();
        lines.push(format!("answer {index}: run {named} position {index} sequence {index} session {}", seen["time"]));
    }
    Ok(Witnessed { lines, test_settings })
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
    signed_time: &dyn Fn(usize) -> Option<u64>,
) -> Result<(Result<Transcript, Reason>, usize), String> {
    let task = task_of(&sha(guest), &inputs);
    let intent = serde_json::json!({"v": 1, "kind": "intent", "task": task, "guest_sha256": sha(guest)});
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
    let result = serde_json::json!({"v": 1, "kind": "result", "intent": intent_hash, "outcome": state, "transcript": commitment});
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

/// What `audit` and `attested` are given on the command line.
struct AuditInputs {
    dir: Option<String>,
    operator: Option<String>,
    /// The trusted notary's signing key, as verify takes it.
    notary: String,
    verify: String,
}

/// `[ledger dir] [--operator-public <file>] --notary-public <file> --verify <program>`. There is no
/// setting that skips the attestations: without a trusted notary key the audit does not start.
fn audit_inputs(rest: &[String]) -> Result<AuditInputs, String> {
    let (mut dir, mut operator, mut notary, mut verify) = (None, None, None, None);
    let mut words = rest.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--operator-public" => operator = words.next(),
            "--notary-public" => notary = words.next(),
            "--verify" => verify = words.next(),
            flag if flag.starts_with("--") => return Err("unknown audit option".into()),
            _ if dir.is_none() => dir = Some(word),
            _ => return Err("too many audit arguments".into()),
        }
    }
    let notary = notary.ok_or("the notary's public key is not given: --notary-public <file>")?;
    let verify = verify.ok_or("the verify program is not given: --verify <program you built>")?;
    if !fs::metadata(verify).is_ok_and(|m| m.is_file()) {
        return Err("the verify program is not a file".into());
    }
    Ok(AuditInputs { dir: dir.cloned(), operator: operator.cloned(), notary: notary_key(notary)?, verify: verify.clone() })
}

/// The notary's signing key from its file: one compressed P-256 point in lowercase hex.
fn notary_key(file: &str) -> Result<String, String> {
    let text = fs::read_to_string(file).map_err(|_| "the notary's public key is not a readable file")?;
    let key = text.trim();
    let hex_key = key.len() == 66 && key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if hex_key && (key.starts_with("02") || key.starts_with("03")) { Ok(key.to_string()) } else { Err("the notary's public key is not a signing public key".into()) }
}

/// The test trust setting for the log: a directory of stored replies and the key that signed them.
fn stand_in_log() -> Result<Option<(String, String)>, String> {
    match (env::var("HYALOS_TRUST_LOG_DIR"), env::var("HYALOS_TRUST_LOG_KEY")) {
        (Ok(dir), Ok(key)) => Ok(Some((dir, key))),
        (Err(_), Err(_)) => Ok(None),
        _ => Err("a stand-in log needs both HYALOS_TRUST_LOG_DIR and HYALOS_TRUST_LOG_KEY".into()),
    }
}

/// The whole audit of one record. Everything is checked before anything is reported, so a failure
/// states no proof.
fn audit_record(record: &str, inputs: &AuditInputs, stand_in: Option<(String, String)>, verifier: &Verifier) -> Result<String, String> {
    let t: Transcript = serde_json::from_slice(&fs::read(record).map_err(|_| "cannot read")?).map_err(|_| "cannot parse")?;
    let salt = fs::read(format!("{record}.salt")).map_err(|_| "no salt for this record")?;
    let commitment = sha([salt.as_slice(), &serde_json::to_vec(&t).map_err(|_| "encode")?].concat().as_slice());
    let operator = inputs.operator.as_deref().ok_or("the operator's public key is not given: --operator-public <file>")?;
    let dir = inputs.dir.as_deref().unwrap_or("ledger");
    let audited = ledger::Auditor::new(dir, operator, stand_in)?.audit(&claim_of(&t, commitment)?)?;
    let answers = witnessed(&t, record, &inputs.notary, verifier, Some((&audited.intent, audited.window)))?;
    Ok(audit_report(&audited, answers.lines.len(), answers.test_settings))
}

/// What a passing audit proved, then what it did not.
fn audit_report(audited: &ledger::Audited, answers: usize, test_settings: bool) -> String {
    let log = if audited.stand_in { "stand-in log" } else { "public log" };
    let mut lines = vec!["audit passed".to_string()];
    if audited.stand_in {
        lines.push("test trust settings: the log here is a stand-in".to_string());
    }
    lines.push("proven: every link's signature verifies under the operator's public key".to_string());
    lines.push(format!("proven: every entry's inclusion proof and signed time verify under the {log}'s key"));
    lines.push(format!("proven: the chain is unbroken and is the same list as the {log}'s entries under the operator's key"));
    lines.push(format!("proven: this record is at link {} of {}, with one result for its run", audited.link, audited.links));
    let times = if audited.runs == 1 { "time" } else { "times" };
    lines.push(format!("the task was run {} {times}, as the intent links name the task", audited.runs));
    lines.push(format!("the chain holds {} run(s) of any task under this key", audited.all_runs));
    if test_settings {
        lines.push("test trust settings: the provider here is a stand-in".to_string());
    }
    lines.push(format!("proven: all {answers} answers were attested by the trusted notary for this run and position, inside the run's window, each with a sequence number equal to its position"));
    lines.push("not proven: the notary's independence is the customer's own".to_string());
    lines.push("not proven: the model's name is the provider's claim".to_string());
    lines.push("not proven: the answer's correctness is not judged".to_string());
    lines.push("not proven: the log is trusted to have shown every entry under the operator's key".to_string());
    lines.push(HIDDEN_BYTES_LIMIT.to_string());
    lines.join("\n")
}

/// The backend for the commands that run a guest. The audit never calls this, so it opens no
/// kernel file.
fn load_backend() -> Result<Backend, String> {
    match env::var("HYALOS_KERNEL") {
        Ok(path) => Ok(Backend::Kvm { kernel: fs::read(path).map_err(|_| "cannot read kernel")?, limit: Duration::from_secs(30) }),
        Err(_) => Ok(Backend::Process),
    }
}

/// A task's name: its guest and inputs, without a salt, so repeated runs of one task show as such.
fn task_of(guest_sha256: &str, inputs: &[Vec<u8>]) -> String {
    sha(format!("{guest_sha256}:{}", sha(&inputs.concat())).as_bytes())
}

/// What the record itself says it is, for the audit to hold the chain to.
fn claim_of(t: &Transcript, commitment: String) -> Result<ledger::Claim, String> {
    let inputs = t.inputs_hex.iter().map(|i| unhex(i)).collect::<Option<Vec<_>>>().ok_or("the record's inputs are unreadable")?;
    Ok(ledger::Claim { commitment, task: task_of(&t.guest_sha256, &inputs), guest: t.guest_sha256.clone() })
}

fn main_inner() -> Result<bool, String> {
    let args: Vec<String> = env::args().collect();
    let arg = |i: usize| args.get(i).map(String::as_str).ok_or_else(|| "usage: run|replay|attacks|bench ...".to_string());
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
            let guest = fs::read(arg(2)?).map_err(|_| "cannot read guest")?;
            let inputs = read_inputs(arg(3)?)?;
            let issue = arg(6)?.parse().map_err(|_| "bad issue number")?;
            // A second run on one path would overwrite the first run's evidence.
            if !live::record_is_fresh(arg(4)?) {
                return Err("the record path is already in use".into());
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
            let (outcome, seq) = declared_run(&mut |link| ledger.append(link), &backend, &guest, inputs, mode, &salt, arg(4)?, &|seq| ledger.signed_time(seq))?;
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
            println!("{}", audit_record(arg(2)?, &inputs, stand_in_log()?, &Verifier::from_env(&inputs.verify))?);
            Ok(true)
        }
        "attested" => {
            let inputs = audit_inputs(&args[3..])?;
            if inputs.dir.is_some() || inputs.operator.is_some() {
                return Err("attested takes no ledger: --notary-public <file> --verify <program>".into());
            }
            let t: Transcript = serde_json::from_slice(&fs::read(arg(2)?).map_err(|_| "cannot read")?).map_err(|_| "cannot parse")?;
            let answers = witnessed(&t, arg(2)?, &inputs.notary, &Verifier::from_env(&inputs.verify), None)?;
            println!("{}", answers.lines.join("\n"));
            println!("not checked here: the run's links and its window. The audit checks them");
            println!("{}", attested_report(answers.lines.len(), answers.test_settings));
            Ok(true)
        }
        "forge" => {
            // A test of the auditor: the operator logs a second result for an earlier run.
            let t: Transcript = serde_json::from_slice(&fs::read(arg(2)?).map_err(|_| "cannot read")?).map_err(|_| "cannot parse")?;
            let home = env::var("HOME").map_err(|_| "no home")?;
            let ledger = ledger::Ledger::open("ledger", &home).ok_or("cannot open the ledger")?;
            let salt = fs::read(format!("{}.salt", arg(3)?)).map_err(|_| "no salt")?;
            fs::write(format!("{}.salt", arg(2)?), &salt).map_err(|_| "cannot write")?;
            let commitment = sha([salt.as_slice(), &serde_json::to_vec(&t).map_err(|_| "encode")?].concat().as_slice());
            let links = ledger.links();
            let intent = links.iter().rev().find(|l| l.windows(15).any(|w| w == b"\"kind\":\"intent\"")).map(|l| sha(l)).ok_or("no intent")?;
            let forged = serde_json::json!({"v": 1, "kind": "result", "intent": intent, "outcome": "ok", "transcript": commitment});
            ledger.append(forged).map_err(|r| format!("{r:?}"))?;
            println!("forged a second result for the last run");
            Ok(true)
        }
        "fuzz" => {
            let inputs = read_inputs(arg(2)?)?;
            let rounds = arg(3)?.parse().map_err(|_| "bad count")?;
            let host = || Host { mode: Mode::Record { seed: 1 }, inputs: inputs.clone(), answers: vec![], effects: vec![] };
            let (replied, refused) = kvm::fuzz(host, rounds).ok_or("no memory")?;
            println!("{rounds} random requests: {replied} answered, {refused} refused, no crash");
            Ok(true)
        }
        "replay" => {
            let backend = load_backend()?;
            let guest = fs::read(arg(2)?).map_err(|_| "cannot read guest")?;
            let t: Transcript = serde_json::from_slice(&fs::read(arg(3)?).map_err(|_| "cannot read")?).map_err(|_| "cannot parse")?;
            replay(&backend, &guest, &t).map_err(|r| format!("{r:?}"))?;
            println!("replay identical");
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
        _ => Err("usage: run|replay|attacks|bench ...".into()),
    }
}

fn main() -> ExitCode {
    match main_inner() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("refused: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod live_tests;

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
