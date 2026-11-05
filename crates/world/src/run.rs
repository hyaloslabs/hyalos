//! One simulated job, from a request to the end of its task (S1.5, ADR-0013, ADR-0036).

use std::collections::BTreeSet;
use std::error::Error;
use std::path::{Path, PathBuf};

use hyalos_broker::{Admission, Credential, LiveJobs, Refusal};
use hyalos_engine::{Decider, Engine, Ruling, replay};
use hyalos_log::{Head, Log, LogKey, StoredEvent};
use hyalos_policy::{GateRequest, ask};
use hyalos_sim::Sim;
use hyalos_substrate::{Ending, SandboxSpec, Substrate};
use hyalos_types::{
    Budget, Clock, Digest, Entropy, Event, Gate, JobId, JobOutcome, MANIFEST_VERSION, Manifest,
    ManifestKey, Origin, ReasonCode, Role, Stakes, TaskId, Timestamp,
};
use sha2::{Digest as _, Sha256};

use crate::{
    FakeForge, FakeModel, Fault, Fixtures, Forge, Model, SeededGate, SeededSubstrate, scan::Leak,
};

/// The operator's policy for the simulated repo: what the manifest asks for, no more.
const OPERATOR: &str = r#"
    permit (principal, action in [Action::"read", Action::"push", Action::"open_pr", Action::"comment"], resource)
      when { resource.repo == "sim/repo" && principal.budget_tokens <= 200000 };
    permit (principal, action == Action::"use_tool", resource)
      when { ["Bash", "Edit", "Read"].contains(resource.name) };
    permit (principal, action == Action::"data_flow", resource)
      when { resource.name == "job->model" };
    permit (principal, action == Action::"request_host", resource)
      when { ["github.com", "api.github.com"].contains(resource.name) };
"#;

/// The budget in the manifest, which the run holds the model's spending to.
pub const TOKEN_BUDGET: u64 = 200_000;

/// What happened at each boundary in one run, so a test can tell the run did something.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counts {
    pub issues_read: u32,
    pub forge_faults: u32,
    pub model_calls: u32,
    pub model_faults: u32,
    pub model_replies: u32,
    pub egress_checks: u32,
    pub model_prompts_with_forge_content: u32,
    pub pull_requests: u32,
    pub gates_asked: u32,
    pub gates_refused: u32,
    pub tokens_spent: u64,
    pub sandbox_started: bool,
    /// The broker refused a step because the manifest's deadline had passed.
    pub expired: bool,
}

/// A finished simulated run. Its log is in `dir`, and was checked before this was returned.
#[derive(Debug)]
pub struct Run {
    pub seed: u64,
    pub dir: PathBuf,
    pub fixtures: Fixtures,
    pub events: Vec<StoredEvent>,
    pub head: Head,
    pub outcome: JobOutcome,
    pub counts: Counts,
    /// The environment each sandbox was started with, one entry per start.
    pub sandbox_env: Vec<std::collections::BTreeMap<String, String>>,
}

impl Run {
    /// Planted strings found in the files the run left behind.
    pub fn leaks(&self) -> std::io::Result<Vec<Leak>> {
        crate::scan(&self.dir, &self.fixtures)
    }
}

struct Live(BTreeSet<JobId>);

impl LiveJobs for Live {
    fn is_live(&self, job: JobId) -> bool {
        self.0.contains(&job)
    }
}

type Res<T> = Result<T, Box<dyn Error>>;

fn digest_of(bytes: &[u8]) -> Digest {
    Digest::from_bytes(Sha256::digest(bytes).into())
}

fn key_bytes(sim: &Sim, label: &str) -> [u8; 32] {
    let mut rng = sim.stream(label);
    let mut out = [0u8; 32];
    for chunk in out.chunks_mut(8) {
        chunk.copy_from_slice(&rng.next_u64().to_le_bytes());
    }
    out
}

fn code(text: &str) -> ReasonCode {
    ReasonCode::new(text).unwrap_or_else(ReasonCode::unavailable)
}

fn names(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// Runs one job to the end of its task on the seed, in a log under `dir`, and checks the run: the
/// state holds the properties of ADR-0009, a replay of the log gives the same state, and the log
/// verifies against the engine's public key.
pub fn run_job(seed: u64, dir: &Path) -> Res<Run> {
    std::fs::create_dir_all(dir)?;
    let sim = Sim::new(seed);
    let clock = sim.clock();
    let fixtures = Fixtures::for_seed(seed);
    let mut forge = FakeForge::new(sim.stream("forge"), clock.clone(), &fixtures);
    let mut model = FakeModel::new(sim.stream("model"), clock.clone(), &fixtures);
    let substrate = SeededSubstrate::new(sim.stream("substrate"), clock.clone());
    let mut gate = SeededGate::new(sim.stream("gate"), clock.clone());
    let mut order = sim.stream("order");
    let mut counts = Counts::default();

    let log_key = LogKey::from_seed(key_bytes(&sim, "log-key"));
    let public = log_key.public();
    let db = dir.join("events.db");
    let mut engine = Engine::open(&db, log_key)?;

    // The operator's policy, approved by a human, then the task and its job.
    engine.approve_policy(clock.now(), OPERATOR, Decider::Human, code("sim.operator"))?;
    let issue = 1 + order.below(900);
    let request = format!("fix issue {issue} in sim/repo");
    let task = TaskId::from_seq(
        engine
            .submit(
                clock.now(),
                Origin::Engine,
                Event::TaskCreated {
                    request: digest_of(request.as_bytes()),
                },
            )?
            .seq,
    );
    let job = JobId::from_seq(
        engine
            .submit(
                clock.now(),
                Origin::Engine,
                Event::JobStarted {
                    task,
                    substrate: substrate.recorded_as(),
                    credentials: hyalos_types::CredentialTier::BrokerHeld,
                },
            )?
            .seq,
    );

    // The engine mints the manifest; the broker admits it. Each step below is admitted again, so an
    // expired manifest authorizes nothing (ADR-0004).
    let manifest_key = ManifestKey::from_seed(key_bytes(&sim, "manifest-key"));
    let deadline = Timestamp::from_millis(clock.now().as_millis() + 400 + order.below(2_600));
    let manifest = Manifest {
        version: MANIFEST_VERSION,
        job,
        role: Role::Actor,
        tainted: true,
        repo: "sim/repo".into(),
        branch_prefix: "hyalos/".into(),
        pr_base: None,
        actions: names(&["open_pr", "push", "read"]),
        egress_hosts: names(&["api.github.com", "github.com"]),
        tools: names(&["Bash", "Edit", "Read"]),
        data_flows: names(&["job->model"]),
        budget: Budget {
            tokens: TOKEN_BUDGET,
        },
        deadline,
        approval: None,
    };
    let signed = engine.mint_manifest(
        &manifest,
        &hyalos_types::StepInputs::default(),
        &manifest_key,
        clock.now(),
    )?;
    let admission = Admission::new(manifest_key.public());
    let credential = Credential {
        job,
        role: Role::Actor,
    };
    let live = Live(BTreeSet::from([job]));
    admission.admit(&signed, &credential, &live, &clock)?;
    let hosts: Vec<String> = manifest.egress_hosts.iter().cloned().collect();

    // The sandbox gets the placeholder credential and nothing else (ADR-0016).
    let mut spec = SandboxSpec::new(
        job,
        "sim/actor:1",
        vec!["run".into()],
        PathBuf::from("/run/hyalos/sim.sock"),
    );
    spec.env
        .insert("HYALOS_CREDENTIAL".into(), fixtures.job_credential.clone());

    let mut sandbox = substrate.start(&spec).ok();
    counts.sandbox_started = sandbox.is_some();
    let mut ended: Option<JobOutcome> = match sandbox {
        Some(_) => None,
        None => Some(JobOutcome::Failed),
    };

    let mut tainted: Vec<String> = Vec::new();
    let mut steps = 3 + order.below(8);
    while ended.is_none() && steps > 0 {
        steps -= 1;
        clock.advance(std::time::Duration::from_millis(1 + order.below(50)));
        match admission.admit(&signed, &credential, &live, &clock) {
            Ok(_) => {}
            Err(Refusal::Expired { .. }) => {
                counts.expired = true;
                ended = Some(JobOutcome::DeadlineExceeded);
                break;
            }
            Err(other) => return Err(other.into()),
        }
        // What the step asks the gates about, and which gate. None for a step with no gate.
        let mut ask_gate: Option<(Gate, Vec<u8>)> = None;
        match order.below(4) {
            0 => match forge.read_issue(&fixtures.forge_token, issue) {
                Ok(text) => {
                    counts.issues_read += 1;
                    ask_gate = Some((Gate::Content, text.clone().into_bytes()));
                    tainted.push(text);
                }
                Err(Fault::Unavailable) => counts.forge_faults += 1,
                Err(Fault::Unauthorized) => {
                    return Err("the forge refused the broker's token".into());
                }
            },
            1 => {
                let prompt = if tainted.is_empty() {
                    "plan the work".to_string()
                } else {
                    tainted.join("\n")
                };
                counts.model_calls += 1;
                match model.complete(&fixtures.provider_key, &prompt) {
                    Ok(reply) => {
                        counts.model_replies += 1;
                        counts.tokens_spent += reply.tokens;
                        ask_gate = Some((Gate::Content, reply.text.into_bytes()));
                        if counts.tokens_spent > TOKEN_BUDGET {
                            ended = Some(JobOutcome::Failed);
                        }
                    }
                    Err(Fault::Unavailable) => counts.model_faults += 1,
                    Err(Fault::Unauthorized) => {
                        return Err("the model refused the broker's key".into());
                    }
                }
            }
            2 => {
                let host = hosts[order.below(hosts.len() as u64) as usize].clone();
                counts.egress_checks += 1;
                ask_gate = Some((Gate::Egress, host.into_bytes()));
            }
            _ => match forge.open_pr(&fixtures.forge_token, "hyalos/sim") {
                Ok(_) => counts.pull_requests += 1,
                Err(Fault::Unavailable) => counts.forge_faults += 1,
                Err(Fault::Unauthorized) => {
                    return Err("the forge refused the broker's token".into());
                }
            },
        }
        if let Some((kind, what)) = ask_gate {
            let request = GateRequest {
                gate: kind,
                inputs: digest_of(&what),
                stakes: Stakes::routine(),
            };
            let gate_deadline = Timestamp::from_millis(clock.now().as_millis() + 250);
            let outcome = ask(&mut gate, &clock, gate_deadline, &request);
            match engine.rule_gate(clock.now(), Origin::Broker, job, &request, outcome)? {
                Ruling::Proceed => {}
                Ruling::Refused => counts.gates_refused += 1,
                // The engine has already ended the job and opened the hold.
                Ruling::Held(_) => ended = Some(JobOutcome::Held),
            }
        }
    }
    counts.gates_asked = gate.asked;
    counts.model_prompts_with_forge_content = model.saw_forge_content;

    // How the sandbox ended decides the job unless something above already did.
    let outcome = match (ended, sandbox.as_mut()) {
        (Some(outcome), sb) => {
            if let Some(sb) = sb {
                sb.stop();
            }
            outcome
        }
        (None, Some(sb)) => match sb.wait().ending {
            Ending::Exited(0) => JobOutcome::Succeeded,
            Ending::DeadlineExceeded => JobOutcome::DeadlineExceeded,
            Ending::Exited(_) | Ending::Stopped | Ending::Lost => JobOutcome::Failed,
        },
        (None, None) => JobOutcome::Failed,
    };
    drop(sandbox);
    if outcome != JobOutcome::Held {
        // The codes `hyalos run` records for the same endings (ADR-0053).
        let why = match outcome {
            JobOutcome::Succeeded => "harness.finished",
            JobOutcome::DeadlineExceeded => "job.deadline",
            JobOutcome::Failed | JobOutcome::Held => "sandbox.failed",
        };
        engine.submit(
            clock.now(),
            Origin::Engine,
            Event::JobEnded {
                job,
                outcome,
                reason: ReasonCode::new(why).ok_or("reason")?,
            },
        )?;
    }
    if outcome == JobOutcome::Succeeded {
        engine.submit(clock.now(), Origin::Engine, Event::TaskCompleted { task })?;
    }

    // The checks of ADR-0013: the state's invariants, the projection equal to a replay of the log,
    // and a log that verifies against the engine's public key alone.
    let violations = engine.state().invariant_violations();
    if !violations.is_empty() {
        return Err(format!("seed {seed}: {violations:?}").into());
    }
    let state = engine.state().clone();
    let head = engine.head()?.ok_or("empty log")?;
    drop(engine);
    let log = Log::open(&db, LogKey::from_seed(key_bytes(&sim, "log-key")))?;
    let events = log.events()?;
    if replay(&events)? != state {
        return Err(format!("seed {seed}: the projection is not a replay of the log").into());
    }
    log.verify(&public)?;
    drop(log);

    Ok(Run {
        seed,
        dir: dir.to_path_buf(),
        fixtures,
        events,
        head,
        outcome,
        counts,
        sandbox_env: substrate.started().into_iter().map(|s| s.env).collect(),
    })
}
