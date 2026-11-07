//! S5.9: `hyalos merge` records the operator's signed approval, bound to the head they reviewed, and
//! merges with a token minted for that merge alone and revoked after it (ADR-0012, ADR-0071).

#![allow(dead_code)]

mod common;

use common::{GitHub, MERGE_COMMIT, PR_HEAD, Res, World, ok_cli};
use hyalos_types::{Event, JobOutcome, MergeMethod, Resolution, Signed};

fn merges(world: &World) -> Res<Vec<Event>> {
    Ok(world
        .events()?
        .into_iter()
        .filter(|e| !matches!(e, Event::PolicyApproved { .. }))
        .collect())
}

#[test]
fn an_approved_merge_is_signed_merged_at_the_head_and_its_token_revoked() -> Res {
    let root = common::dir("merge-ok")?;
    let gh = GitHub::start(&root, false)?;
    let world = World::new("merge-ok", Some(&gh))?;
    let out = ok_cli(world.signed(&[
        "merge",
        "octo/repo",
        "42",
        "--head",
        PR_HEAD,
        "--method",
        "squash",
    ])?)?;
    assert!(
        out.contains(&format!("merged {PR_HEAD} as {MERGE_COMMIT}")),
        "{out}"
    );
    assert!(out.contains("forge token revoked"), "{out}");
    // The token was minted with the merge's permissions only, and is revoked.
    assert!(gh.issued().is_empty());
    let seen = gh.seen();
    let asked = seen
        .iter()
        .find(|s| s.path == "/app/installations/77/access_tokens")
        .ok_or("no token asked for")?;
    let asked: serde_json::Value = serde_json::from_slice(&asked.body)?;
    assert_eq!(
        asked["permissions"],
        serde_json::json!({ "contents": "write", "pull_requests": "write" })
    );
    let put = seen
        .iter()
        .find(|s| s.method == "PUT")
        .ok_or("no merge asked for")?;
    assert_eq!(put.path, "/repos/octo/repo/pulls/42/merge");
    let body: serde_json::Value = serde_json::from_slice(&put.body)?;
    assert_eq!(
        body,
        serde_json::json!({ "sha": PR_HEAD, "merge_method": "squash" })
    );
    // The log: a task of its own, the signed approval, one broker job that succeeded, the task done.
    let events = merges(&world)?;
    let [
        Event::TaskCreated { .. },
        Event::MergeApproved {
            task,
            repo,
            pr,
            head,
            method,
            by,
            signed,
            ..
        },
        Event::JobStarted { task: job_task, .. },
        Event::JobEnded {
            outcome, reason, ..
        },
        Event::TaskCompleted { task: done },
    ] = events.as_slice()
    else {
        return Err(format!("{events:?}").into());
    };
    assert_eq!(
        (repo.as_str(), *pr, head.as_str()),
        ("octo/repo", 42, PR_HEAD)
    );
    assert_eq!((*method, *by), (MergeMethod::Squash, Resolution::Human));
    assert!(matches!(signed, Signed::Operator(_)), "{signed:?}");
    assert_eq!((job_task, done), (task, task));
    assert_eq!(
        (*outcome, reason.as_str()),
        (JobOutcome::Succeeded, "merge.merged")
    );
    // And verify checks the operator's signature on it.
    ok_cli(world.cli(&["log", "verify"])?)?;
    Ok(())
}

#[test]
fn a_head_that_moved_is_refused_by_github_and_recorded_as_such() -> Res {
    let root = common::dir("merge-moved")?;
    let gh = GitHub::start(&root, false)?;
    let world = World::new("merge-moved", Some(&gh))?;
    let reviewed = "f".repeat(40);
    let output = world.signed(&["merge", "octo/repo", "42", "--head", &reviewed])?;
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("merge.head_moved"), "{stderr}");
    // GitHub's own words never reach the operator or the log.
    for text in [&stdout, &stderr] {
        assert!(!text.contains("Head branch was modified"), "{text}");
    }
    assert!(stdout.contains("forge token revoked"), "{stdout}");
    assert!(gh.issued().is_empty());
    let ended = merges(&world)?
        .into_iter()
        .find_map(|e| match e {
            Event::JobEnded {
                outcome, reason, ..
            } => Some((outcome, reason)),
            _ => None,
        })
        .ok_or("no job ended")?;
    assert_eq!(
        (ended.0, ended.1.as_str()),
        (JobOutcome::Failed, "merge.head_moved")
    );
    assert!(matches!(
        merges(&world)?.last(),
        Some(Event::TaskCompleted { .. })
    ));
    Ok(())
}

#[test]
fn a_wrong_passphrase_or_a_bad_request_writes_nothing_and_asks_nothing() -> Res {
    let root = common::dir("merge-refused")?;
    let gh = GitHub::start(&root, false)?;
    let world = World::new("merge-refused", Some(&gh))?;
    let before = world.events()?.len();
    let wrong = world.signed_with(
        &["merge", "octo/repo", "42", "--head", PR_HEAD],
        "not the operator's passphrase",
    )?;
    assert_eq!(wrong.status.code(), Some(1));
    for args in [
        vec!["merge", "octo/repo", "42"],
        vec!["merge", "octo/repo", "42", "--head", "abc"],
        vec![
            "merge",
            "octo/repo",
            "42",
            "--head",
            PR_HEAD,
            "--method",
            "ff",
        ],
        vec!["merge", "not-a-repo", "42", "--head", PR_HEAD],
    ] {
        let output = world.signed(&args)?;
        assert_eq!(output.status.code(), Some(2), "{args:?}");
    }
    assert_eq!(world.events()?.len(), before);
    assert!(gh.seen().is_empty());
    Ok(())
}
