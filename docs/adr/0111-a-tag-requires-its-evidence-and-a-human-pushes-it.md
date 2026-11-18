# ADR-0111: A tag requires its evidence, and a human pushes it

**Status:** Proposed

## Context

S8.6 (#55). [ADR-0013](0013-deterministic-simulation-and-evidence-gated-releases.md) decides: "A release cannot be tagged without green simulation, contract, red-team and chaos results for its commit, but green results do not tag it." Publishing is irreversible, so a human decides it ([ADR-0012](0012-humans-approve-only-irreversible-actions.md)).

The evidence now exists. All of it runs in the `ci` workflow, on every pull request and on every push to `main`:
- the simulation and its fault sweep ([ADR-0104](0104-fault-schedules-swept-in-ci.md)), in `simulation`;
- the job spec's model check ([ADR-0108](0108-the-job-state-machine-s-tla-spec-and-its-model-check.md)), in `job spec`;
- the contract suites ([ADR-0109](0109-property-based-contract-suites-at-every-boundary.md)) and the red-team corpus ([ADR-0110](0110-the-red-team-reach-is-recorded-per-release-and-may-not-regress.md)), in `test (the rest of the workspace)`;
- the chaos scenarios ([ADR-0106](0106-a-dependency-lost-mid-job-holds-the-task.md)), in `test (the hyalos crate)`.

Nothing tied a release to them. `release.yml` ([ADR-0081](0081-prebuilt-releases.md)) released any tag that was the workspace's version and on `main`, whatever `ci` said about its commit. A commit merged with an admin override, or tagged before `ci` finished, would ship unchecked.

## Decision

**A release needs its commit's evidence, read from GitHub, before anything else runs.** `release.yml` gets a first job, `release evidence`. It checks what the `image` job used to check: the tag is `v` and the workspace's version, and it is on `main`. Then it runs `.github/scripts/release-evidence.sh` on the tagged commit. The script passes only if a `ci` run for that commit, on a push to `main`, completed with success, and in its latest attempt each evidence job succeeded:
- `simulation`;
- `job spec`;
- `test (the hyalos crate)`;
- `test (the rest of the workspace)`;
- `build, lint, test`.

A run still going, a job skipped, cancelled or failed, no run at all, or an answer it cannot read is no evidence, and the release stops. The job reads with `actions: read` and writes nothing. Every other release job (`reach`, `image` and through it `binary`) needs it, so no image is pushed and nothing is published without it.

**Green evidence is necessary, never sufficient.** No workflow creates a tag or a release whose tag does not exist already. `gh release create` runs only with `--verify-tag`, so it publishes a tag a human pushed and never makes one. The tag comes from the operator. `.github/scripts/tag-release.sh`, run in their own terminal, reads `main`'s head and the workspace's version. It runs the same evidence check, and only then creates the annotated tag, locally. It prints the push and does not run it: pushing is the decision. The script is a convenience, not the gate. A tag pushed any other way meets the same check in the workflow.

**The gate is checked on every pull request.** `crates/xtask/tests/release_gate.rs` reads the workflows and scripts. It fails if:
- an evidence job name is not a `ci` job's name, matrix parts included, so a renamed job cannot silently empty the evidence;
- a job of a release does not wait for `evidence`, directly or through another job;
- anything in `.github/workflows` or `.github/scripts` other than the operator's script creates a tag, pushes one, calls the API that makes refs or tags, or creates a release without `--verify-tag`.

## Consequences

A tag without evidence releases nothing. The run fails in its first job, names what is missing, and has pushed no image. A tag pushed before `ci` finished is released by re-running the workflow, not by cutting the version again.

The workflow cannot stop the tag itself from being pushed. To refuse that too, the operator adds a tag ruleset on `v*` that only maintainers may create. That is a repository setting, documented in [releases.md](../operations/releases.md), not something this repository can check.

The evidence is `ci`'s run on `main` for the exact commit. With squash merges, that is the run of the merge itself. A commit that never ran `ci` on `main`, such as one pushed to a tag only, can never be released.

Adding a kind of evidence means adding its job to `ci.yml` and its name to the script's list. The xtask test then holds the name to the job.
