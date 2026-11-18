# ADR-0104: Fault schedules, swept in CI

**Status:** Accepted. Supersedes [ADR-0036](0036-the-simulated-world-and-the-log-scan.md)'s rule that failure rates are fixed in the code.

## Context

S8.1 (#50). [ADR-0013](0013-deterministic-simulation-and-evidence-gated-releases.md) decides that the engine runs under a fake clock, fake boundaries and seeded randomness, "replaying thousands of fault schedules in CI". [ADR-0036](0036-the-simulated-world-and-the-log-scan.md) built the world that does this for one job: four fakes on their own streams and `run_job`, which checks each run. Three things were still missing:
- **The faults were fixed.** ADR-0036 fixed each failure rate in the code so that every path occurs within 150 seeds. So every seed ran under the same mix: no run had a calm world, a boundary that went down mid-job and stayed down, or a storm of faults.
- **Few seeds ran.** The tests run 150 seeds inside `cargo test`, which already takes most of the required job's 20 minutes.
- **There was no single command.** A failing seed could be replayed only by editing a test.

## Decision

**A seed draws a fault schedule, then the run.** `Schedule::for_seed` draws from a stream of its own, `schedule`, which no fake draws from. So the seed alone fixes both the schedule and the run, and adding schedules changes no fake's draws. Each schedule has one of four profiles:

| Profile | Share | What fails |
| --- | --- | --- |
| Calm | 1/8 | Nothing. Every gate allows on time, and every sandbox exits 0. |
| Usual | 2/8 | ADR-0036's fixed rates. |
| Outage | 2/8 | The usual rates, plus one boundary (forge, model, gate or substrate) down for good after 0 to 3 calls. A substrate outage is a runtime that refuses the start. |
| Storm | 3/8 | Each rate drawn, from none up to about half the calls. |

**The fakes take their rates from the schedule.** A rate is `hits` in `of`, decided by one `below(of)` draw. A gate's answer and a sandbox's ending are slots in one draw. Each fake defaults to the usual schedule, which reproduces ADR-0036's draws exactly. So `run_job(seed)` gives the same log byte for byte as before, and the existing 150-seed tests are unchanged. `run_with(seed, schedule, dir)` runs any schedule.

**Every run is checked against properties that hold under any schedule.** These are on top of what `run_with` already checks (ADR-0009's invariants, the projection equal to a replay of the log, a log that verifies):
- every question to a gate is answered in the log, so a fault skips none;
- a held job has a `hold.opened`, and only a succeeded job completes its task, last;
- an expired manifest ends the job as `DeadlineExceeded` ([ADR-0004](0004-one-signed-manifest-per-job.md));
- a job over its token budget does not succeed;
- the sandbox holds the placeholder credential and nothing else ([ADR-0016](0016-agent-runtime-and-broker-protocols.md));
- nothing planted reaches a file the run left ([ADR-0036](0036-the-simulated-world-and-the-log-scan.md)).

Each profile adds a property of its own:
- **Gate outage:** once the gate is down, the next question holds the task ([ADR-0009](0009-gates-answer-allow-deny-unknown.md), [ADR-0020](0020-gate-answers-timeouts-and-holds.md): fail closed).
- **Calm:** no fault and no refusal, and a job fails only by overspending.
- **Forge or model outage:** nothing is served after the outage begins.
- **Substrate outage:** no sandbox starts, and the job fails.

The tests break each property on purpose and require the check to refuse the run, so the check cannot pass by checking nothing.

**A sweep must reach everything it is for.** `sweep` runs a range of seeds on threads, each in its own directory. It keeps the directory of a failing seed and removes the rest. A sweep fails if any seed fails. It also fails if it leaves a gap: a profile no seed drew, an outcome no run ended with, or a gate outage that was never asked a question. A clean sweep that exercised nothing is not a pass.

**One command replays a seed.** The `sim` binary in `crates/world` takes a seed: `cargo run -q --locked -p hyalos-world --bin sim -- <seed>`. It prints the schedule, the outcome, the counts and every event, keeps the log at a path that names the seed, and exits non-zero if the run fails. The same binary sweeps with `--sweep <from> <to> [threads]`. For each failing seed it prints that seed's command, so a CI log hands the developer the command to run.

**CI sweeps seeds 0 to 9,999 on every pull request**, in a `simulation` job of its own in `ci.yml`, built in release mode. A run takes 30 to 80 ms in release, depending on how many share the disk, against about 250 ms in debug. On a ten-core machine the whole sweep takes about 75 seconds. The required `build, lint, test` job already takes 14 to 20 of its 20 minutes, so the sweep cannot go there. The seeds are fixed, not drawn per run, so a red sweep is the change's fault and not luck, and the same range runs locally. The `cargo test` sweep runs 100 seeds in debug, the fewest with room to spare that still reach every profile and outcome, or `HYALOS_SIM_SEEDS` of them; the schedule tests together take under two minutes on a CI runner.

**The job is a required check.** For a failure to fail the pull request, the `simulation` check is added to the `main` ruleset's required status checks, with `build, lint, test`, `dependency audit` and `policy proofs`.

This supersedes ADR-0036's rule that failure rates are fixed in the code. Those rates remain as the usual schedule.

## Consequences

The engine now runs in ten thousand worlds on every change, not one mix of faults 150 times. A failing seed is one command to replay. The CI log names the seed, its profile and why it failed.

A change to how a schedule is drawn, or to a fake's draws, changes which seeds hit which path. Fixed seeds keep a sweep reproducible but do not explore new ones. Widening the range is the way to search further, and it costs CI time.

What a sweep shows is still bounded by ADR-0036. The fakes stand in for the forge, the model, the gate and the substrate, so faults in the real broker and real sandboxes are for the chaos scenarios (S8.5) and the contract suites (S8.3). Faults inside the engine's own process, such as a disk that fails mid-write, are not scheduled here.
