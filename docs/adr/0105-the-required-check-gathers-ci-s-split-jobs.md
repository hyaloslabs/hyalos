# ADR-0105: The required check gathers CI's split jobs

**Status:** Accepted. Supersedes [ADR-0104](0104-fault-schedules-swept-in-ci.md)'s rule that `simulation` is added to the ruleset's required checks by name. Its list of the jobs `build, lint, test` gathers is superseded by [ADR-0108](0108-the-job-state-machine-s-tla-spec-and-its-model-check.md), which adds `spec`.

## Context

#320. `build, lint, test` ran formatting, clippy, the layout checks and `cargo test --workspace` in one job, with a 20-minute limit. On `main` it took 14 to 20 minutes, and once 20m11s. In #319 (S8.1) two more minutes of tests got it cancelled at the limit. S8.3, S8.4 and S8.5 each add suites to this pipeline. The last full run spent:
- 3 minutes on clippy;
- 4.5 minutes compiling the tests;
- about 12 minutes running test binaries, one after another, of which 341 seconds were the `hyalos` crate's.

The `main` ruleset requires three checks by name: `build, lint, test`, `dependency audit` and `policy proofs`. A job added beside them is not required unless someone edits the ruleset. So splitting the job naively would make the moved tests optional. And [ADR-0104](0104-fault-schedules-swept-in-ci.md)'s `simulation` job is not required yet, because it is waiting on that same edit.

## Decision

**The work is split into jobs that run in parallel:**
- **`lint`:** formatting, clippy and `cargo xtask layout` and `lines`.
- **`test`:** a matrix of two parts. One is `cargo test -p hyalos`. The other is `cargo test --workspace --exclude hyalos`, so a new package lands in the second part without an edit to the workflow. At the split, the two parts list 266 and 679 tests, and `--workspace` lists 945, so together they are the whole workspace. Each part installs Docker's image, gVisor and Firecracker, and sets the `HYALOS_REQUIRE_*` variables, as the single job did.

**`build, lint, test` gathers them.** It keeps its name, so the ruleset needs no edit. It needs `changes`, `lint`, `test` and `simulation`, runs unless the workflow was cancelled, and passes only in two cases:
- every gathered job succeeded;
- only documentation changed, and every gathered job was skipped.

Anything else fails it: a failure, a cancelled job, a timeout, or a job that never ran. So a split never makes a test optional, and adding a part to the matrix keeps it required.

**`simulation` is required through it.** This supersedes ADR-0104's rule that `simulation` is added to the ruleset's required checks by name. Gathering it needs no change to the ruleset, and it cannot be forgotten when the ruleset changes.

**The budget is written in `ci.yml`.** Every job has a 20-minute limit and should finish in 15. When a job nears 15 minutes, the fix is to move packages between the parts of `test`, or to add a part. Raising the limit is not the fix.

## Consequences

The required job's wall time is now that of its slowest part, about 10 to 12 minutes, against 18 to 20 before. Each part compiles its own tests, so the runner minutes go up by about one compile.

Clippy no longer delays the tests, and a lint failure shows up before the tests finish.

The ruleset still names three checks. Everything gathered behind `build, lint, test` is required with it.
