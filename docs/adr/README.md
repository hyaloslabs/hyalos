# Architecture Decision Records

One decision per file, named `NNNN-short-title.md`, numbered in the order proposed.

## Rules

- **Immutable once accepted.** Do not edit the Context, Decision or Consequences of an accepted ADR. To change a decision, write a new ADR and mark the old one `Superseded by ADR-NNNN`. Only the Status line of the old ADR changes.
- **Proposed ADRs may be edited freely** until the planning session accepts, changes or rejects them.
- **Statuses:** `Proposed`, `Accepted`, `Rejected`, `Superseded by ADR-NNNN`.
- Start from [`0000-template.md`](0000-template.md).
- Trusted-core paths require human code-owner review ([ADR-0014](0014-tracker-holds-status.md)).

## Index

| ADR | Title | Status |
| --- | --- | --- |
| [0001](0001-rust-for-the-trusted-core.md) | Rust for the trusted core | Accepted |
| [0002](0002-one-binary-two-processes-sandboxes.md) | One binary, two processes, sandboxes | Accepted |
| [0003](0003-event-log-is-the-database.md) | The event log is the database | Accepted |
| [0004](0004-one-signed-manifest-per-job.md) | One signed manifest per job, enforced only by the broker | Accepted |
| [0005](0005-provenance-is-a-type.md) | Provenance is a type; step roles are checked at install | Accepted |
| [0006](0006-plugins-are-skills-and-wasm-components.md) | Plugins are Agent Skills and WASM components; MCP servers run contained | Accepted |
| [0007](0007-capability-based-marketplace-trust.md) | Marketplace trust is capability-based and automated | Proposed |
| [0008](0008-cedar-for-policy.md) | Cedar for policy | Accepted |
| [0009](0009-gates-answer-allow-deny-unknown.md) | Every gate answers allow, deny or unknown | Accepted |
| [0010](0010-mcp-primary-interface-two-principals.md) | MCP is the primary interface; two principals | Accepted |
| [0011](0011-pluggable-substrates.md) | Pluggable substrates; best isolation by default; attestation recorded | Proposed |
| [0012](0012-humans-approve-only-irreversible-actions.md) | Humans set policy and approve only irreversible actions | Accepted |
| [0013](0013-deterministic-simulation-and-evidence-gated-releases.md) | Deterministic simulation testing and evidence-gated releases | Accepted |
| [0014](0014-tracker-holds-status.md) | The tracker holds status; PRDs hold intent; ADRs hold decisions | Accepted |
| [0015](0015-project-named-hyalos.md) | The project is named Hyalos | Accepted |
| [0016](0016-agent-runtime-and-broker-protocols.md) | Agent runtime, broker protocols and model credentials | Proposed |
