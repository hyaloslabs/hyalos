# ADR-003: The contract, SDK and script travel as the first input records

**Status:** accepted (developer, 2026-11-25)

**Proposed by:** author (spec); accepted by the developer at the spec ADR checkpoint

## Context

The monitor must enforce a contract on a live run and again on replay, from the record alone
(S-01.5). A Python task's script and SDK must be part of the task identity a customer authorises
(S-03). The record is version 4: it names a profile and frames the guest hash and every input with
its length (`hyalos-os/audit/src/record.rs:205`, `:256-276`). The input loader splits a file on
newlines (`hyalos-os/monitor/src/main.rs:182`), and the guest interface has no call to fetch a
script. Refusals are a closed set named in `hyalos-os/ABI.md`.

## Options

1. **A new record version with `contract`, `sdk` and `script` fields.** Rejected: a record format
   change, its own pinned bytes, verifier and upcast work, for a draft contract capsules will
   replace.
2. **Bake the script and SDK into the guest's bytes (frozen modules).** The guest hash binds them.
   Rejected: every script is a new interpreter build, the contract still needs a home, and capsules'
   split (runtime fixed and signed, only the data varying) cannot be tried.
3. **Input records 0, 1 and 2 are the contract, the SDK and the script, read whole by a spike run
   command; data records follow.** Version 4 framing binds all of them. Under a spike profile,
   `execute_with` parses record 0 as the contract before the guest loads, and `Host` enforces it.
   Chosen.
4. **For an ungranted call, a new reason such as `NotGranted`.** Rejected: a change to the guest
   interface's refusal table for a draft. The existing `UnknownSchema` and `UnknownEffect` already
   mean "this run may not ask or do that".

## Decision

Under a spike profile, input record 0 is the contract, 1 the SDK and 2 the script, each read whole.
The contract is enforced from record 0 on every run and replay. An ungranted schema or effect ends
the run with `UnknownSchema` or `UnknownEffect`. A missing or unreadable contract ends it before the
guest loads with `BadContract`, a reason compiled only with `spike-contract` and appended last to the
runner's reason table.

## Consequences

- No record version, field or upcast changes; existing verifiers refuse a spike record only because
  they do not know its profile.
- The guest can read its own contract, SDK and script with `read`. That is harmless: none is secret.
- The draft contract's format is the spike's own (a small JSON object with exactly `asks` and
  `may_touch`, unknown fields refused), and capsules owns the real one.
- Capsules inherits the question of a looser authorisation for new data (parent brief): this layout
  makes the fixed part (records 0 to 2) and the varying part (the data) easy to see.
