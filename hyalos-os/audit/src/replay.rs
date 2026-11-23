//! Running a guest in this process under a profile, with its host calls answered by a `Source`.
//! The audit's source is the record. The monitor plugs its live source in from outside this crate.

use crate::record::*;
use hyalos_profile::Profile;
use wasmi::{
    Caller, Config, Engine, Error, Extern, ExternType, Linker, Memory, Module, Store, StoreLimits,
    StoreLimitsBuilder, TrapCode,
};

/// What a source answers a model call with.
pub struct Asked {
    pub label: i32,
    pub model: String,
    pub response_id: String,
    pub attestation: String,
    pub provider_proofs: Vec<ProviderProof>,
}

/// Where a run's model answers and effects come from.
pub trait Source {
    fn infer(
        &mut self,
        at: usize,
        schema: i32,
        question: &[u8],
        digest: &str,
    ) -> Result<Asked, Reason>;
    fn effect(
        &mut self,
        at: usize,
        kind: i32,
        body: &[u8],
        body_hex: &str,
    ) -> Result<String, Reason>;
}

/// A replay: every answer and effect comes from the record, and nothing is performed.
pub struct ReplaySource {
    pub answers: Vec<Answer>,
    pub effects: Vec<EffectRecord>,
}

impl ReplaySource {
    pub fn answer(
        answers: &[Answer],
        at: usize,
        schema: i32,
        digest: &str,
    ) -> Result<Asked, Reason> {
        match answers.get(at) {
            Some(a)
                if a.schema == schema
                    && a.question_sha256 == digest
                    && (0..LABELS).contains(&a.label) =>
            {
                Ok(Asked {
                    label: a.label,
                    model: a.model.clone(),
                    response_id: a.response_id.clone(),
                    attestation: a.attestation.clone(),
                    provider_proofs: a.provider_proofs.clone(),
                })
            }
            _ => Err(Reason::ReplayDiverged),
        }
    }

    /// A replay performs nothing: it only checks the task would have sent the same bytes.
    pub fn sent(
        effects: &[EffectRecord],
        at: usize,
        kind: i32,
        body_hex: &str,
    ) -> Result<String, Reason> {
        match effects.get(at) {
            Some(e) if e.kind == kind && e.body_hex == body_hex => Ok(e.receipt.clone()),
            _ => Err(Reason::ReplayDiverged),
        }
    }
}

impl Source for ReplaySource {
    fn infer(&mut self, at: usize, schema: i32, _: &[u8], digest: &str) -> Result<Asked, Reason> {
        Self::answer(&self.answers, at, schema, digest)
    }

    fn effect(&mut self, at: usize, kind: i32, _: &[u8], body_hex: &str) -> Result<String, Reason> {
        Self::sent(&self.effects, at, kind, body_hex)
    }
}

/// What a run may read, ask and do. Both backends answer the guest's calls only through this.
pub struct Host<S> {
    pub source: S,
    pub inputs: Vec<Vec<u8>>,
    pub answers: Vec<Answer>,
    pub effects: Vec<EffectRecord>,
}

impl<S: Source> Host<S> {
    pub fn new(source: S, inputs: Vec<Vec<u8>>) -> Self {
        Self {
            source,
            inputs,
            answers: Vec::new(),
            effects: Vec::new(),
        }
    }

    pub fn input(&self, index: i32) -> Option<&[u8]> {
        self.inputs
            .get(usize::try_from(index).ok()?)
            .map(Vec::as_slice)
    }

    /// Checks a question's type before its bytes are read, and gives the most bytes it may have.
    pub fn pre_infer(&self, schema: i32) -> Result<usize, Reason> {
        if schema == 0 {
            Ok(MAX_QUESTION)
        } else {
            Err(Reason::UnknownSchema)
        }
    }

    pub fn infer(&mut self, schema: i32, question: &[u8]) -> Result<i32, Reason> {
        let digest = sha(question);
        let asked = self
            .source
            .infer(self.answers.len(), schema, question, &digest)?;
        let label = asked.label;
        self.answers.push(Answer {
            schema,
            question_sha256: digest,
            label,
            model: asked.model,
            response_id: asked.response_id,
            attestation: asked.attestation,
            provider_proofs: asked.provider_proofs,
        });
        Ok(label)
    }

    pub fn pre_effect(&self, kind: i32) -> Result<usize, Reason> {
        if kind != NOTIFY {
            return Err(Reason::UnknownEffect);
        }
        if self.effects.len() >= MAX_EFFECTS {
            Err(Reason::EffectLimit)
        } else {
            Ok(MAX_BODY)
        }
    }

    pub fn effect(&mut self, kind: i32, body: &[u8]) -> Result<(), Reason> {
        let body_hex = hex(body);
        let receipt = self
            .source
            .effect(self.effects.len(), kind, body, &body_hex)?;
        self.effects.push(EffectRecord {
            kind,
            body_hex,
            receipt,
        });
        Ok(())
    }
}

struct Process<S> {
    host: Host<S>,
    result: Option<Vec<u8>>,
    refused: Option<Reason>,
    limits: StoreLimits,
}

fn refuse<S>(caller: &mut Caller<'_, Process<S>>, reason: Reason) -> Error {
    caller.data_mut().refused = Some(reason);
    Error::new("refused")
}

fn memory<S>(caller: &mut Caller<'_, Process<S>>) -> Result<Memory, Error> {
    match caller.get_export("memory") {
        Some(Extern::Memory(memory)) => Ok(memory),
        _ => Err(refuse(caller, Reason::BadExport)),
    }
}

fn guest_bytes<S>(
    caller: &mut Caller<'_, Process<S>>,
    ptr: i32,
    len: i32,
    max: usize,
) -> Result<Vec<u8>, Error> {
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

fn linker<S: Source + 'static>(engine: &Engine) -> Result<Linker<Process<S>>, Reason> {
    let mut linker = Linker::new(engine);
    let bad = |_| Reason::InvalidModule;
    linker
        .func_wrap(
            "hyalos",
            "read",
            |mut c: Caller<'_, Process<S>>, index: i32, ptr: i32, cap: i32| -> Result<i32, Error> {
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
            },
        )
        .map_err(bad)?;
    linker
        .func_wrap(
            "hyalos",
            "infer",
            |mut c: Caller<'_, Process<S>>,
             schema: i32,
             ptr: i32,
             len: i32|
             -> Result<i32, Error> {
                let max = c
                    .data()
                    .host
                    .pre_infer(schema)
                    .map_err(|r| refuse(&mut c, r))?;
                let question = guest_bytes(&mut c, ptr, len, max)?;
                c.data_mut()
                    .host
                    .infer(schema, &question)
                    .map_err(|r| refuse(&mut c, r))
            },
        )
        .map_err(bad)?;
    linker
        .func_wrap(
            "hyalos",
            "effect",
            |mut c: Caller<'_, Process<S>>, kind: i32, ptr: i32, len: i32| -> Result<i32, Error> {
                let max = c
                    .data()
                    .host
                    .pre_effect(kind)
                    .map_err(|r| refuse(&mut c, r))?;
                let body = guest_bytes(&mut c, ptr, len, max)?;
                c.data_mut()
                    .host
                    .effect(kind, &body)
                    .map_err(|r| refuse(&mut c, r))?;
                Ok(0)
            },
        )
        .map_err(bad)?;
    linker
        .func_wrap(
            "hyalos",
            "return",
            |mut c: Caller<'_, Process<S>>, ptr: i32, len: i32| -> Result<(), Error> {
                let result = guest_bytes(&mut c, ptr, len, MAX_RESULT)?;
                c.data_mut().result = Some(result);
                Err(Error::i32_exit(0))
            },
        )
        .map_err(bad)?;
    Ok(linker)
}

/// This backend's one mapping from a profile to its interpreter configuration.
fn engine(profile: &Profile) -> Engine {
    let mut config = Config::default();
    config
        .consume_fuel(profile.consume_fuel)
        .floats(profile.floats)
        .allow_start_fn(profile.start_function)
        .ignore_custom_sections(profile.ignore_custom_sections)
        .wasm_memory64(profile.memory64)
        .wasm_multi_memory(profile.multi_memory)
        .wasm_tail_call(profile.tail_call)
        .wasm_wide_arithmetic(profile.wide_arithmetic)
        .wasm_custom_page_sizes(profile.custom_page_sizes);
    Engine::new(&config)
}

/// Runs a guest under a profile in this process.
pub fn in_process<S: Source + 'static>(
    guest: &[u8],
    host: Host<S>,
    profile: &Profile,
) -> Result<(Host<S>, Vec<u8>, u64), Reason> {
    let engine = engine(profile);
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
                && if func {
                    matches!(e.ty(), ExternType::Func(_))
                } else {
                    matches!(e.ty(), ExternType::Memory(_))
                }
        })
    };
    if !has("run", true) || !has("memory", false) {
        return Err(Reason::BadExport);
    }
    let limits = StoreLimitsBuilder::new()
        .memory_size(profile.max_memory)
        .memories(profile.memories)
        .tables(profile.tables)
        .table_elements(profile.table_elements)
        .instances(profile.instances)
        .trap_on_grow_failure(profile.trap_on_grow_failure)
        .build();
    let mut store = Store::new(
        &engine,
        Process {
            host,
            result: None,
            refused: None,
            limits,
        },
    );
    store.limiter(|p| &mut p.limits);
    store
        .set_fuel(profile.fuel)
        .map_err(|_| Reason::InvalidModule)?;
    let linker = linker(&engine)?;
    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .map_err(|_| Reason::BadImport)?;
    let run = instance
        .get_typed_func::<(), ()>(&store, "run")
        .map_err(|_| Reason::BadExport)?;
    let outcome = run.call(&mut store, ());
    if let Some(reason) = store.data().refused {
        return Err(reason);
    }
    match outcome {
        Ok(()) => return Err(Reason::NoResult),
        Err(error) if error.i32_exit_status() == Some(0) => {}
        Err(error) if error.as_trap_code() == Some(TrapCode::OutOfFuel) => {
            return Err(Reason::OutOfFuel);
        }
        Err(_) => return Err(Reason::GuestTrap),
    }
    let fuel_used = profile.fuel - store.get_fuel().map_err(|_| Reason::GuestTrap)?;
    let process = store.into_data();
    Ok((
        process.host,
        process.result.ok_or(Reason::NoResult)?,
        fuel_used,
    ))
}

/// A run written at `version` under the profile it names, on whichever backend `run` is.
pub fn execute_with<S: Source>(
    version: u32,
    named: Option<&str>,
    guest: &[u8],
    inputs: Vec<Vec<u8>>,
    source: S,
    run: impl FnOnce(&[u8], Host<S>, &'static Profile) -> Result<(Host<S>, Vec<u8>, u64), Reason>,
) -> Result<Transcript, Reason> {
    let profile = profile_for(version, named).map_err(|_| Reason::ReplayDiverged)?;
    let inputs_hex = inputs.iter().map(|i| hex(i)).collect();
    let task = match version {
        1 => String::new(),
        _ => task_for(version, named, &sha(guest), &inputs).ok_or(Reason::ReplayDiverged)?,
    };
    let (host, result, fuel_used) = run(guest, Host::new(source, inputs), profile)?;
    Ok(Transcript {
        version,
        profile: named.map(str::to_string),
        task,
        guest_sha256: sha(guest),
        inputs_hex,
        answers: host.answers,
        effects: host.effects,
        result_hex: hex(&result),
        fuel_used,
    })
}

/// Re-executes a record's guest from its answers and demands the record's exact bytes.
pub fn replay_with<S: Source>(
    guest: &[u8],
    recorded: &Transcript,
    source: impl FnOnce(Vec<Answer>, Vec<EffectRecord>) -> S,
    run: impl FnOnce(&[u8], Host<S>, &'static Profile) -> Result<(Host<S>, Vec<u8>, u64), Reason>,
) -> Result<(), Reason> {
    if sha(guest) != recorded.guest_sha256 {
        return Err(Reason::GuestMismatch);
    }
    let inputs = recorded
        .inputs_hex
        .iter()
        .map(|i| unhex(i))
        .collect::<Option<Vec<_>>>()
        .ok_or(Reason::ReplayDiverged)?;
    let source = source(recorded.answers.clone(), recorded.effects.clone());
    let again = execute_with(
        recorded.version,
        recorded.profile.as_deref(),
        guest,
        inputs,
        source,
        run,
    )?;
    let same = serde_json::to_vec(&again).map_err(|_| Reason::ReplayDiverged)?
        == serde_json::to_vec(recorded).map_err(|_| Reason::ReplayDiverged)?;
    if same {
        Ok(())
    } else {
        Err(Reason::ReplayDiverged)
    }
}

/// The audit's replay: in this process, from the record alone.
pub fn replay(guest: &[u8], recorded: &Transcript) -> Result<(), Reason> {
    replay_with(
        guest,
        recorded,
        |answers, effects| ReplaySource { answers, effects },
        in_process,
    )
}
