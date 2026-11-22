# ADR-006: An unknown provider proof is reported as unchecked

**Status:** accepted (developer, 2026-11-22)

**Proposed by:** author (spec); accepted by the developer at the spec ADR checkpoint

## Context

#410 asks for a typed slot for a provider-side proof, with no real proof to check yet. On 2026-11-02 Anthropic released FlashProve, a research prototype that is no API feature. The audit must decide what to do with a well-formed entry of a kind it does not know. The rule is that an unknown value never counts as an allow.

## Options

1. **Refuse the whole record.** Rejected: nothing the audit claims depends on the entry, so refusing would let any later tool's addition break an otherwise sound record, and teaches readers nothing.
2. **Ignore the entry silently.** Rejected: a reader of the record could believe the proof was checked.
3. **Pass or fail on the other checks, and report the entry as not checked and proving nothing.** Chosen.

## Decision

A well-formed entry of an unknown kind has no effect on the verdict. The report names it as not checked. A malformed entry is refused when the record is read. The limit that the model's name is the provider's claim stays in every report until a later version defines a kind it can verify.

## Consequences

- The format can gain a real proof kind without changing how older entries are read.
- No code path can turn an unknown kind into a claim, and a test holds that.
- Defining the first real kind is a new record version and a new decision.
