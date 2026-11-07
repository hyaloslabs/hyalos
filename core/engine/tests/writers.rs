//! S5.2: several processes write one log (ADR-0069). Each engine projects the log in memory, so before
//! it checks an event of its own it reads, verifies and projects what the others appended, and it
//! appends only onto the events it has seen. A job's typed values are recorded with its ending.

use std::path::PathBuf;

use hyalos_engine::{Engine, EngineError, RuleViolation, replay};
use hyalos_log::{Log, LogKey};
use hyalos_types::{
    Attestation, CredentialTier, Digest, Event, JobId, JobOutcome, Origin, ReasonCode,
    SubstrateName, TaskId, Timestamp, TypedValue,
};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn key() -> LogKey {
    LogKey::from_seed([3; 32])
}

fn at(n: u64) -> Timestamp {
    Timestamp::from_millis(1_000 + n)
}

fn code(text: &str) -> ReasonCode {
    ReasonCode::new(text).unwrap_or_else(ReasonCode::unavailable)
}

fn log(test: &str) -> Res<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("writers-{test}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("events.db"))
}

fn task(e: &mut Engine, n: u64) -> Res<TaskId> {
    let head = e.submit(
        at(n),
        Origin::Engine,
        Event::TaskCreated {
            request: Digest::from_bytes([1; 32]),
        },
    )?;
    Ok(TaskId::from_seq(head.seq))
}

fn job(e: &mut Engine, n: u64, task: TaskId) -> Res<JobId> {
    let head = e.submit(
        at(n),
        Origin::Engine,
        Event::JobStarted {
            task,
            substrate: SubstrateName::new("broker").unwrap_or_else(SubstrateName::unrecorded),
            credentials: CredentialTier::BrokerHeld,
            attestation: Attestation::no_hardware_root(),
        },
    )?;
    Ok(JobId::from_seq(head.seq))
}

fn ended(job: JobId) -> Event {
    Event::JobEnded {
        job,
        outcome: JobOutcome::Succeeded,
        reason: code("step.answered"),
    }
}

fn violation<T: std::fmt::Debug>(result: Result<T, EngineError>) -> Res<RuleViolation> {
    match result {
        Err(EngineError::Rule(v)) => Ok(v),
        other => Err(format!("expected a refusal, got {other:?}").into()),
    }
}

#[test]
fn two_engines_on_one_log_each_see_the_others_events_before_writing() -> Res {
    let path = log("two")?;
    let mut a = Engine::open(&path, key())?
        .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
    let mut b = Engine::open(&path, key())?
        .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
    let first = task(&mut a, 1)?;
    // B never saw the task, yet starts a job on it: it reads A's event first.
    let started = job(&mut b, 2, first)?;
    assert_eq!(b.state().jobs()[&started].task, first);
    // A then ends B's job with its values, in one write, having read B's event.
    a.submit_all(
        at(3),
        &[
            (
                Origin::Engine,
                Event::JobValues {
                    job: started,
                    values: vec![TypedValue::Choice(1), TypedValue::Bool(true)],
                },
            ),
            (Origin::Engine, ended(started)),
        ],
    )?;
    let second = task(&mut b, 4)?;
    assert_eq!(second.seq(), 5);

    let all = Log::open(&path, key())?.events()?;
    let replayed = replay(&all)?;
    assert_eq!(a.state().last_seq(), 4);
    // A has not written since B's last event; it catches up on its next write, or on reopening.
    drop(a);
    assert_eq!(
        Engine::open(&path, key())?
            .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]))
            .state(),
        &replayed
    );
    assert_eq!(b.state(), &replayed);
    let done = &replayed.jobs()[&started];
    assert_eq!(
        done.values,
        Some(vec![TypedValue::Choice(1), TypedValue::Bool(true)])
    );
    assert_eq!(done.reason, Some(code("step.answered")));
    Ok(())
}

#[test]
fn an_event_stamped_before_another_processes_last_one_is_refused_and_nothing_is_written() -> Res {
    let path = log("time")?;
    let mut a = Engine::open(&path, key())?
        .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
    let mut b = Engine::open(&path, key())?
        .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
    task(&mut a, 10)?;
    // B's clock says 5, but the log already holds an event at 10 that B had not read.
    let v = violation(b.submit(
        at(5),
        Origin::Engine,
        Event::TaskCreated {
            request: Digest::from_bytes([2; 32]),
        },
    ))?;
    assert!(matches!(v, RuleViolation::TimeWentBackwards { .. }), "{v}");
    let all = Log::open(&path, key())?.events()?;
    assert_eq!(all.len(), 1);
    replay(&all)?;
    Ok(())
}

#[test]
fn a_job_returns_values_once_while_it_runs_and_no_more_than_an_output_holds() -> Res {
    let path = log("values")?;
    let mut e = Engine::open(&path, key())?
        .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
    let t = task(&mut e, 1)?;
    let j = job(&mut e, 2, t)?;
    let values = |n: usize| Event::JobValues {
        job: j,
        values: vec![TypedValue::Integer(1); n],
    };
    let v = violation(e.submit(at(3), Origin::Engine, values(17)))?;
    assert!(matches!(v, RuleViolation::TooManyValues(_)), "{v}");
    e.submit(at(3), Origin::Engine, values(16))?;
    let v = violation(e.submit(at(4), Origin::Engine, values(1)))?;
    assert!(matches!(v, RuleViolation::JobValuesTwice(_)), "{v}");
    e.submit(at(5), Origin::Engine, ended(j))?;

    let k = job(&mut e, 6, t)?;
    e.submit(at(7), Origin::Engine, ended(k))?;
    let after = Event::JobValues {
        job: k,
        values: vec![TypedValue::Bool(false)],
    };
    let v = violation(e.submit(at(8), Origin::Engine, after.clone()))?;
    assert!(matches!(v, RuleViolation::JobNotRunning(_)), "{v}");
    let v = violation(e.submit(at(8), Origin::Broker, after))?;
    assert!(matches!(v, RuleViolation::OriginNotAllowed { .. }), "{v}");

    // Kept through compaction, as the run that resumes reads them from the state.
    let before = e.state().clone();
    e.compact(at(9), &path.with_file_name("archive.db"))?;
    drop(e);
    assert_eq!(
        Engine::open(&path, key())?
            .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]))
            .state(),
        &before
    );
    Ok(())
}

#[test]
fn an_engine_that_stamps_forward_follows_another_processes_later_event() -> Res {
    let path = log("forward")?;
    let mut a = Engine::open(&path, key())?
        .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
    let mut b = Engine::open(&path, key())?
        .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
    b.stamp_forward();
    task(&mut a, 10)?;
    // B's clock says 5: it is stamped at A's 10 instead of refused.
    let head = task(&mut b, 5)?;
    let all = Log::open(&path, key())?.events()?;
    assert_eq!(all.len(), 2);
    assert_eq!(head.seq(), 2);
    assert_eq!(b.state().last_at(), Some(at(10)));
    replay(&all)?;
    Ok(())
}
