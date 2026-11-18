//! The fakes on their own: what each does for a seed, and that each refuses a credential that is not
//! its own (ADR-0013, ADR-0036).

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use hyalos_policy::{Evaluate, GateRequest};
use hyalos_sim::{START, Sim};
use hyalos_substrate::{Ending, Substrate};
use hyalos_types::{Answer, Clock, Digest, Gate, JobId, Stakes};
use hyalos_world::{
    FakeForge, FakeModel, Fault, Fixtures, Forge, Model, SeededGate, SeededSubstrate,
};

fn forge(seed: u64) -> (FakeForge, hyalos_sim::FakeClock, Fixtures) {
    let sim = Sim::new(seed);
    let fx = Fixtures::for_seed(seed);
    let clock = sim.clock();
    (
        FakeForge::new(sim.stream("forge"), clock.clone(), &fx),
        clock,
        fx,
    )
}

#[test]
fn the_forge_serves_only_its_own_token_and_takes_fake_time() {
    let (mut f, clock, fx) = forge(3);
    assert_eq!(f.read_issue("wrong", 1), Err(Fault::Unauthorized));
    assert_eq!(
        f.open_pr("wrong", "b").map(|_| ()),
        Err(Fault::Unauthorized)
    );
    assert!(clock.now() > START, "a refused request still takes time");
    // A good token is served, or fails the way a forge does; never refused as unauthorized.
    let mut served = 0;
    let mut failed = 0;
    for n in 0..200 {
        match f.read_issue(&fx.forge_token, n) {
            Ok(text) => {
                served += 1;
                assert!(text.contains(&fx.forge_content));
                assert!(text.starts_with(&format!("Issue #{n}: ")));
            }
            Err(Fault::Unavailable) => failed += 1,
            Err(Fault::Unauthorized) => panic!("the right token was refused"),
        }
    }
    assert!(
        served > 100 && failed > 0 && failed < 60,
        "{served} served, {failed} failed"
    );
    assert_eq!(f.requests, 202);
}

#[test]
fn the_forge_repeats_for_a_seed_and_differs_between_seeds() {
    let take = |seed| {
        let (mut f, _, fx) = forge(seed);
        (0..30)
            .map(|n| f.read_issue(&fx.forge_token, n).ok().map(|t| t.len()))
            .collect::<Vec<_>>()
    };
    assert_eq!(take(1), take(1));
    assert_ne!(take(1), take(2));
}

#[test]
fn pull_requests_have_a_head_commit() -> Result<(), Box<dyn std::error::Error>> {
    let (mut f, _, fx) = forge(4);
    let mut heads = BTreeSet::new();
    for _ in 0..50 {
        if let Ok(pr) = f.open_pr(&fx.forge_token, "hyalos/x") {
            assert_eq!(pr.head_sha.len(), 40);
            assert!(pr.head_sha.bytes().all(|b| b.is_ascii_hexdigit()));
            assert!((1..=5000).contains(&pr.number));
            heads.insert(pr.head_sha);
        }
    }
    assert!(heads.len() > 30);
    Ok(())
}

#[test]
fn the_model_serves_only_its_own_key_and_counts_tokens_by_the_prompt() {
    let sim = Sim::new(8);
    let fx = Fixtures::for_seed(8);
    let mut m = FakeModel::new(sim.stream("model"), sim.clock(), &fx);
    assert_eq!(
        m.complete("wrong", "hi").map(|_| ()),
        Err(Fault::Unauthorized)
    );
    let long = format!("{} {}", fx.forge_content, "x".repeat(40_000));
    let (mut short_tokens, mut long_tokens) = (u64::MAX, 0);
    let mut ok = 0;
    for _ in 0..100 {
        if let Ok(r) = m.complete(&fx.provider_key, "hi") {
            assert!(r.text.contains(&fx.model_text));
            short_tokens = short_tokens.min(r.tokens);
            ok += 1;
        }
        if let Ok(r) = m.complete(&fx.provider_key, &long) {
            long_tokens = long_tokens.max(r.tokens);
        }
    }
    assert!(ok > 60 && ok < 100, "{ok} of 100 served");
    assert!(short_tokens >= 500);
    assert!(
        long_tokens >= 500 + 10_000,
        "a long prompt costs more: {long_tokens}"
    );
    assert!(
        m.saw_forge_content > 0,
        "forge content in a prompt is noticed"
    );
    assert_eq!(m.calls, 201);
}

#[test]
fn a_longer_prompt_costs_exactly_its_length_in_quarters_more() {
    let sim = Sim::new(21);
    let fx = Fixtures::for_seed(21);
    // Two models on the same seed draw the same numbers, so the only difference is the prompt.
    let mut a = FakeModel::new(sim.stream("model"), sim.clock(), &fx);
    let mut b = FakeModel::new(sim.stream("model"), sim.clock(), &fx);
    let (short, long) = ("hi".to_string(), "x".repeat(4_002));
    let mut compared = 0;
    for _ in 0..50 {
        let (x, y) = (
            a.complete(&fx.provider_key, &short),
            b.complete(&fx.provider_key, &long),
        );
        if let (Ok(x), Ok(y)) = (x, y) {
            assert_eq!(y.tokens - x.tokens, (4_002 - 2) / 4);
            compared += 1;
        }
    }
    assert!(compared > 30);
}

#[test]
fn the_seeded_gate_gives_every_answer_and_takes_time() -> Result<(), Box<dyn std::error::Error>> {
    let sim = Sim::new(11);
    let mut g = SeededGate::new(sim.stream("gate"), sim.clock());
    let request = GateRequest {
        gate: Gate::Egress,
        inputs: Digest::from_bytes([1; 32]),
        stakes: Stakes::routine(),
    };
    let mut seen = BTreeSet::new();
    let mut faults = 0;
    let mut slowest = Duration::ZERO;
    for _ in 0..400 {
        let before = sim.clock().now();
        match g.evaluate(&request) {
            Ok(v) => {
                seen.insert(format!("{:?}", v.answer));
            }
            Err(_) => faults += 1,
        }
        slowest = slowest.max(Duration::from_millis(
            sim.clock().now().as_millis() - before.as_millis(),
        ));
    }
    assert_eq!(seen.len(), 3, "{seen:?}");
    assert!(seen.contains(&format!("{:?}", Answer::Unknown)));
    assert!(faults > 0);
    assert!(
        slowest >= Duration::from_millis(400),
        "some answers are too slow for a gate's deadline"
    );
    assert_eq!(g.asked, 400);
    Ok(())
}

fn spec(job: u64) -> hyalos_substrate::SandboxSpec {
    hyalos_substrate::SandboxSpec::new(
        JobId::from_seq(job),
        "sim/actor:1",
        vec!["run".into()],
        PathBuf::from("/run/hyalos/sim.sock"),
    )
}

#[test]
fn the_seeded_substrate_ends_sandboxes_as_the_seed_says() -> Result<(), Box<dyn std::error::Error>>
{
    let take = |seed| {
        let sim = Sim::new(seed);
        let s = SeededSubstrate::new(sim.stream("substrate"), sim.clock());
        (0..200)
            .map(|n| match s.start(&spec(n + 1)) {
                Ok(mut sb) => Some(sb.wait().ending),
                Err(_) => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(take(2), take(2));
    assert_ne!(take(2), take(3));
    let endings = take(2);
    assert!(
        endings.iter().any(|e| e.is_none()),
        "no start was ever refused"
    );
    for want in [
        Ending::Exited(0),
        Ending::Exited(1),
        Ending::DeadlineExceeded,
        Ending::Lost,
    ] {
        assert!(endings.contains(&Some(want)), "never {want:?}");
    }
    Ok(())
}

#[test]
fn the_seeded_substrate_refuses_a_bad_spec_and_tracks_what_is_live()
-> Result<(), Box<dyn std::error::Error>> {
    let sim = Sim::new(5);
    let s = SeededSubstrate::new(sim.stream("substrate"), sim.clock());
    let mut bad = spec(1);
    bad.image = "-rm".into();
    assert!(matches!(
        s.start(&bad),
        Err(hyalos_substrate::StartError::Spec(_))
    ));
    assert!(s.started().is_empty(), "a refused spec is not recorded");

    let mut sandbox = loop {
        if let Ok(sb) = s.start(&spec(7)) {
            break sb;
        }
    };
    assert_eq!(s.existing()?, vec![sandbox.id().to_string()]);
    assert!(s.started().iter().all(|sp| sp.job == JobId::from_seq(7)));
    sandbox.stop();
    assert!(s.existing()?.is_empty());
    // Waiting for a sandbox that was stopped reports that it was stopped.
    assert_eq!(sandbox.wait().ending, Ending::Stopped);
    assert!(s.existing()?.is_empty());
    Ok(())
}
