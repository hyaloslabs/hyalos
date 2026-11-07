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
| [0007](0007-capability-based-marketplace-trust.md) | Marketplace trust is capability-based and automated | Accepted |
| [0008](0008-cedar-for-policy.md) | Cedar for policy | Accepted |
| [0009](0009-gates-answer-allow-deny-unknown.md) | Every gate answers allow, deny or unknown | Accepted |
| [0010](0010-mcp-primary-interface-two-principals.md) | MCP is the primary interface; two principals | Accepted |
| [0011](0011-pluggable-substrates.md) | Pluggable substrates; best isolation by default; attestation recorded | Accepted |
| [0012](0012-humans-approve-only-irreversible-actions.md) | Humans set policy and approve only irreversible actions | Accepted |
| [0013](0013-deterministic-simulation-and-evidence-gated-releases.md) | Deterministic simulation testing and evidence-gated releases | Accepted |
| [0014](0014-tracker-holds-status.md) | The tracker holds status; PRDs hold intent; ADRs hold decisions | Accepted |
| [0015](0015-project-named-hyalos.md) | The project is named Hyalos | Accepted |
| [0016](0016-agent-runtime-and-broker-protocols.md) | Agent runtime, broker protocols and model credentials | Accepted |
| [0017](0017-workspace-layout-and-trusted-core-boundary.md) | Workspace layout and the trusted-core boundary | Accepted |
| [0018](0018-event-log-format-and-verification.md) | Event log format and verification | Accepted |
| [0019](0019-typed-events-and-in-memory-projections.md) | Typed events and in-memory projections | Accepted |
| [0020](0020-gate-answers-timeouts-and-holds.md) | Gate answers, timeouts and holds in the engine | Accepted |
| [0021](0021-backup-and-restore-of-the-event-log.md) | Backup and restore of the event log | Accepted |
| [0022](0022-engine-keys-generation-storage-backup-and-loss.md) | The engine's keys: generation, storage, backup and loss | Accepted |
| [0023](0023-event-schema-versions-and-upcasting.md) | Event schema versions and upcasting | Accepted |
| [0024](0024-event-origins-and-time-order.md) | Event origins and time order | Accepted |
| [0025](0025-releasing-a-hold-stakes-authority-and-evidence.md) | Releasing a hold: stakes, authority and evidence | Accepted |
| [0026](0026-compacting-the-log-by-signed-snapshot.md) | Compacting the log by signed snapshot | Accepted |
| [0027](0027-recording-the-pruning-of-an-archive.md) | Recording the pruning of an archive | Accepted |
| [0028](0028-auditing-snapshots-against-their-archives.md) | Auditing snapshots against their archives | Accepted |
| [0029](0029-cancelling-a-held-task-and-recording-a-refusal.md) | Cancelling a held task, and recording a refusal | Accepted |
| [0030](0030-hardening-the-engines-key-owner-and-core-dumps.md) | Hardening the engine's key: its owner, and core dumps | Accepted |
| [0031](0031-the-signed-manifest-and-its-admission.md) | The signed manifest and its admission | Accepted |
| [0032](0032-the-substrate-interface-and-its-contract.md) | The substrate interface and its contract | Accepted |
| [0033](0033-the-shipped-policy-schema-baseline-and-proofs.md) | The shipped policy schema, baseline and proofs | Accepted |
| [0034](0034-the-operator-policy-store-and-minting-within-policy.md) | The operator policy store and minting within policy | Accepted |
| [0035](0035-the-private-channel-between-engine-and-broker.md) | The private channel between the engine and the broker | Accepted |
| [0036](0036-the-simulated-world-and-the-log-scan.md) | The simulated world and the log scan | Accepted |
| [0037](0037-model-protocol-adapters-and-their-line-budget.md) | Model protocol adapters and their line budget | Accepted |
| [0038](0038-model-calls-from-the-sandbox-and-the-provider-key-file.md) | Model calls from the sandbox, and the provider key file | Accepted |
| [0039](0039-tls-to-model-providers.md) | TLS to model providers | Accepted |
| [0040](0040-the-sandbox-image.md) | The sandbox image | Accepted |
| [0041](0041-an-init-in-the-sandbox-and-the-image-tests-further-checks.md) | An init in the sandbox, and the image test's further checks | Accepted |
| [0042](0042-the-egress-gate-and-the-allowlisting-proxy.md) | The egress gate and the allowlisting proxy | Accepted |
| [0043](0043-choosing-the-substrate-recording-it-and-doctor.md) | Choosing the substrate, recording it on the job, and `hyalos doctor` | Accepted |
| [0044](0044-minting-per-job-github-tokens-from-the-app.md) | Minting per-job GitHub tokens from the App | Accepted |
| [0045](0045-forge-tokens-up-to-4-kib.md) | Forge tokens up to 4 KiB, and revoking a token the engine cannot use | Accepted |
| [0046](0046-the-git-proxy.md) | The git proxy | Accepted |
| [0047](0047-forge-content-as-typed-values-and-derived-taint.md) | Forge content as typed values, and taint derived from the step | Accepted |
| [0048](0048-opening-a-pull-request-and-the-content-gate-on-its-text.md) | Opening a pull request, and the content gate on its text | Accepted |
| [0049](0049-git-in-the-sandbox-image.md) | Git in the sandbox image | Accepted |
| [0050](0050-running-one-job-from-the-command-line.md) | Running one job from the command line | Accepted |
| [0051](0051-provider-hosts-the-egress-gate-never-reaches.md) | Provider hosts the egress gate never reaches | Accepted |
| [0052](0052-reading-the-jobs-issue-from-github.md) | Reading the job's issue from GitHub | Accepted |
| [0053](0053-an-interrupted-run-ends-its-job.md) | An interrupted run ends its job | Accepted |
| [0054](0054-the-content-gate-on-what-a-push-carries.md) | The content gate on what a push carries | Accepted |
| [0055](0055-data-flows-name-where-the-jobs-data-may-go.md) | Data flows name where the job's data may go | Accepted |
| [0056](0056-the-gvisor-substrate.md) | The gVisor substrate | Accepted |
| [0057](0057-recording-a-jobs-attestation.md) | Recording a job's attestation | Accepted |
| [0058](0058-the-firecracker-microvm-substrate.md) | The Firecracker microVM substrate | Accepted |
| [0059](0059-tainted-text-cannot-reach-an-actor-prompt-by-type.md) | Tainted text cannot reach an actor's prompt, by type | Accepted |
| [0060](0060-the-workflow-document-and-the-check-at-install.md) | The workflow document, and the check at install | Accepted |
| [0061](0061-reader-steps-run-in-the-broker.md) | Reader steps run in the broker, and the engine gets typed values | Accepted |
| [0062](0062-the-fix-issue-workflow-and-its-runner.md) | The fix-issue workflow, and the runner that mints only from the check | Accepted |
| [0063](0063-gated-handles-for-raw-text.md) | Gated handles: the broker holds raw text, a human releases it by digest | Accepted |
