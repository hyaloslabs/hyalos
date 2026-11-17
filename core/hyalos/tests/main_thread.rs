//! The policy engine answers from the thread the binary runs its commands on (#280).
//!
//! This test has no harness (`harness = false`), so it starts on a process's main thread, as the
//! binary does. Built for musl, Cedar refuses on that thread: its stack is reported as only the
//! part mapped so far. The release check runs this built for musl, where it shows that `stack::run`
//! gives Cedar a stack it can read. That `main` runs every command through `stack::run` is the
//! binary's to keep: no command judges a request without a job, and `hyalos policy check` only
//! validates, which never reaches Cedar's check (v0.1.0 passes it).

use std::process::ExitCode;

use hyalos_policy::{Action, JobFacts, Policies, PolicyRequest, TargetFacts};
use hyalos_types::Answer;

const STARTER: &str = include_str!("../policies/starter.cedar");

/// The starter policy's answer to a fix-issue job's first step: reading its repository.
fn read_answer() -> Result<Answer, String> {
    let policies = Policies::load(STARTER).map_err(|e| format!("{e:?}"))?;
    let request = PolicyRequest {
        action: Action::Read,
        job: JobFacts {
            tainted: false,
            repo: "owner/name".to_string(),
            budget_tokens: 32_000,
        },
        target: TargetFacts {
            repo: "owner/name".to_string(),
            name: String::new(),
            in_branch_prefix: true,
            protected: false,
        },
        approved: false,
        subscription_token: false,
        writers_only: false,
    };
    Ok(policies.judge_request(&request).answer)
}

fn main() -> ExitCode {
    match hyalos::stack::run(read_answer) {
        Ok(Ok(Answer::Allow)) => {
            println!("main_thread: the policy engine answers on the command thread");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("main_thread: the starter policy should allow a read, and gave {other:?}");
            ExitCode::FAILURE
        }
    }
}
