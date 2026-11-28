# ADR-017: An optional per-schema prompt in the contract

**Status:** accepted (developer, 2026-11-25); amended by ADR-021, the apostrophe is allowed; amended by ADR-027, the default instruction's exact wording

**Proposed by:** author (spec); accepted by the developer after the adversarial read

## Context

This amends ADR-014's choice of one fixed system line. The adversarial read showed that the author's question and hostile data then travel in the same user text, so the fixed line separates nothing.

## Options

1. **One fixed system line.** Rejected: as above.
2. **An optional, bounded `prompt` per schema, part of the schema's canonical type, sent as the request's instruction text and held equal by the audit.** Chosen.

## Decision

Each `asks` schema may carry a `prompt` of 1 to 512 bytes from a closed character set with no quote, backslash or newline. It is part of the canonical type bytes, so it goes from the contract to the request to the audit's check. Without one, a fixed default instruction is used. The user text remains the guest's question bytes.

## Consequences

The prompt is text the customer reads in the contract and authorises with the task. The report still states that no instruction text defends against injection on its own.
