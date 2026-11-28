# ADR-041: The measurement's transport key, its stops, and where step D is recorded

**Status:** accepted (developer, 2026-11-27); amended by ADR-042, step D's full scope and abandoned runs, the Trust line's clock, identity and binaries, the transport key's source, and fresh re-runs

**Proposed by:** author (cold-read panel, run 6); accepted by the developer before round 16

## Context

The run 6 panel's Trust reviewer found four trust behaviours of the #411 measurement (Slice 10) that the pack left to the developer at the moment they arise. The notary's signing key now comes from `notary public --dir` on the notary host (ADR-025), but its transport public key still comes from the notary's standard output at start (`hyalos-os/attest/probes/two_machines.sh:41`, `:52`), and no decision records that. Three stops (sessions naming different models; `attest check` refusing a real presentation `root`, `notary` or `template`; a host-key fingerprint mismatch at setup step A) gave no next step. Setup step D, deleting the copies ADR-040 allows, follows Slice 10's merge, and nothing said where it is recorded.

## Options

1. **Transport key:** keep it from the notary's standard output, trusted through the pinned SSH host key (two-machine path) or the local `coproc` (one-process path), and record that (chosen); or print it from the state directory with `notary public --dir` and refuse a standard-output mismatch (rejected: more code in Slice 10 for a key the pinned channel already carries).
2. **Model-name difference and a refused real presentation:** a failed run, then a re-run (chosen); or a trust incident with a notary key rotation first (rejected: the cause is recorded either way, and a re-run on a new issue spends a fresh authorisation).
3. **Host-key mismatch:** an incident, with no credential copied anywhere until it is resolved (chosen); or pick another host at once (rejected: it moves on before the mismatch is understood).
4. **Step D's record:** Slice 11's progress file, which Slice 11 may not be `done` without (chosen); or off-repository evidence only (rejected: nothing in the repository would show the copies were removed).

## Decision

- The notary's transport public key comes from the notary's standard output at start, as today. On the two-machine path it is trusted through the SSH host key pinned at setup step A; on the one-process path through the local `coproc`. ADR 0140's Trust line says so.
- Sessions that report different model names, within a path or between the paths, make that measurement a failed run. So does `attest check` refusing a real presentation `root`, `notary` or `template`. The run goes in ADR 0140's failed-runs table with its cause, and the developer runs `notary authorize --again` and re-runs that path on a new issue. After a `root`, `notary` or `template` refusal, the re-run waits until the cause is written in that row.
- A host-key fingerprint that differs at setup step A is an incident. No credential is copied to any host (step B) until it is resolved. The developer then re-keys or replaces `SSH_HOST` and repeats step A.
- The developer records step D in Slice 11's progress file, naming paths only, and Slice 11 may not be set `done` until that line exists.

## Consequences

The measurement's stops each have a next step, and the deletion of the copied credential is visible in the repository before the pin merges. A Slice 11 executor cannot finish while step D is unrecorded, so Slice 11 stays `blocked` on it if Slice 10's copies are still there.
