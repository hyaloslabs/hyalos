# ADR-0129: Plan packs live in `docs/plans/`, and the tracker still holds status

**Status:** Accepted

## Context

ADR-0014 gives each kind of information one home. Planning working material lives in `docs/planning/`, is never authoritative, and is deleted or folded into an ADR once it is settled. GitHub issues are the only source of work status.

Work on the task-kernel spike is now planned as plan packs. A pack is a folder that specifies one piece of work before it is built: what it is for, what exists today, the decisions taken, the design, and the slices it is split into, one issue and one pull request each. The people and agents who build a slice read the pack from `main`, so it has to be in the repository for as long as its slices are open. That is longer than ADR-0014 lets planning material live, and a pack is not loose notes.

A pack's README also has a Status row, and a pack gains a `progress/` file per slice while it is built. Left unexplained, both read as status kept in the repository, which ADR-0014 forbids.

## Decision

Plan packs live in `docs/plans/<slug>/`. This adds a home to ADR-0014's list and changes nothing else in it.

A pack is working material for its slices' issues. It is kept while any of its slices is open and may be deleted once they are all closed.

The tracker stays the only source of work status. A pack's Status row says how far the planning of that pack has got, and nothing about what is built. A `progress/` file is the builder's notes for one slice. When either disagrees with an issue, the issue is right.

The decisions inside a pack, under its `decisions/`, bind that pack's work only. A decision that changes the trusted core, or anything an ADR in `docs/adr/` owns, is still made as an ADR there.

## Consequences

- A slice's issue can point at a contract in the repository, and a reviewer can read what was meant to be built beside what was.
- There are two places a decision may be written. The rule above says which, and a pack that touches `core/` or `crates/` must bring its ADRs to `docs/adr/`.
- Packs for finished work will pile up unless someone deletes them. Nothing enforces that.
- So far packs plan only spike work under `spikes/`, which is not trusted core. The first pack that plans trusted-core work should test this decision.
