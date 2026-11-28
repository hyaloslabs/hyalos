# ADR-039: The #411 measurement uses a fresh ledger key, outside the repository

**Status:** accepted (developer, 2026-11-27); amended by ADR-040, the measurement's HOME also holds a copy of the operator transport key; amended by ADR-042, step D's full scope and abandoned runs, the Trust line's clock, identity and binaries, the transport key's source, and fresh re-runs

**Proposed by:** author (cold-read); accepted by the developer in cold-read round 13

## Context

Slice 10's measurement runs `live`, which opens its chain as `ledger` in its working directory and its operator ledger key under `$HOME` (`hyalos-os/monitor/src/main.rs:790-791`), the key made on first use if absent. The repository commits a chain under an operator key (`hyalos-os/evidence/ledger/`), and `s_06_1_e1` audits it against the public log (`hyalos-os/verify/tests/real.rs:104-118`). That audit refuses whenever the log lists a different number of entries under the key than the chain holds (`hyalos-os/audit/src/chain.rs:543-550`). Run from `hyalos-os/` with the developer's usual key, the measurement would start a second chain at `00000` under the same key, forking the public, permanent chain and failing `s_06_1_e1` whenever the log is reachable. `hyalos-os/ledger` is not git-ignored.

## Options

1. **A fresh ledger key and chain, outside the repository.** Chosen.
2. **Extend the committed chain**, running `live` on a copy of `hyalos-os/evidence/ledger` with the usual key and committing the longer chain. Rejected: the measurement's entries become part of the committed evidence, and a failed run between log append and commit leaves the log and the chain unequal.
3. **Accept the fork**, re-pinning or retiring `s_06_1_e1` and git-ignoring `hyalos-os/ledger`. Rejected: it gives up a committed public-log proof for a measurement.

## Decision

- Every `live` run and probe of the measurement, on both paths, runs with `HOME` set to a new directory outside the repository, made for the measurement, and with a working directory outside the repository, so `live` makes a new ledger key there on first use and opens a new chain there.
- The new `HOME` holds only what the run reads from it, copied in by the developer: the provider credential, and nothing of the usual operator key. Other tools keep their own configuration (for example `gh`, through its own config directory).
- ADR 0140's Trust line names this key by the operator key hash of the run's intent link, never by reading the key.
- The committed chain under `hyalos-os/evidence/ledger`, its key and `s_06_1_e1` are untouched, and nothing from the measurement is committed but ADR 0140.

## Consequences

The measurement's log entries sit under a key of their own, so the committed chain still equals the public log under its key. The developer keeps the measurement's `HOME` and chain with the evidence they hold off the repository, or discards them; the repository does not depend on either.
