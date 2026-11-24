# ADR-007: A line budget for hyalOS's own code

**Status:** accepted (developer, 2026-11-23)

**Proposed by:** author (spec); accepted by the developer at the spec ADR checkpoint

## Context

Parent ADR-001 says this pack must set the new workspace's line budget and its rules for `unsafe`. The root workspace's 30,000-line budget counts only `core/` (`crates/xtask/src/main.rs:17`). On 2026-11-23, hyalOS's own non-test, non-comment code was about 4,700 lines: monitor 1,847, kernel image 239, guest 171 and attestation 2,416. The verifier pack adds at most 3,000.

## Options

1. **No budget.** Rejected: the review's cost grows unnoticed.
2. **Count hyalOS inside the existing 30,000.** Rejected: parent ADR-001 keeps it a separate workspace outside the trusted core's count.
3. **A budget per crate.** Rejected for now: the crates are still moving, and it would add churn.
4. **One budget of 12,000 lines of hyalOS's own non-test code, with an allow-list of files that may use `unsafe`.** Chosen.

## Decision

`cargo xtask lines` also counts hyalOS's own non-test code under `hyalos-os/` and fails above 12,000 lines, using the same counting rules as `core/`. `unsafe` is forbidden except in files on a named list, each block carrying a `SAFETY` comment. Vendored or third-party code is not counted, and is covered by the review scope instead.

## Consequences

- There is room for this pack and the verifier, at roughly 8,000 to 9,000 lines, with a margin for capsules.
- Raising the budget takes a new ADR.
- The `unsafe` list starts with the monitor's KVM code, the kernel image, the guest and the notary's system calls.
