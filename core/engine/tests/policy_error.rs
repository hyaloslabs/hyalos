//! S3.15: a policy that errors holds the task, and the hold is recorded with the gate, the digest of
//! the inputs and the reason (ADR-0008, ADR-0009).

use hyalos_engine::{Engine, Ruling, TaskStatus};
use hyalos_log::{Log, LogKey};
use hyalos_policy::{Evaluate, Fault, GateRequest, Policies, Query, Verdict, ask};
use hyalos_sim::{FakeClock, START};
use hyalos_types::{
    Clock, Digest, Event, Gate, JobId, Origin, Stakes, SubstrateName, TaskId, Timestamp,
};

type Res = Result<(), Box<dyn std::error::Error>>;

const BARE: &str = r#"[{"uid":{"type":"Resource","id":"r1"},"attrs":{},"parents":[]}]"#;
const UNMARKED: &str =
    r#"[{"uid":{"type":"Resource","id":"r1"},"attrs":{"sensitive":false},"parents":[]}]"#;

/// A baseline `forbid` that reads an attribute, and an operator `permit` beside it.
const POLICIES: &str = "
    forbid (principal, action, resource) when { resource.sensitive };
    permit (principal, action, resource);
";

/// The policy gate: asks Cedar about one resource.
struct PolicyGate<'a> {
    policies: &'a Policies,
    entities: &'a str,
}

impl Evaluate for PolicyGate<'_> {
    fn evaluate(&mut self, _: &GateRequest) -> Result<Verdict, Fault> {
        Ok(self.policies.judge(&Query {
            principal: r#"Job::"j1""#,
            action: r#"Action::"push""#,
            resource: r#"Resource::"r1""#,
            entities: self.entities,
        }))
    }
}

fn key() -> LogKey {
    LogKey::from_seed([6; 32])
}

struct Rig {
    engine: Engine,
    clock: FakeClock,
    task: TaskId,
    job: JobId,
    path: std::path::PathBuf,
}

fn rig(name: &str) -> Result<Rig, Box<dyn std::error::Error>> {
    let dir =
        std::env::temp_dir().join(format!("hyalos-policy-error-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("events.db");
    let clock = FakeClock::new(START);
    let mut engine = Engine::open(&path, key())?;
    let task = TaskId::from_seq(
        engine
            .submit(
                clock.now(),
                Origin::Engine,
                Event::TaskCreated {
                    request: Digest::from_bytes([1; 32]),
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
                    substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
                },
            )?
            .seq,
    );
    Ok(Rig {
        engine,
        clock,
        task,
        job,
        path,
    })
}

fn request() -> GateRequest {
    GateRequest {
        gate: Gate::Egress,
        inputs: Digest::from_bytes([0x5c; 32]),
        stakes: Stakes::routine(),
    }
}

impl Rig {
    fn ask_policies(&mut self, entities: &str) -> Result<Ruling, Box<dyn std::error::Error>> {
        let policies = Policies::parse(POLICIES)?;
        let deadline = Timestamp::from_millis(self.clock.now().as_millis() + 1_000);
        let outcome = ask(
            &mut PolicyGate {
                policies: &policies,
                entities,
            },
            &self.clock,
            deadline,
            &request(),
        );
        Ok(self.engine.rule_gate(
            self.clock.now(),
            Origin::Broker,
            self.job,
            &request(),
            outcome,
        )?)
    }
}

#[test]
fn a_policy_error_holds_the_task_and_records_the_gate_the_digest_and_the_reason() -> Res {
    let mut r = rig("error")?;
    let Ruling::Held(hold) = r.ask_policies(BARE)? else {
        return Err("a policy error must hold the task".into());
    };
    assert_eq!(r.engine.state().tasks()[&r.task].status(), TaskStatus::Held);
    assert!(r.engine.state().hold_is_open(&hold));

    // Three events, written together: the unknown answer, the job ending held, the hold opening.
    let events = Log::open(&r.path, key())?.events()?;
    let kinds: Vec<&str> = events.iter().skip(2).map(|e| e.kind.as_str()).collect();
    assert_eq!(kinds, ["gate.answered", "job.ended", "hold.opened"]);

    let answered = String::from_utf8_lossy(&events[2].payload).into_owned();
    for expected in [
        r#""answer":"unknown""#,
        r#""gate":"egress""#,
        r#""reason":"policy.error""#,
        &format!(r#""inputs":"{}""#, "5c".repeat(32)),
    ] {
        assert!(answered.contains(expected), "{expected} not in {answered}");
    }
    let opened = String::from_utf8_lossy(&events[4].payload).into_owned();
    assert!(
        opened.contains(&"5c".repeat(32)) && opened.contains("egress"),
        "{opened}"
    );
    Ok(())
}

#[test]
fn the_same_policies_with_a_clean_evaluation_let_the_request_through() -> Res {
    // The control: nothing is held because of the policies themselves, only because of the error.
    let mut r = rig("clean")?;
    assert!(matches!(r.ask_policies(UNMARKED)?, Ruling::Proceed));
    assert_eq!(r.engine.state().tasks()[&r.task].status(), TaskStatus::Open);
    Ok(())
}
