# Hyalos

Clean-room Rust rewrite of hivrr. Currently in the planning phase: docs only, no code.

## Documentation rules (see docs/README.md)

- Intent and scope: `docs/prd/`. Never put status there.
- Decisions: `docs/adr/NNNN-title.md`, one per file, from `0000-template.md`. Add new rows to `docs/adr/README.md`.
- Never edit Context, Decision or Consequences of an **Accepted** ADR. Write a new ADR that supersedes it and change only the old one's Status line. **Proposed** ADRs may be edited.
- Status lives in GitHub issues (epics as parent issues, milestones M1 to M4), not in this repo. `docs/planning/` is working material only.

## Planned stack (ADR-0001)

Single Cargo workspace producing one `hyalos` binary (engine + broker). Trusted-core paths require human code-owner review.
