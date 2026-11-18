# ADR-0107: A release runs a complete job before it publishes

**Status:** Accepted. Supersedes [ADR-0081](0081-prebuilt-releases.md)'s rule that the workflow checks only that the binary is static, runs and names its image, and its pull-request check jobs.

## Context

#290 (S8.8). v0.1.0 was published with a binary that refused every job (#280). On musl's small thread stack, Cedar could not answer, so every job's manifest was refused. The release workflow had checked what [ADR-0081](0081-prebuilt-releases.md) asks: the binary is static, it prints its version, and `hyalos setup` names its image. None of that runs a job. Only bot1's first run caught it.

The fix added a regression test for that one cause, `main_thread.rs` on musl. The next cause will be different: a missing sandbox tool, an image the binary cannot start, or a gate that cannot answer in a static build. A release should not publish a binary until that binary has run a whole job.

## Decision

**Before `Publish the release`, the workflow runs one complete job** with the binary it built and the image it pushed. It uses `.github/scripts/run-release-job.sh`, which runs `core/hyalos/tests/release_job.rs` against them. The job is the fix-issue workflow on issue 7 of the stand-in's `octo/repo`:
- the task starts;
- a reader reads the issue as tainted and returns typed values, and a planner chooses from them;
- Codex runs as the actor, in the sandbox image the binary names by default (it is given no `--image`);
- Codex pushes the job's branch, opens a pull request and submits its result.

The step fails unless all of these hold:
- the workflow started and both steps answered;
- the running container's image is the one the release names by digest;
- the branch is at the stand-in and a pull request was opened from it;
- no token is left live;
- the three jobs succeeded and the task completed;
- the binary's own `hyalos log verify` passes on the log it wrote.

It does not trust the binary's exit status alone. A failed step publishes nothing, since `Publish the release` comes after it in the same job.

**It needs no secret.** The model is scripted on loopback, and GitHub is the stand-in the integration tests use (`core/hyalos/tests/common`). So the same step runs on pull requests.

**A pull request that changes the release** runs one check job, `release (check)`, in place of the two. It builds the image as `hyalos-sandbox:latest`, which a check build of the binary names, builds and checks the binary as a release does, and runs the same complete job. It can still write nothing. The pull-request paths now include the test, its common module, the script and the image.

**The check is shown to catch a refusing binary.** `a_binary_that_refuses_every_job_fails_the_check`, in the same file, runs the check on a stand-in for v0.1.0. The stand-in passes every command through to the real binary except `run`, which prints a refusal and exits 0. The check must fail. That test needs no image, so it runs in CI's `test` part on every pull request. Without `HYALOS_RELEASE_JOB_BINARY` the complete job itself is skipped, and only the release workflow sets it.

## Consequences

A release takes longer: the test compiles on the runner, and the job starts the image. The binary job's limit goes from 30 to 50 minutes, and the pull-request check's to 60.

A binary that cannot run a job is not published, whatever the cause. A binary that runs this job but fails another is still possible. The job covers the default path a new operator takes: the fix-issue workflow, Codex and the container substrate. It does not cover Claude Code, gVisor or a microVM.

The stand-in GitHub is not GitHub. A change in GitHub's API is caught by the bot runs, not by this step.
