# ADR-0083: A fix-issue pull request closes its issue

**Status:** Accepted. Its rule that the closing line is added beside a harness line that already closes the issue is superseded by [ADR-0122](0122-a-pull-request-says-it-closes-its-issue-once.md), which drops the harness's duplicate.

## Context

In the timed bot1 run (#253), the first issue to go all the way through Hyalos, #24 in a private test repository (its name redacted before this repository went public), was merged as pull request #28, and the issue stayed open. The pull request's body did not say `Closes #24`, so GitHub did not close the issue on the merge (#284).

The harness writes the pull request's title and body. The broker checks them with the content gate ([ADR-0048](0048-opening-a-pull-request-and-the-content-gate-on-its-text.md), [ADR-0065](0065-the-content-gate-on-what-a-tainted-job-publishes.md)) and sends them as they are. Asking the harness in its prompt to write `Closes #N` would not settle it: the actor is tainted, so whether its text says so is not something to rely on.

Hyalos already knows the issue. A fix-issue run is started for issue `N` of `owner/name` (`hyalos run --issue-number`, `hyalos start`, `start_job`). That number comes from the run's request, made by the operator or the driving agent, not from forge text. But the actor's manifest, which is all the broker enforces, did not carry it.

## Decision

**The manifest names the issue a job fixes.** `Manifest.closes: Option<u64>` is the number of an issue in the manifest's repository. It is left out of the signed bytes when absent, so a manifest without it signs as before and the manifest version stays 2 ([ADR-0031](0031-the-signed-manifest-and-its-admission.md)). The fix-issue runner sets it on the actor's manifest from the run's own `issue_number`, and on no other manifest. Minting refuses it on a merge's manifest (`mint.merge_not_alone`): a merge asks for the merge alone.

**The broker adds the closing line.** When a job whose manifest names issue `N` opens a pull request, the broker first checks the harness's title and body with the content gate, as before. It then puts `Closes #N` first, followed by a blank line and the harness's body byte for byte (or as the whole body, if the harness wrote none), and opens the pull request with that. The added line is the broker's own text, so the content gate does not check it.

**First, and always.** The line leads the body because Markdown lets earlier text swallow what follows it, not what comes before it: a code block or HTML comment the harness opens and never closes would hide a line at the end from GitHub. And the broker adds it even when the harness's body already says it closes `#N`. Deciding whether text closes an issue as GitHub would (keywords inside code or comments, word boundaries) is a parser the broker would have to get right, and a wrong yes leaves the issue open. A second closing line is harmless; a missing one is not.

**Within GitHub's limit.** A body that would be longer than GitHub's 65,536 characters with the line is refused, as an over-long body was before (422, `forge.bad_request`). A tainted job's text is under the content gate's 20,000 characters, so the line always fits for an actor.

This adds to ADR-0048's call: the broker sends the checked title and the checked body with its line. Nothing else in ADR-0048 changes.

## Consequences

A fix-issue run's pull request closes its issue when it is merged into the repository's default branch, whatever the harness wrote. A pull request into another branch does not fire GitHub's keywords, so the issue stays open there. Closing it on such a merge would be another forge write, and is not built.

Only an issue the run was started for is named by the broker. The harness's own text can still name other issues with GitHub's keywords, as before. The operator sees the whole body when they review the pull request before `hyalos merge`, and that review is where an unexpected closing line is caught.

A plain `hyalos run --issue <text>` names no issue, and its pull request is the harness's text alone.

Tests: the broker's line leads and the harness's text follows unchanged, even text that would hide a line after it, and a body with no room for the line is refused (`pulls.rs`). A fix-issue run's pull request starts with `Closes #N`, and a run with no issue adds nothing (`run_job.rs`). `closes` is signed when present and left out when absent (`manifest.rs`). A merge's manifest that names an issue is refused (`merges.rs`).
