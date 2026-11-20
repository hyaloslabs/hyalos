# ADR-0122: A pull request says it closes its issue once

**Status:** Accepted. Supersedes [ADR-0083](0083-a-fix-issue-pull-request-closes-its-issue.md)'s rule that the broker's closing line is added beside a harness line that already closes the issue.

## Context

[ADR-0083](0083-a-fix-issue-pull-request-closes-its-issue.md) has the broker put `Closes #N` first in a fix-issue pull request's body, "even when the harness's body already says it closes `#N`". The reason was to avoid a parser: deciding whether text closes an issue as GitHub would is easy to get wrong, and a wrong yes would leave the issue open.

Both harnesses write the line themselves. On bot1 on 2026-11-19, Codex's #34 and Claude Code's #35, in a private test repository (its name redacted before this repository went public), each said `Closes #33` twice (#349). GitHub does not mind, but the operator reviews every such pull request, and the repetition reads as a mistake.

## Decision

**The broker's line stays first and is always there.** That part of ADR-0083 is unchanged.

**The harness's lines that only close the same issue are dropped.** A line of the harness's body is dropped when, trimmed, it is a GitHub closing keyword (`close`, `closes`, `closed`, `fix`, `fixes`, `fixed`, `resolve`, `resolves`, `resolved`, any case, with an optional colon after it), then `#N` for the manifest's issue, with an optional full stop. Blank lines left at the start of the body are dropped too, so the broker's line is followed by one blank line. Every other line is kept byte for byte: other issues, other repositories' references, and keywords inside a sentence or code span.

This needs no judgement about what GitHub would parse. A dropped line repeats what the broker's line already says, so dropping it can never leave the issue open. A line that is not exactly a duplicate is kept, and may still say it twice.

## Consequences

- The pull requests the operator reviews say `Closes #N` once in the usual case, where the harness writes it on a line of its own.
- The broker now edits the harness's text, but only by removing lines that repeat its own. The content gate still checks the harness's text before the edit, as ADR-0083 had it.
- A code block in the body whose line is only `Closes #N` loses that line. That costs nothing a reviewer needs.

Tests: `pulls.rs` (`closing`, and the fix-issue pull request in `core/broker/tests/pulls.rs`).
