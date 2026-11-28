# ADR-021: The prompt character set allows the apostrophe

**Status:** accepted (developer, 2026-11-25)

**Proposed by:** author (slice); accepted by the developer at the slice checkpoint

## Context

This amends ADR-017, whose "no quote" disagrees with the design's set, which holds the apostrophe so a prompt can read naturally.

## Options

1. **No apostrophe.** Rejected: it gains nothing, since an apostrophe needs no escape in JSON.
2. **Letters, digits, space and `,.?'()_-`; no double quote, backslash or newline.** Chosen.

## Decision

A `prompt` is 1 to 512 bytes of letters, digits, space and `,.?'()_-`. It holds no double quote, backslash or newline, so it never needs escaping.

## Consequences

The parser, the generator and the corpus test the apostrophe as allowed and the double quote as refused.
