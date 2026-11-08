# ADR-0082: The starter policy permits merge

**Status:** Proposed

## Context

[ADR-0076](0076-the-set-up-hyalos-skill-and-the-setup-checklist.md) chose a starter policy, `hyalos policy starter`, compiled from `core/hyalos/policies/starter.cedar`. It permits `read`, `push`, `open_pr` and `data_flow`, "what the fix-issue workflow needs and no more". [ADR-0077](0077-handing-the-operator-one-step-at-a-time.md) and [ADR-0080](0080-setup-asks-the-operator-in-their-own-terminal.md) superseded ADR-0076's handover, and kept its starter. `hyalos setup policy` shows the starter and approves it.

So a first issue stops at an open pull request. In the timed bot1 run (#253), the operator reviewed the pull request and ran `hyalos merge`, and it was refused: the policy in force permits no `merge`. Merging needs a second policy, edited and approved by hand. The starter's comment pointed at `docs/operations/running-a-job.md` for that, and that page did not say how.

Leaving `merge` out of the starter guards nothing that the baseline and [ADR-0071](0071-merging-after-the-operators-approval.md) do not already guard:
- `baseline.irreversible_needs_approval` forbids a merge without an approval, whatever the operator's policy permits.
- The approval is the operator's signed `merge.approved` event for one pull request's head commit, written only by `hyalos merge`. Minting refuses a manifest that does not repeat it exactly, and GitHub refuses a head that moved.
- `baseline.tainted_no_merge` forbids a merge by a tainted job. Every actor is tainted, so no job of a workflow can merge.
- The driving agent has no merge tool, and `hyalos merge` is one of `tools_cmd::DECISIONS`, which the agent never runs.

What leaving it out does cost is the first merge, and so the whole first issue.

## Decision

**The starter policy permits `merge`,** alongside `read`, `push`, `open_pr` and `data_flow`. Its other rules are unchanged: no web host, and a tainted job never sends data to the web. Its comment says what still guards a merge: the operator's signed approval of that pull request's head, one `hyalos merge` at a time, with no merge by a job that read forge content.

**A first issue ends in a merge.** When a first job has opened its pull request, the `set-up-hyalos` skill tells the operator to review it on GitHub and, if they want it, to merge it in their own terminal with `hyalos merge <owner/repo> <pr> --head <commit>`. The skill gives the pull request's number and the head commit's full id, which it reads with `gh`, but never runs the command. `docs/operations/running-a-job.md` gains a section on merging the pull request.

**Policies already in force are not changed.** An approved policy is the operator's signed decision, and nothing rewrites it. An operator who approved the earlier starter puts the new one in force with `hyalos setup policy`, which offers to replace the policy in force with the starter, or with `hyalos policy approve` on a file of their own.

This supersedes ADR-0076's starter, which ADR-0077 and ADR-0080 kept. Nothing else in those ADRs changes.

## Consequences

A first issue runs from setup to a merged pull request with no policy edit. The operator still makes two decisions on the way, both signed in their own terminal: releasing the issue's text to the actor, and merging after review.

An operator who wants no merges at all approves a policy without `merge`, as they would to remove any other action.

A merge still depends on the repository: branch protection, required checks and allowed merge methods are GitHub's, and the App merges as itself ([github-app.md](../operations/github-app.md), section 6).

The run under the starter (`run_job.rs`) now runs under a policy that permits `merge`. It still opens a pull request and merges nothing, since its actor never asks for `merge` and is tainted. A test merges under the starter with an approval, and the starter is checked to refuse a merge without one and a merge by a tainted job.
