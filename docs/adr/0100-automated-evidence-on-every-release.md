# ADR-0100: Automated evidence on every release

**Status:** Proposed

## Context

[ADR-0007](0007-capability-based-marketplace-trust.md) replaces the human signing each release with automated evidence the pipeline gathers on every release. That evidence is static analysis, review agents, red-team items and the capability diff, and a failure in any of them blocks publication. Trust then rests on the runtime enforcing capabilities ([ADR-0006](0006-plugins-are-skills-and-wasm-components.md)). The evidence catches what a release should never ask for, or should never say, before it is signed. [ADR-0094](0094-the-marketplace-release-pipeline.md) built the pipeline in `hyaloslabs/marketplace`. Its `sign` job ran once the two builds agreed and the settings check had passed, and nothing looked inside a skill. S7.4 (#45) asks that all four checks report, and that a failure blocks publication.

A review agent calls a model, so the pipeline would hold a model provider's key. Whether to hold one, which provider and model, and what to spend are the operator's decisions. They are split into #313.

## Decision

**`tools/evidence.py`** (in `hyaloslabs/marketplace`) checks every built archive. It reads the archive format itself (ADR-0090), so it checks the bytes that will be signed. It reports per skill, and any finding fails it.

- **Static analysis.**
  - `capabilities.json` is a version-1 capabilities document. Every name in it is in Hyalos's vocabulary (ADR-0085). No action is one the shipped policy never permits a job (`change_policy`, `publish`, `spend_over_cap`, `export_credentials`).
  - No file is binary.
  - Every URL in any file names, by its real host (case-insensitive, userinfo stripped, ASCII only), a host the skill asks for.
  - No text file runs what it decodes or downloads: a pipe into a shell or interpreter, process substitution, `$(curl …)`, `eval` or `base64 -d`. No text file reads a credential or the environment.
  - In a script, every line that reaches the network names a declared host.
  - `shellcheck --severity=warning` passes on every shell script. The pipeline requires shellcheck, so a missing one fails.
- **Red-team items.** `redteam/items.json` holds patterns for prompt injection, a new role, the system prompt, capabilities asked for in prose, concealment from the operator, exfiltration, invisible characters (including Unicode tag characters) and HTML comments. A file that matches any item fails. Each item has a case in `redteam/cases/` that the tests require it to catch, and the tests require the shipped skills to match none.
- **The capability diff.** Each skill is compared with the last release of it: the latest release's archives in `release.yml`, or the base branch's build on a pull request. When there is no earlier release, everything the skill asks for counts as added. Every name a release adds must be listed for that name and version in `capability-changes.json`, which changes only in a reviewed pull request to `main`. A widening is therefore always a visible decision, and an acknowledgement never carries over to another version. A narrowing needs nothing.

The report holds only rule ids, red-team item ids, paths, versions and names in the vocabulary. Nothing a skill wrote, beyond what the vocabulary allows, reaches it. It goes to the run's summary and to an `evidence` artifact kept for 90 days. A missing or empty set of archives is refused, never passed.

**Blocking.** `release.yml` has an `evidence` job that needs `build`, and `sign` needs `evidence`, so nothing is signed and nothing is published unless the evidence passed. The `alert` job reports a failure there like any other. `ci.yml` runs the same check on every pull request. The job has only the workflow's `contents: read` and checks out without credentials.

**Review agents** are deferred to #313. They will run only after the operator decides on a key, and their output is tainted text that informs but never decides alone (ADR-0047). A failure or timeout there will block publication too.

Hyalos itself does not change. It still trusts a release because the pipeline signed it, and the evidence now gates that signature.

## Consequences

- A release that asks for a refused action, carries an injection the items know, or widens its capabilities without a reviewed acknowledgement is never signed, so no Hyalos host can install it.
- The rules are patterns and catch what they describe, not every attack. A skill can still behave badly at run time, within what it was granted. The runtime's enforcement (ADR-0006) bounds that, and revocation (ADR-0098) removes a release found later.
- A new skill that asks for any capability needs an entry in `capability-changes.json` in its first pull request.
- Adding a red-team item needs a case, and a false positive on a shipped skill fails the tests before it can block a release.
