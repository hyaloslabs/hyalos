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
    EffectFailed,
    LedgerFailed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
struct EffectRecord {
    kind: i32,
    body_hex: String,
    receipt: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

/// Checks that every model answer in a record is the one a notary saw the provider's server give
/// to that exact question. It needs no network and no credential, only the notary key to trust.
fn attested(t: &Transcript, record: &str, notary: &str) -> Result<String, String> {
    let tool = env::var("HYALOS_ATTEST").map_err(|_| "set HYALOS_ATTEST to the attestation tool")?;
    for (index, answer) in t.answers.iter().enumerate() {
        let file = format!("{record}.attest/{index:03}.presentation");
        let bytes = fs::read(&file).map_err(|_| format!("answer {index}: no attestation"))?;
        if answer.attestation.is_empty() || sha(&bytes) != answer.attestation {
            return Err(format!("answer {index}: the attestation is not the one the record names"));
        }
        let output = std::process::Command::new(&tool).args(["verify", &file, notary]).output().map_err(|_| "cannot run the attestation tool")?;
        if !output.status.success() {
            return Err(format!("answer {index}: the attestation does not verify"));
        }
        let seen: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|_| "cannot read the verdict")?;
        let same = seen["question_sha256"] == answer.question_sha256.as_str()
            && seen["label"] == answer.label
            && seen["model"] == answer.model.as_str()
            && seen["response_id"] == answer.response_id.as_str();
        if !same {
            return Err(format!("answer {index}: the notary saw a different question or answer than the record claims"));
        }
    }
    Ok(format!("attested: {} of {} model answers match what the notary saw", t.answers.len(), t.answers.len()))
}

fn main_inner() -> Result<bool, String> {
    let args: Vec<String> = env::args().collect();
    let arg = |i: usize| args.get(i).map(String::as_str).ok_or_else(|| "usage: run|replay|attacks|bench ...".to_string());
    let backend = match env::var("HYALOS_KERNEL") {
        Ok(path) => Backend::Kvm { kernel: fs::read(path).map_err(|_| "cannot read kernel")?, limit: Duration::from_secs(30) },
        Err(_) => Backend::Process,
    };
    match arg(1)? {
        "run" => {
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
            let guest = fs::read(arg(2)?).map_err(|_| "cannot read guest")?;
            let inputs = read_inputs(arg(3)?)?;
            let issue = arg(6)?.parse().map_err(|_| "bad issue number")?;
            let key = format!("{}/.config/hyalos-spike/anthropic.key", env::var("HOME").map_err(|_| "no home")?);
            // `stub` as the repository runs the same committed flow with the stand-in model and no
            // real notification, so the chain can be exercised without a credential.
            let mut mode = if arg(5)? == "stub" {
                Mode::Record { seed: issue }
            } else {
                // With HYALOS_ATTEST naming the attestation tool, every model call goes through a
                // notarized session and its presentation is kept beside the record.
                let attest = env::var("HYALOS_ATTEST").ok().map(|tool| (tool, format!("{}.attest", &args[4])));
                Mode::Live(live::Live::new(&key, arg(5)?, issue, attest).ok_or("cannot use the credential file")?)
            };
            let backend = match backend {
                Backend::Kvm { kernel, .. } => Backend::Kvm { kernel, limit: Duration::from_secs(600) },
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
            // The task is named without a salt, so repeated runs of one task are visible as such.
            let task = sha(format!("{}:{}", sha(&guest), sha(&inputs.concat())).as_bytes());
            let intent = serde_json::json!({"v": 1, "kind": "intent", "task": task, "guest_sha256": sha(&guest)});
            let (_, intent_hash) = ledger.append(intent).map_err(|r| format!("{r:?}"))?;
            // The run is declared. From here every attested call names it.
            if let Mode::Live(live) = &mut mode {
                live.bind(&intent_hash);
            }
            let outcome = execute(&backend, &guest, inputs, mode);
            let (state, commitment) = match &outcome {
                Ok(t) => ("ok".to_string(), sha([salt.as_bytes(), &serde_json::to_vec(t).map_err(|_| "encode")?].concat().as_slice())),
                Err(reason) => (format!("refused:{reason:?}"), String::new()),
            };
            let result = serde_json::json!({"v": 1, "kind": "result", "intent": intent_hash, "outcome": state, "transcript": commitment});
            let (seq, _) = ledger.append(result).map_err(|r| format!("{r:?}"))?;
            let stamped = ledger.timestamp(seq).is_some();
            let t = outcome.map_err(|r| format!("{r:?}"))?;
            fs::write(arg(4)?, serde_json::to_vec_pretty(&t).map_err(|_| "encode")?).map_err(|_| "cannot write")?;
            fs::write(format!("{}.salt", arg(4)?), &salt).map_err(|_| "cannot write")?;
            println!("committed: link {seq} in the public log, Bitcoin timestamp requested: {stamped}");
            let model = t.answers.first().map_or("", |a| a.model.as_str());
            println!("ok: {} inputs, {} inferences by {model}, fuel {}", t.inputs_hex.len(), t.answers.len(), t.fuel_used);
            for e in &t.effects {
                println!("sent: {}", e.receipt);
            }
            Ok(true)
        }
        "audit" => {
            let t: Transcript = serde_json::from_slice(&fs::read(arg(2)?).map_err(|_| "cannot read")?).map_err(|_| "cannot parse")?;
            let salt = fs::read(format!("{}.salt", arg(2)?)).map_err(|_| "no salt for this record")?;
            let commitment = sha([salt.as_slice(), &serde_json::to_vec(&t).map_err(|_| "encode")?].concat().as_slice());
            let home = env::var("HOME").map_err(|_| "no home")?;
            let ledger = ledger::Ledger::open(args.get(3).map_or("ledger", String::as_str), &home).ok_or("cannot open the ledger")?;
            println!("audit passed: {}", ledger.audit(&commitment)?);
            if let Ok(notary) = env::var("HYALOS_NOTARY") {
                println!("{}", attested(&t, arg(2)?, &notary)?);
            }
            Ok(true)
        }
        "attested" => {
            let t: Transcript = serde_json::from_slice(&fs::read(arg(2)?).map_err(|_| "cannot read")?).map_err(|_| "cannot parse")?;
            println!("{}", attested(&t, arg(2)?, arg(3)?)?);
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
            let guest = fs::read(arg(2)?).map_err(|_| "cannot read guest")?;
            let t: Transcript = serde_json::from_slice(&fs::read(arg(3)?).map_err(|_| "cannot read")?).map_err(|_| "cannot parse")?;
            replay(&backend, &guest, &t).map_err(|r| format!("{r:?}"))?;
            println!("replay identical");
            Ok(true)
        }
        "attacks" => {
            let guest = fs::read(arg(2)?).map_err(|_| "cannot read guest")?;
            run_attacks(&backend, &guest, &read_inputs(arg(3)?)?)
        }
        "bench" => {
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
