# ADR-010: A version 2 intent names the operator's key, and run counts say they are per key

**Status:** accepted (developer, 2026-11-22)

**Proposed by:** author (spec), from the pack's adversarial read (F6); accepted by the developer at the spec ADR checkpoint

## Context

An intent link holds its version, kind, task, guest, sequence and previous hash, and no key. The first intent of a chain is therefore byte-identical under every key, so its run hash is too, and one ticket's attestations could back a chain under a second key that shows none of the first chain's runs. The report's `the task was run 1 time` also reads as a total when it is only one key's chain.

## Options

1. **Drop the two run-count lines.** Rejected: within one key they still catch a discarded run relabelled as another task.
2. **Have the notary record the operator's log key.** Rejected: the notary would have to trust what the operator tells it, and the log key is not part of its link.
3. **Put the operator key's fingerprint in the version 2 intent link, and qualify both lines.** Chosen.

## Decision

A version 2 intent link carries `operator`, the SHA-256 of the operator's public key. The audit refuses a version 2 intent whose value is not the key it was given. Both run-count lines say `under this log key only`.

## Consequences

- A run hash belongs to one log key, so attestations cannot be reused under another.
- The ceiling, which the notary signs, is the number to rely on across keys.
