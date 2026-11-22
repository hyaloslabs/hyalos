# ADR-007: A version 2 audit replays the guest

**Status:** accepted (developer, 2026-11-22)

**Proposed by:** author (spec), from the pack's adversarial read (F1); accepted by the developer at the spec ADR checkpoint

## Context

ADR-002 binds a ticket to one exact task, but the audit never runs the guest (`audit_record`, `spikes/task-kernel/kernel/src/main.rs:714`). `replay` is called only by the `attacks` and `replay` commands. A run's later sessions spend no ticket, and the request's question is free text, so an operator with a budget of 1 can open several sessions in one run, ask reworded questions, and write a record holding any answers and any result. Every answer would have the right run, sequence number and ticket, and the report would say attempt 1 of at most 1.

## Options

1. **Cap the sessions per ticket at a number the customer sets.** Rejected: the notary cannot see the questions, so it cannot tell whether the sessions are the ones the guest would ask, and a reworded question still fits under the cap.
2. **Show the notary the guest and have it check each question.** Rejected: the notary would have to run the guest and see the plaintext, which it is designed never to do.
3. **The auditor replays the guest as part of the audit.** Chosen.

## Decision

A version 2 audit requires `--guest <file>`, checks its hash against the record, and replays the record on the in-process backend before it reports. A replay that diverges refuses the audit.

## Consequences

- Extra answers, changed questions, a changed result and changed effects all fail the audit, so each position in a run has exactly one session that can count.
- An auditor needs the guest's bytes as well as the record.
- A guest's own branching on an answer stays legitimate, because replay follows it.
- The audit runs untrusted guest code under the interpreter's existing limits, and it still opens no kernel file.
