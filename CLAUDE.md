# Hyalos

Clean-room Rust rewrite of hivrr: one `hyalos` binary (engine and broker) that runs coding agents in sandboxes holding no secrets, with every decision in a signed, hash-chained event log. Spike rigs under `spikes/` are not part of the workspace and not trusted core.

This file holds the rules. The design is in the ADRs: start at [`docs/adr/README.md`](docs/adr/README.md) and read the ADRs for the code you touch (the map below). Operator procedures are in `docs/operations/`.

## Documentation rules (see docs/README.md)

- Intent and scope: `docs/prd/`. Never put status there.
- Decisions: `docs/adr/NNNN-title.md`, one per file, from `0000-template.md`. Add new rows to `docs/adr/README.md`.
- Never edit Context, Decision or Consequences of an **Accepted** ADR. Write a new ADR that supersedes it and change only the old one's Status line. **Proposed** ADRs may be edited.
- A feature's ADR goes in with the feature as Proposed, may change in review, and is set to Accepted (Status line and README row) in that pull request's last commit before it merges. Superseding one also changes the old ADR's Status line and row in the same commit.
- Status lives in GitHub issues (epics as parent issues, milestones M1 to M4), not in this repo. `docs/planning/` is working material only.
- A change that a rule below names (a new tool, event, wire message, schema version) updates the ADR that owns it, or adds one.

## Two principals (ADR-0010, ADR-0070)

The driving agent and the operator are different principals, and an agent working on this repository is held to the same split:
- Never read, print or ask for a secret: `engine.key`, `operator.key`, `provider.keys`, `github-app.pem`, the operator's passphrase, an agent credential.
- Never run a decision: `hyalos approve`, `cancel`, `release`, `merge`, `policy approve`, `agent issue|revoke`, `run --release-issue`. The operator runs them in a terminal of their own (`tools_cmd::DECISIONS`).
- No MCP tool decides. A new MCP tool needs its thin-CLI command in the same change (`tools_cmd::COMMANDS`, ADR-0075).

## Rules for the code

- **Fail closed.** Every gate and check proceeds only on an on-time allow. An error, a timeout or anything it cannot read is `unknown` (the engine holds) or a refusal, never allow (ADR-0009, ADR-0020).
- **Echo nothing.** A secret or a tainted value never reaches `Debug`, an error message, a log line, or a reply to the sandbox or the agent. Map it to a `ReasonCode`.

## Before pushing

Run what CI runs (the toolchain is pinned in `rust-toolchain.toml`):

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo xtask layout
cargo xtask lines
cargo deny check        # if cargo-deny is installed; CI's dependency audit
```

A pull request that changes only `docs/`, `spikes/`, Markdown under `skills/` or Markdown at the root skips the build jobs and still reports its required checks (`changes` job in `ci.yml`).

Tests that need a tool skip without it unless CI's variable is set: `HYALOS_REQUIRE_CONTAINER`, `_GIT`, `_GVISOR`, `_MICROVM`, `_SANDBOX_IMAGE`, `_CVC5`, `_MCP_CONFORMANCE`. A skip is not a pass; say which ran.

## Code layout (ADR-0017)

- `core/` is the trusted core: everything that runs in the engine or broker process, the CLI included. It has a 30,000-line budget, and parts have their own in `PART_BUDGETS` (`crates/xtask`). Trusted-core paths need code-owner review (`.github/CODEOWNERS`).
- `crates/` holds the rest: checks, the simulator, test kits, sandbox tools. It is not counted, and `core/` uses it only as dev-dependencies, so none of it is in the binary.
- Dependency edges are a table in `crates/xtask`: only the engine and the `hyalos` crate (the engine process) depend on `hyalos-log`; the broker never depends on the engine, log or substrate crates; the engine never depends on the broker. A new `core/` crate or edge edits that table and the ADR.
- `hyalos-types`, `hyalos-log`, `hyalos-policy` and `hyalos-engine` are deterministic (ADR-0013). Time comes from `hyalos_types::Clock` and randomness from `hyalos_types::Entropy`. Their `clippy.toml` bans `SystemTime`, `Instant`, `HashMap`, `HashSet`, `RandomState` and `fs::read_dir`, and any external dependency needs an entry in `EXTERNAL_ALLOWED`.
- `cedar-policy` turns on `serde_json`'s `preserve_order` for the whole build, so nothing may rely on a `serde_json` map's key order. It is pinned `=4.14.1` to match `cedar-policy-symcc`.
- Licences and advisories are `deny.toml`'s; a new dependency may need an entry there.

## Changing what is stored or sent

These are pinned by tests. A change to one bumps its version (except where noted) and updates the ADR that owns it, or adds one:
- **Events** (ADR-0019, ADR-0023): the closed `hyalos_types::Event` set. A new or changed payload bumps `SCHEMA_VERSION`, adds the version's pinned payloads to `core/types/tests/schema.rs` and its upcast steps. The log is never rewritten. A change to what the state keeps bumps the `StateImage` version (ADR-0026).
- **Decisions** (ADR-0025, ADR-0070): `hold.resolved`, `task.cancelled`, `policy.approved`, `handle.released`, `agent.issued|revoked` and `merge.approved` are written only by their `Engine` method, signed by the operator's key for a human; `submit` refuses them raw and the broker may not send them.
- **The engine–broker channel** (ADR-0035): the closed `hyalos_types::wire` set. A new message bumps the wire version.
- **The manifest** (ADR-0031): pinned bytes and signature in `core/types/tests/manifest.rs`. A new optional field left out of the signed bytes when empty needs no version bump, since older manifests sign the same; anything else bumps the manifest version.
- **Reasons** are `ReasonCode`s, never free text. Forge and model text are tainted types with no `as_str` (ADR-0047, ADR-0059); never add a way out.

## Where things are

| Code | What | ADRs |
| --- | --- | --- |
| `core/types` | Events, schema, manifest, wire, provenance, prompts, workflows, results, plugin grants, approvals, merges | 0019, 0023, 0024, 0031, 0035, 0043, 0045, 0047, 0057, 0059, 0060, 0063, 0064, 0066, 0068, 0070, 0071, 0079 |
| `core/log` | The signed SQLite log, compaction, pruning, backup | 0018, 0021, 0026, 0027, 0028 |
| `core/policy` | Cedar, the shipped baseline and schema, gates | 0008, 0009, 0020, 0033 |
| `core/engine` | State, holds, cancels, compaction and audit, the policy store, minting, handles, agents, several writers | 0013, 0024, 0025, 0026, 0027, 0028, 0029, 0031, 0034, 0047, 0055, 0063, 0066, 0068, 0069, 0070, 0071 |
| `core/broker` | Channel, model relay and TLS, egress, git proxy, forge, content gates, readers, handles, results, merge | 0035, 0037, 0038, 0039, 0042, 0046, 0047, 0048, 0051, 0054, 0055, 0061, 0063, 0064, 0065, 0071, 0079 |
| `core/substrate` | Containers, gVisor, Firecracker, choosing one, attestation | 0032, 0041, 0043, 0056, 0057, 0058 |
| `core/hyalos` | The binary: keys, log commands, the broker process and channel, doctor, GitHub App, `hyalos run`, the fix-issue workflow, MCP server, operator commands and merge, thin CLI, setup | 0018, 0021, 0022, 0026–0028, 0030, 0035, 0038, 0043, 0044, 0045, 0050, 0052, 0053, 0058, 0062, 0063, 0067–0075, 0077, 0078 |
| `crates/xtask` | `layout` and `lines` | 0017 |
| `crates/sim`, `crates/world` | Deterministic simulation and the simulated world | 0013, 0036 |
| `crates/substrate-contract`, `crates/vm-init` | The contract every substrate passes; the microVM's init | 0032, 0041, 0056, 0058 |
| `crates/sandbox-tools`, `images/sandbox` | The sandbox image and the tools in it | 0016, 0040, 0049, 0064 |
| `crates/policy-proof` | Symbolic proofs of the baseline (cvc5) | 0033 |
| `crates/mcp-conformance` | The pinned MCP conformance suite | 0067, 0069 |
| `crates/redteam` | The red-team corpus | 0065 |
| `skills/set-up-hyalos` | The setup skill (`.claude/skills/set-up-hyalos` links to it; edit this one) | 0077, 0078 |

The image builds from the repository root and must name its target: `--target sandbox -t hyalos-sandbox:latest` for jobs, `--target test -t hyalos-sandbox:test` for the image tests (they skip without that tag), as in `docker build -f images/sandbox/Containerfile --target test -t hyalos-sandbox:test .`. Regenerate the harness lockfile with the image's own npm, not the host's.
