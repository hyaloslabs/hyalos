//! S4.7: Claude Code and Codex each return a job's result by running `hyalos-submit` in the sandbox
//! image, and the broker reads it as the job's signed manifest asks and gives the engine typed values
//! only (ADR-0016, ADR-0064). The scripted model asks each harness to run the command with its own
//! shell tool, so the path is the one a real job takes.
//!
//! What the harness says is "ok"; that is never the result. The control is a run where the model
//! asks for nothing: the harness exits 0 and the broker has no result to give. Like the image test,
//! it skips without the image unless `HYALOS_REQUIRE_SANDBOX_IMAGE` is set.

mod common;

use common::{Res, Spec, Wall, image_present, job_spec, sandboxed, scripted_model};
use hyalos_types::wire::{ToBroker, ToEngine};
use hyalos_types::workflow::{Field, Output, TypedValue};
use hyalos_types::{JobId, ReasonCode};

const RESULT: &str = r#"{"outcome":"no_change","tests_added":false}"#;

fn output() -> Output {
    [
        (
            "outcome".to_string(),
            Field::Choice(vec![
                "pr_opened".into(),
                "no_change".into(),
                "blocked".into(),
            ]),
        ),
        ("tests_added".to_string(), Field::Bool),
    ]
    .into()
}

#[test]
fn claude_code_and_codex_each_submit_a_result_the_broker_reads_as_typed_values() -> Res {
    if !image_present()? {
        return Ok(());
    }
    let model = scripted_model()?;
    let resolv = std::env::temp_dir().join(format!("hysub-{}-resolv.conf", std::process::id()));
    std::fs::write(&resolv, "nameserver 127.0.0.1\n")?;
    let harnesses: [(&str, &[&str]); 2] = [
        (
            "Claude Code",
            &[
                "claude",
                "-p",
                "report the result",
                "--tools",
                "Bash",
                "--allowedTools",
                "Bash",
            ],
        ),
        (
            "Codex",
            &[
                "codex",
                "exec",
                "--skip-git-repo-check",
                "report the result",
            ],
        ),
    ];
    let mut failures = Vec::new();
    let mut seq = 40u64;
    for (name, command) in harnesses {
        for asked in [true, false] {
            seq += 1;
            if let Ok(mut run) = model.run.lock() {
                *run = asked.then(|| format!("hyalos-submit '{RESULT}'"));
            }
            model.clear();
            let mut job = job_spec(
                model.port,
                seq,
                Spec {
                    result: Some(output()),
                    ..Spec::default()
                },
            )?;
            let (code, said) = sandboxed(&job.socket, &resolv, command)?;
            let posted = job
                .requests
                .lock()
                .map(|r| r.iter().any(|l| l == "POST /hyalos/submit HTTP/1.1"))
                .unwrap_or(false);
            let id = JobId::from_seq(seq);
            let taken = job.broker.handle(ToBroker::TakeResult { job: id }, &Wall)?;
            let expected = if asked {
                vec![ToEngine::Values {
                    job: id,
                    values: vec![TypedValue::Choice(1), TypedValue::Bool(false)],
                }]
            } else {
                vec![ToEngine::Refused {
                    job: id,
                    reason: ReasonCode::new("submit.missing").ok_or("code")?,
                }]
            };
            let report = format!(
                "{name}, asked to submit {asked}: exit {code}, posted {posted}, broker gave {taken:?}"
            );
            eprintln!("{report}");
            if code != 0 || posted != asked || taken.as_ref() != Some(&expected) {
                failures.push(format!("{report}\n{said}"));
            }
        }
    }
    if let Ok(mut run) = model.run.lock() {
        *run = None;
    }
    let _ = std::fs::remove_file(&resolv);
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    Ok(())
}
