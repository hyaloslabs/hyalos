# ADR-007: A directory that refuses every change is a stated limit

**Status:** accepted (developer, 2026-11-22)

**Proposed by:** author (cold-read); accepted by the developer in cold-read round 6

## Context

This amends ADR-003, which says a completed check that is not a pass leaves no pass. A check records a non-pass result by a rename and, if that fails, by removing the state file. When the notary's directory refuses both, for example on a read-only or failing disk, no program can remove the earlier pass.

## Options

1. **Accept it as a narrow, loud limit.** Chosen.
2. **Write an in-progress marker before every check and have the notary refuse sessions while it exists.** Rejected by the developer: every daily check would briefly block sessions and could end a run in flight, and a crashed check would block the notary until the next good check.

## Decision

When the directory refuses both the rename and the removal, the check exits 2 and prints that the state was not updated and the notary must be stopped. The earlier pass may be served until its 24 hours end. This is the only case where a completed non-pass check can leave a pass.

## Consequences

- The exposure is bounded by the 24-hour window and needs a disk failure at the moment of a non-pass verdict.
- `RECORD.md` states this limit.
