//! The broker serves the engine over the channel and admits only the engine's manifests (S3.11,
//! ADR-0002, ADR-0004, ADR-0035).

use std::cell::Cell;
use std::collections::BTreeSet;

use hyalos_broker::{Broker, Exit, ServeError, serve_with_plugins};
use hyalos_types::wire::{ToBroker, ToEngine, WIRE_VERSION, WireError, read_frame, write_frame};
use hyalos_types::{
    Budget, Clock, ForgeToken, JobId, MANIFEST_VERSION, Manifest, ManifestKey, ReasonCode, Role,
    Timestamp,
};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct Now(Cell<u64>);

impl Clock for Now {
    fn now(&self) -> Timestamp {
        Timestamp::from_millis(self.0.get())
    }
}

fn engine_key() -> ManifestKey {
    ManifestKey::from_seed([3; 32])
}

fn manifest(job: u64, role: Role, deadline: u64) -> Manifest {
    Manifest {
        version: MANIFEST_VERSION,
        job: JobId::from_seq(job),
        role,
        tainted: false,
        repo: "hyaloslabs/hyalos".into(),
        branch_prefix: "hyalos/".into(),
        pr_base: None,
        actions: BTreeSet::new(),
        egress_hosts: BTreeSet::new(),
        tools: BTreeSet::new(),
        data_flows: BTreeSet::new(),
        budget: Budget { tokens: 1 },
        deadline: Timestamp::from_millis(deadline),
        approval: None,
        handles: Default::default(),
        result: None,
        closes: None,
    }
}

fn start(key: &ManifestKey, m: &Manifest, role: Role) -> Res<ToBroker> {
    Ok(ToBroker::StartJob {
        job: m.job,
        role,
        manifest: key.sign(m)?.to_bytes()?,
        socket: "/unused".into(),
        forge_token: None,
    })
}

fn hello() -> ToBroker {
    ToBroker::Hello {
        version: WIRE_VERSION,
        manifest_key: engine_key().public(),
        forge: None,
    }
}

fn code(text: &str) -> Res<ReasonCode> {
    ReasonCode::new(text).ok_or_else(|| "bad code".into())
}

/// The frames of a conversation, as the engine would send them.
fn frames(messages: &[ToBroker]) -> Res<Vec<u8>> {
    let mut out = Vec::new();
    for m in messages {
        write_frame(&mut out, m)?;
    }
    Ok(out)
}

/// What the broker answers to a conversation, and how serving ended.
fn run(messages: &[ToBroker], now: u64) -> Res<(Result<Exit, ServeError>, Vec<ToEngine>)> {
    let input = frames(messages)?;
    let mut output = Vec::new();
    let result = serve_with_plugins(
        &mut &input[..],
        &mut output,
        &Now(Cell::new(now)),
        None,
        None,
    );
    let mut answers = Vec::new();
    let mut rest = &output[..];
    while let Some(a) = read_frame::<_, ToEngine>(&mut rest)? {
        answers.push(a);
    }
    Ok((result, answers))
}

fn ready() -> ToEngine {
    ToEngine::Ready {
        version: WIRE_VERSION,
    }
}

#[test]
fn it_admits_the_engines_manifest_and_enforces_what_it_says() -> Res {
    let m = manifest(5, Role::Actor, 10_000);
    let (result, answers) = run(
        &[
            hello(),
            start(&engine_key(), &m, Role::Actor)?,
            ToBroker::Shutdown,
        ],
        100,
    )?;
    assert_eq!(result, Ok(Exit::Shutdown));
    assert_eq!(
        answers,
        vec![
            ready(),
            ToEngine::Admitted {
                job: JobId::from_seq(5)
            }
        ]
    );
    // The manifest the broker holds is the one the engine signed, read back.
    let mut broker = Broker::new(engine_key().public());
    broker.handle(start(&engine_key(), &m, Role::Actor)?, &Now(Cell::new(100)))?;
    assert_eq!(broker.manifest(JobId::from_seq(5)), Some(&m));
    Ok(())
}

#[test]
fn it_refuses_a_manifest_the_engine_did_not_sign_and_goes_on_serving() -> Res {
    let other = ManifestKey::from_seed([4; 32]);
    let forged = manifest(5, Role::Actor, 10_000);
    let good = manifest(6, Role::Actor, 10_000);
    let (result, answers) = run(
        &[
            hello(),
            start(&other, &forged, Role::Actor)?,
            start(&engine_key(), &good, Role::Actor)?,
        ],
        100,
    )?;
    // The refusal fails that job; the next one is served, and ending the channel ends serving.
    assert_eq!(result, Ok(Exit::EngineClosed));
    assert_eq!(
        answers,
        vec![
            ready(),
            ToEngine::Refused {
                job: JobId::from_seq(5),
                reason: code("manifest.bad_signature")?
            },
            ToEngine::Admitted {
                job: JobId::from_seq(6)
            },
        ]
    );
    // And a refused job is not live afterwards.
    let mut broker = Broker::new(engine_key().public());
    broker.handle(start(&other, &forged, Role::Actor)?, &Now(Cell::new(100)))?;
    assert_eq!(broker.manifest(JobId::from_seq(5)), None);
    Ok(())
}

#[test]
fn every_check_of_admission_gives_its_own_code() -> Res {
    let key = engine_key();
    let live = manifest(5, Role::Actor, 10_000);
    // A manifest for another job under the credential of this one is not possible to send, so the
    // wrong-job case is a relabelled envelope: the engine signed job 9 and says it started job 5.
    let for_nine = manifest(9, Role::Actor, 10_000);
    let relabelled = ToBroker::StartJob {
        job: JobId::from_seq(5),
        role: Role::Actor,
        manifest: key.sign(&for_nine)?.to_bytes()?,
        socket: "/unused".into(),
        forge_token: None,
    };
    let mut wrong_version = key.sign(&live)?;
    wrong_version = hyalos_types::SignedManifest::from_parts(
        7,
        wrong_version.payload().to_vec(),
        *wrong_version.signature(),
    );
    let cases: Vec<(&str, ToBroker, u64, &str)> = vec![
        ("wrong job", relabelled, 100, "admission.wrong_job"),
        (
            "wrong role",
            start(&key, &live, Role::Reader)?,
            100,
            "admission.role_mismatch",
        ),
        (
            "expired",
            start(&key, &live, Role::Actor)?,
            10_001,
            "admission.expired",
        ),
        (
            "unknown version",
            ToBroker::StartJob {
                job: JobId::from_seq(5),
                role: Role::Actor,
                manifest: wrong_version.to_bytes()?,
                socket: "/unused".into(),
                forge_token: None,
            },
            100,
            "manifest.unknown_version",
        ),
        (
            "not an envelope",
            ToBroker::StartJob {
                job: JobId::from_seq(5),
                role: Role::Actor,
                manifest: b"not json".to_vec(),
                socket: "/unused".into(),
                forge_token: None,
            },
            100,
            "manifest.malformed",
        ),
    ];
    for (what, message, now, expected) in cases {
        let (result, answers) = run(&[hello(), message], now)?;
        assert_eq!(result, Ok(Exit::EngineClosed), "{what}");
        assert_eq!(
            answers,
            vec![
                ready(),
                ToEngine::Refused {
                    job: JobId::from_seq(5),
                    reason: code(expected)?
                }
            ],
            "{what}"
        );
    }
    // Exactly at the deadline is still valid.
    let (_, answers) = run(&[hello(), start(&key, &live, Role::Actor)?], 10_000)?;
    assert!(matches!(answers.get(1), Some(ToEngine::Admitted { .. })));
    Ok(())
}

#[test]
fn a_job_is_started_once_and_ended_jobs_are_not_admitted_again_without_a_new_start() -> Res {
    let key = engine_key();
    let m = manifest(5, Role::Actor, 10_000);
    let (_, answers) = run(
        &[
            hello(),
            start(&key, &m, Role::Actor)?,
            // The same job again while live: the first admission stands.
            start(&key, &m, Role::Actor)?,
            ToBroker::EndJob {
                job: JobId::from_seq(5),
            },
            // After it ended, the engine starting it again is a new start.
            start(&key, &m, Role::Actor)?,
        ],
        100,
    )?;
    let job = JobId::from_seq(5);
    assert_eq!(
        answers,
        vec![
            ready(),
            ToEngine::Admitted { job },
            ToEngine::Refused {
                job,
                reason: code("admission.already_live")?
            },
            ToEngine::Admitted { job },
        ]
    );
    // Ending a job drops its manifest.
    let mut broker = Broker::new(key.public());
    broker.handle(start(&key, &m, Role::Actor)?, &Now(Cell::new(100)))?;
    broker.handle(ToBroker::EndJob { job }, &Now(Cell::new(100)))?;
    assert_eq!(broker.manifest(job), None);
    Ok(())
}

#[test]
fn the_engine_must_say_hello_first_once_and_in_a_version_the_broker_speaks() -> Res {
    let m = manifest(5, Role::Actor, 10_000);
    let before = run(&[start(&engine_key(), &m, Role::Actor)?], 100)?;
    assert_eq!(before, (Err(ServeError::Order), vec![]));
    let twice = run(&[hello(), hello()], 100)?;
    assert_eq!(twice, (Err(ServeError::Order), vec![ready()]));
    let nothing = run(&[], 100)?;
    assert_eq!(nothing, (Err(ServeError::Order), vec![]));
    let newer = ToBroker::Hello {
        version: WIRE_VERSION + 1,
        manifest_key: engine_key().public(),
        forge: None,
    };
    assert_eq!(
        run(&[newer], 100)?,
        (
            Err(ServeError::Version {
                engine: WIRE_VERSION + 1
            }),
            vec![]
        )
    );
    Ok(())
}

#[test]
fn a_frame_that_is_not_a_message_ends_the_channel_and_is_not_served() -> Res {
    let mut input = frames(&[hello()])?;
    // A well-framed body that is not a message from the engine: here, one meant for the engine.
    let body = br#"{"kind":"admitted","job":5}"#;
    input.extend_from_slice(&(body.len() as u32).to_be_bytes());
    input.extend_from_slice(body);
    let mut output = Vec::new();
    let result = serve_with_plugins(&mut &input[..], &mut output, &Now(Cell::new(0)), None, None);
    assert_eq!(result, Err(ServeError::Wire(WireError::Malformed)));
    // A channel cut off inside a frame is a fault, not a clean close.
    let mut cut = frames(&[hello(), ToBroker::Shutdown])?;
    cut.truncate(cut.len() - 1);
    let result = serve_with_plugins(
        &mut &cut[..],
        &mut Vec::new(),
        &Now(Cell::new(0)),
        None,
        None,
    );
    assert_eq!(result, Err(ServeError::Wire(WireError::Truncated)));
    Ok(())
}

#[test]
fn a_refused_job_can_be_started_again_with_a_manifest_that_is_good() -> Res {
    let other = ManifestKey::from_seed([4; 32]);
    let m = manifest(5, Role::Actor, 10_000);
    let job = JobId::from_seq(5);
    let (_, answers) = run(
        &[
            hello(),
            start(&other, &m, Role::Actor)?,
            start(&engine_key(), &m, Role::Actor)?,
        ],
        100,
    )?;
    // The refusal did not leave the job marked live, so the retry is a first start, not a repeat.
    assert_eq!(
        answers,
        vec![
            ready(),
            ToEngine::Refused {
                job,
                reason: code("manifest.bad_signature")?
            },
            ToEngine::Admitted { job },
        ]
    );
    Ok(())
}

#[test]
fn the_forge_token_is_held_only_for_an_admitted_job_and_only_while_it_is_live() -> Res {
    // ADR-0044: the engine hands the job's token over with its manifest; the broker keeps it for the
    // job and forgets it when the job ends. A refused job leaves nothing behind.
    let key = engine_key();
    let now = Now(Cell::new(100));
    let token = ForgeToken::new("ghs_job5", Timestamp::from_millis(5_000)).ok_or("token")?;
    let with_token = |m: &Manifest, k: &ManifestKey| -> Res<ToBroker> {
        let ToBroker::StartJob {
            job,
            role,
            manifest,
            socket,
            ..
        } = start(k, m, Role::Actor)?
        else {
            return Err("not a start".into());
        };
        Ok(ToBroker::StartJob {
            job,
            role,
            manifest,
            socket,
            forge_token: Some(token.clone()),
        })
    };
    let mut broker = Broker::new(key.public());
    let m = manifest(5, Role::Actor, 10_000);
    broker.handle(with_token(&m, &key)?, &now)?;
    assert_eq!(broker.forge_token(m.job), Some(&token));
    broker.handle(ToBroker::EndJob { job: m.job }, &now)?;
    assert_eq!(broker.forge_token(m.job), None);

    // Signed by someone else: refused, and the token is not kept.
    let other = manifest(6, Role::Actor, 10_000);
    let answers = broker.handle(with_token(&other, &ManifestKey::from_seed([9; 32]))?, &now)?;
    assert!(matches!(
        answers.as_deref(),
        Some([ToEngine::Refused { .. }])
    ));
    assert_eq!(broker.forge_token(other.job), None);

    // A manifest that needs no forge access comes with no token.
    let plain = manifest(7, Role::Actor, 10_000);
    broker.handle(start(&key, &plain, Role::Actor)?, &now)?;
    assert_eq!(broker.forge_token(plain.job), None);
    Ok(())
}

/// Stands in for the broker process's plugin runtime: answers every call with one value and records
/// that it was asked.
#[derive(Debug, Default)]
struct Runs(std::sync::Mutex<usize>);

impl hyalos_broker::Plugins for Runs {
    fn run(
        &self,
        _: &hyalos_types::wire::PluginCall,
    ) -> Result<Vec<hyalos_types::workflow::TypedValue>, ReasonCode> {
        *self.0.lock().map_err(|_| ReasonCode::unavailable())? += 1;
        Ok(vec![hyalos_types::workflow::TypedValue::Bool(true)])
    }
}

/// S6.5 (ADR-0102): the broker runs a plugin only for a live deterministic job, with a call within
/// its limits, and only if its process gave it a runtime; anything else fails the job, not the broker.
#[test]
fn a_plugin_runs_only_for_a_live_deterministic_job() -> Res {
    use hyalos_types::wire::{MAX_PLUGIN_INPUT, PluginCall};
    use hyalos_types::workflow::{Field, TypedValue};
    let call = |input: String| -> Res<PluginCall> {
        Ok(PluginCall {
            store: "/store".into(),
            name: hyalos_types::PluginName::new("risk-score").ok_or("name")?,
            digest: hyalos_types::Digest::from_bytes([1; 32]),
            input,
            output: [("low".to_string(), Field::Bool)].into(),
        })
    };
    let runs = std::sync::Arc::new(Runs::default());
    let clock = Now(Cell::new(100));
    let key = engine_key();
    let mut broker = Broker::new(key.public()).with_plugins(runs.clone());
    let ask = |broker: &mut Broker, job: u64, input: String| -> Res<Vec<ToEngine>> {
        let message = ToBroker::RunPlugin {
            job: JobId::from_seq(job),
            call: call(input)?,
        };
        Ok(broker.handle(message, &clock)?.ok_or("stopped")?)
    };
    let refused = |job: u64, reason: &str| -> Res<Vec<ToEngine>> {
        Ok(vec![ToEngine::Refused {
            job: JobId::from_seq(job),
            reason: code(reason)?,
        }])
    };
    // Not a live job.
    assert_eq!(
        ask(&mut broker, 1, "{}".into())?,
        refused(1, "admission.not_live")?
    );
    // An actor's job never runs a plugin here.
    let actor = manifest(2, Role::Actor, 10_000);
    broker.handle(start(&key, &actor, Role::Actor)?, &clock)?;
    assert_eq!(
        ask(&mut broker, 2, "{}".into())?,
        refused(2, "plugin.wrong_role")?
    );
    // A deterministic job's call past its input limit is refused, and ends the job.
    let step = manifest(3, Role::Deterministic, 10_000);
    broker.handle(start(&key, &step, Role::Deterministic)?, &clock)?;
    let long = " ".repeat(MAX_PLUGIN_INPUT + 1);
    assert_eq!(ask(&mut broker, 3, long)?, refused(3, "plugin.bad_call")?);
    assert_eq!(
        ask(&mut broker, 3, "{}".into())?,
        refused(3, "admission.not_live")?
    );
    assert_eq!(*runs.0.lock().map_err(|_| "lock")?, 0);
    // A live deterministic job's call within its limits runs.
    let step = manifest(4, Role::Deterministic, 10_000);
    broker.handle(start(&key, &step, Role::Deterministic)?, &clock)?;
    assert_eq!(
        ask(&mut broker, 4, "{}".into())?,
        vec![ToEngine::Values {
            job: JobId::from_seq(4),
            values: vec![TypedValue::Bool(true)],
        }]
    );
    assert_eq!(*runs.0.lock().map_err(|_| "lock")?, 1);
    // A broker with no runtime refuses.
    let mut bare = Broker::new(key.public());
    bare.handle(start(&key, &step, Role::Deterministic)?, &clock)?;
    assert_eq!(
        ask(&mut bare, 4, "{}".into())?,
        refused(4, "plugin.no_runtime")?
    );
    Ok(())
}
