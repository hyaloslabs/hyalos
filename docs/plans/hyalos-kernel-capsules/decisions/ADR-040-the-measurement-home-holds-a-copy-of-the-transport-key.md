# ADR-040: The measurement's HOME also holds a copy of the operator transport key

**Status:** accepted (developer, 2026-11-27); amended by ADR-042, step D's full scope and abandoned runs, the Trust line's clock, identity and binaries, the transport key's source, and fresh re-runs

**Proposed by:** author (cold-read); accepted by the developer in cold-read round 13

## Context

This amends ADR-039, whose Decision says the measurement's fresh `HOME` holds "only what the run reads from it, copied in by the developer: the provider credential, and nothing of the usual operator key". The two-machine probe also reads the operator's transport key from `$HOME/.config/hyalos-spike/operator-transport.key` (`hyalos-os/attest/probes/two_machines.sh:15`), so a `HOME` without it cannot run that path. ADR-039 was written about the ledger key, which forks the committed chain if reused; the transport key signs nothing in the public log.

## Options

1. **The fresh `HOME` also holds a copy of the operator transport key; only the ledger key must be fresh.** Chosen.
2. **A fresh transport key too**, made in the measurement's `HOME`. Rejected: it changes the probe's setup for a key whose reuse forks nothing.
3. **The probe takes the transport key's path by an argument**, so `HOME` holds only the credential. Rejected: it changes the probe's interface for the same gain.

## Decision

- The measurement's `HOME` holds copies of the provider credential and of `~/.config/hyalos-spike/operator-transport.key`, each mode 0600, and no ledger key; `live` makes the ledger key fresh there, as ADR-039 says.
- After ADR 0140 is accepted, the developer deletes both copies from each measurement `HOME`, and the credential copy on the notary host, and records that it did, naming paths only.
- The rest of ADR-039 holds.

## Consequences

The two-machine path runs from the measurement's `HOME` unchanged. The usual transport key exists in one more place for the length of the measurement, and the deletion step bounds that.
