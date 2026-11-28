# ADR-015: Request templates are tied to the record version, and the verifier refuses free text

**Status:** accepted (developer, 2026-11-25)

**Proposed by:** author (spec); accepted by the developer after the adversarial read

## Context

This amends ADR-007. The adversarial read found that the notary signs any request to the approved server without reading it (`hyalos-os/attest/src/bin/notary.rs:906-912`); only the attest tool and `attest-check` check the template (`attest/src/main.rs:425`, `attest-check/src/lib.rs:264`). The design named one body builder but not which template is valid for which record version, and `RELEASE` moves only when `attest`'s package version is bumped by hand (`attest/src/lib.rs:15`).

## Options

1. **The notary checks the template.** Rejected: it would have to read the plaintext, against the notary pack's design that it sees none.
2. **One template for all versions.** Rejected: every existing real version 4 record would fail audit.
3. **Both templates accepted for any version.** Rejected: an old four-label request could be put in a version 5 record.
4. **The template is chosen by the record's version: versions 1 to 4 accept only today's template, version 5 only the closed-type one; the audit and `hyalos-verify` refuse a presentation whose request is any other; `RELEASE` embeds a digest of the templates, checked by a test.** Chosen.

## Decision

`attest-check` takes the record version and accepts exactly one template for it. A version 5 presentation whose request is not the closed-type template for the contract's type, prompt included, fails audit. The notary keeps signing blind. `RELEASE` is the TLSNotary tag, the package version and a digest of the request templates, so a template change always changes the release, and a notary and a run of different releases refuse at session open before anything is spent.

## Consequences

S-06.5 is a verifier-side criterion. A free-text request still spends one of the run's numbers at the notary, which ADR-007's count already bounds. No existing golden report may change in the template slice.
