# ADR-0053: An interrupted run ends its job

**Status:** Accepted

## Context

[ADR-0050](0050-running-one-job-from-the-command-line.md) left this undone: "An interrupted run is not cleaned up." If the operator presses Ctrl-C during `hyalos run`, or the command gets `SIGTERM`, the process dies at once. That leaves three problems:
- The job's GitHub token stays valid until it expires, within an hour ([ADR-0044](0044-minting-per-job-github-tokens-from-the-app.md)).
- The log holds a job that started and never ended.
- The broker gets the same Ctrl-C, because a terminal sends it to every process in the foreground process group. So the broker dies before the engine could end the job in it.

S3.10c (#193) asks the run to end the job however it is stopped, as long as the operator lets it.

`job.ended` records only the outcome (`succeeded`, `failed`, `deadline_exceeded`, `held`). An interrupted job would read as `failed`, the same as a harness that crashed. The acceptance asks for a reason code for the interrupt, and `job.ended` has nowhere to put one.

## Decision

**The first signal ends the job; it does not end the process.** For the whole of a run, `hyalos run` takes `SIGINT` and `SIGTERM` on a thread of its own (`hyalos::interrupt::Interrupt`, built on `signal-hook`, since the workspace forbids unsafe code). The first signal marks the run interrupted. The run checks the mark:
- while it finds a substrate;
- before it records the job;
- before it mints the token, starts the broker and starts the sandbox;
- every 200 ms while the sandbox runs.

Once it sees the mark, the run ends the job the way it ends any job. It stops the sandbox, sends `end_job` and `shutdown` to the broker, revokes the token, and records `job.ended` as `failed` with the reason `run.interrupted`. It then exits 1. Before anything is recorded, an interrupt refuses the way any other refusal does: the run exits 1 and writes nothing.

A wait that is already under way finishes first: up to 20 seconds for the broker to start, and up to 30 seconds for one of its answers. The broker answers well within either. So an interrupt during those steps is seen a moment later, not lost.

**A second signal exits at once.** The operator may not want to wait for a broker or for GitHub. Any signal after the first prints:
- that the job's GitHub token may still be live, and how to revoke it;
- that the log does not record how the job ended;
- that the sandbox may still be running until its deadline.

The run then exits 130 without cleaning up. The broker in its own group stops anyway, because its channel closes when the engine exits ([ADR-0035](0035-the-private-channel-between-engine-and-broker.md)).

**The broker and the sandbox runtime are not in the engine's process group.** `BrokerProcess` starts the broker with `process_group(0)`. The container driver starts `podman start --attach` (or `docker`) the same way, as the local-process fake already did. A Ctrl-C at the terminal then reaches only the engine. The engine decides when the broker and the sandbox stop: the broker stops on `shutdown` or when its channel closes, and the sandbox is stopped by the engine. `SIGTERM` sent to the engine's pid never reached them anyway.

**A sandbox can be stopped while another thread waits on it.** The run waits for the sandbox on a thread, and `Sandbox::wait` holds the sandbox. So `Sandbox` gains `stop_handle()`, which returns a `StopHandle`: a shared flag that `wait` checks on each poll. Setting it ends the sandbox as `Sandbox::stop` would, and `wait` returns `Ending::Stopped`. A handle used before `wait` is called stops the sandbox as soon as `wait` runs. Every driver implements it, and the contract suite checks both cases ([ADR-0032](0032-the-substrate-interface-and-its-contract.md)).

The container driver kills a stopping or overdue sandbox again each second until it ends. A kill sent while the runtime is still starting the container can otherwise be lost, and the sandbox would run on to its deadline.

**`job.ended` says why, in a reason code.** It gains a required `reason` (`ReasonCode`). This is schema version 8, and an older `job.ended` reads as `legacy.unrecorded` ([ADR-0023](0023-event-schema-versions-and-upcasting.md)). `hyalos run` records:

| Code | When |
|---|---|
| `harness.finished` | the harness exited 0 (`succeeded`) |
| `job.deadline` | the deadline passed (`deadline_exceeded`) |
| `sandbox.failed` | the sandbox ended any other way |
| `sandbox.not_started` | the substrate did not start it |
| `broker.stopped` | the broker closed its channel during the job |
| `broker.not_started` | the broker did not start or did not answer |
| `manifest.refused` | no manifest key, or the manifest was not minted |
| `forge.no_token` | GitHub did not issue a token |
| `forge.issue_unread` | the issue could not be read and the broker gave no code |
| `run.interrupted` | the operator interrupted the run |

When the broker refuses (at admission, reading the issue, or during the job), its own code is recorded, such as `forge.no_such_issue`. A job that holds ends with the gate's reason, in the same write as the hold ([ADR-0020](0020-gate-answers-timeouts-and-holds.md)). An interrupt outranks every other reason except a hold, and one thing more: a harness that exited 0 before the stop took effect did its work, so that job is recorded as `succeeded` (`harness.finished`). An interrupt that comes after the sandbox has ended changes nothing, since all that is left is the cleanup the run does anyway.

**A failed revoke is reported as at any end.** It uses the same message, `REVOKING THE FORGE TOKEN FAILED`, says what to do, and makes the run exit 1.

## Consequences

An operator can stop a run at any point and, unless they insist, the token is revoked and the log says how the job ended. The record no longer collapses a crash, a refusal and an interrupt into one `failed`.

The tests:
- `core/hyalos/tests/run_interrupt.rs` (its own test executable, so its signals reach no other test). The test sends the process `SIGINT` while a sandbox waits. It checks that the sandbox was stopped, that the stand-in GitHub saw the revoke and has no live token, and that `job.ended` is `failed` with `run.interrupted`. Then it sends `SIGTERM` twice, with a revoke that fails. It checks that the second signal reached `again`, that the failed revoke is reported, and that the end is recorded. It also checks that the broker leads a process group of its own.
- `crates/substrate-contract`: a sandbox stopped through its handle while another thread waits, and stopped before anyone waits.
- `core/types/tests/schema.rs` pins version 8. `core/engine/tests/versions.rs` uses the real `job.ended` step.

In the binary, a second signal calls `std::process::exit`, which a test in the same process cannot do. The test checks that the second signal reaches the callback; that the callback exits is read from the code.

Not here:
- **`SIGHUP`**, such as a closed terminal. It still kills the run as before. Taking it as well is one more signal on the same thread if it is wanted.
- **Interrupting the long-running engine**, which will have its own shutdown.
- **Finding what a second signal left behind**: a sandbox still running, a token still live. `hyalos doctor` could list sandboxes left by the substrate (`Substrate::existing`).
