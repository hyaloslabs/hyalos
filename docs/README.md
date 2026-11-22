# Hyalos documentation

Each kind of information has exactly one home ([ADR-0014](adr/0014-tracker-holds-status.md)).

| What | Where | Rule |
| --- | --- | --- |
| Intent and scope (what and why) | [`prd/`](prd/) | Never holds status |
| Decisions (why it is shaped this way) | [`adr/`](adr/) | One decision per file; immutable once accepted, only superseded |
| Status (what is done, in progress) | GitHub issues (epics as parent issues, milestones M1 to M4) | The only source of status |
| Planning-session working material | [`planning/`](planning/) | Temporary; not authoritative once filed or settled |
| Plan packs: a piece of work specified and split into slices before it is built | [`plans/`](plans/) | Working material for the slices' issues. A pack's Status row tracks the planning flow only; what is done or in progress is the issues' to say ([ADR-0129](adr/0129-plan-packs-live-in-docs-plans.md)) |
| Throwaway rigs behind a spike's evidence | [`../spikes/`](../spikes/) | Not product code; delete when the real component makes it redundant |
| Operator procedures (setup, backup, recovery) | [`operations/`](operations/) | Steps an operator follows; the reasons live in the ADRs they link |
| Reference docs (CLI, manifest schema, WIT) | `reference/` (later) | Generated from types, manifests and ADRs, not hand-written |
| Threat model, data flows, a guide to reviewing the trusted core | [`security/`](security/), and [`SECURITY.md`](../SECURITY.md) at the root | Each control names its code and its test |

## Where to put new things

- **A new product requirement or scope change:** edit the PRD (or add a new PRD under `prd/`).
- **A technical decision with trade-offs:** add an ADR. Don't bury it in a PRD or a code comment.
- **A task, bug or progress note:** a GitHub issue. Not a file in this repo.
