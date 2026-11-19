# Data flows

Where each secret lives, who reads it, and what crosses each boundary. The threats these flows defend against, and the tests that prove each control, are in [threat-model.md](threat-model.md). The flow names a manifest declares (`job->model`, `job->forge`, `job->web`) are defined in [ADR-0055](../adr/0055-data-flows-name-where-the-jobs-data-may-go.md).

## Secrets

| Secret | Lives | Read by | Crosses | Code |
| --- | --- | --- | --- | --- |
| `engine.key`, the log's Ed25519 key | Config directory, mode 0600, owner checked | The engine process only | Nothing. `engine.pub` is public. | `core/hyalos/src/keys.rs::load` |
| The manifest key | Engine memory. A fresh key for each run, never stored. | The engine | Its public half goes to the broker in `ToBroker::Hello` | `core/hyalos/src/launch.rs::fresh_manifest_key` |
| `operator.key` | Config directory, sealed under the operator's passphrase (PBKDF2 and ChaCha20-Poly1305). The passphrase comes from the terminal. | Only the operator's decision commands | Only its signatures enter the log. `operator.pub` is public. | `core/hyalos/src/operator.rs::unlock`; `core/hyalos/src/passphrase.rs::read` |
| `provider.keys` | Config directory, mode 0600 | The broker process only. The engine passes only its path. | Never the engine, the wire or the sandbox. The sandbox holds a placeholder, and the broker swaps the header upstream. | `core/hyalos/src/broker_cmd.rs::providers`; `core/broker/src/model/relay.rs::Relay` |
| `github-app.pem` | Config directory, owner checked | The engine, and the `merge`, `release` and `github check` commands | Never the broker or the sandbox. Only tokens minted from it leave the engine. | `core/hyalos/src/github.rs::GitHubApp::load` |
| A per-job GitHub token | Minted by the engine, scoped to the manifest's repository and actions | The engine, then the broker's memory while the job is live | Engine to broker in `ToBroker::StartJob`, the only secret on the wire. Broker to GitHub for git, issues, pull requests and merges. Revoked when the job ends. Never reaches the sandbox, and is on the content gates' list of held secrets. | `core/hyalos/src/run_cmd/drive.rs::Run::token`; `core/broker/src/serve.rs::Broker::forge_token` |
| An agent credential | Printed once to the operator, then held by the driving agent | The MCP server | Only its SHA-256 is recorded in the log (`agent.issued`) | `core/hyalos/src/agent_cmd.rs::fresh_credential`, `credential_digest` |
| The operator's passphrase | The operator's terminal | Nothing stores it | Never an argument, an environment variable or a file | `core/hyalos/src/passphrase.rs::hidden` |

## Into the sandbox

- The image, with the harness and its tools.
- The command, including the actor's prompt. The prompt is built as an `ActorPrompt` from operator text, plugin text and typed values only. Forge and model text cannot be put into it (`core/types/src/prompt.rs::ActorPrompt`).
- The environment. It is the whole environment, with nothing inherited: proxy variables that point to the forwarder, and placeholder keys (`core/substrate/src/spec.rs::SandboxSpec`).
- Installed skills, read-only (`SKILLS_IN_SANDBOX`), and the broker's socket (`BROKER_SOCKET_IN_SANDBOX`).
- Released issue text, only through a handle the manifest names (`core/broker/src/handles.rs::for_manifest`).

The sandbox has no network and runs as a non-root user.

## Out of the sandbox

Everything leaves through the job's socket, via `hyalos-forward` (`crates/sandbox-tools/src/bin/hyalos-forward.rs`). Each route meets one gate (`core/broker/src/sandbox.rs::connection`, `sink`):

| Route | Flow | Gate |
| --- | --- | --- |
| `/v1/...` model calls | `job->model` | The relay swaps in the key, scrubs hosted tools, meters the budget, and enforces the reader's role |
| `CONNECT` or an absolute URL | `job->web` | The egress allowlist, public addresses only, never a provider host |
| `/git/` push | `job->forge` | The git proxy's repository and branch-prefix checks, then the pack content gate |
| `/git/` fetch | none (it needs `read`) | The git proxy's repository check |
| `/forge/.../pulls` | `job->forge` | The manifest's repository, head and base, then the content gate on the text |
| `/handle/` | none | Served only if the manifest names the handle |
| `/hyalos/submit` | none | Typed values against the manifest's result schema |

The job's stdout is capped (`OUTPUT_LIMIT`).

## Broker to engine

Only the closed `ToEngine` set crosses: `Ready`, `Admitted`, `Refused` (a reason code), `Issue` (a capped, typed `ForgeIssue`), `Held` (a digest and the writers' permissions), `Values` (typed), `Merged` (a commit SHA) and `Gate`. The engine records only `gate.answered` from the broker (`core/types/src/event.rs::Event::may_come_from`). Model text and forge content cannot be put into a `ToEngine` message (`core/types/src/wire.rs::model_text_and_forge_content_cannot_cross_to_the_engine`).

## Engine to the driving agent

The agent sees task status, results and approvals over MCP or the thin CLI. Data derived from untrusted input arrives wrapped in an `<untrusted-data>` element it cannot close (`core/hyalos/src/mcp/mod.rs::untrusted_block`). The agent cannot decide anything. The operator runs every decision in a terminal of their own (`core/hyalos/src/tools_cmd.rs::DECISIONS`).
