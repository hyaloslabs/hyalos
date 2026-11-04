# ADR-0040: The sandbox image

**Status:** Proposed

## Context

[ADR-0016](0016-agent-runtime-and-broker-protocols.md) decides that a harness in the sandbox reaches the broker through a forwarder on a loopback port, returns its result with a `hyalos-submit` helper, and runs with settings that turn off every call but its model's. Spike #72 found those settings and the harness versions they hold for. The substrates mount the broker's socket at `/run/broker.sock` and run the program they are given as the entrypoint ([ADR-0032](0032-the-substrate-interface-and-its-contract.md)). The broker serves each job on a socket of its own ([ADR-0038](0038-model-calls-from-the-sandbox-and-the-provider-key-file.md)). No image had been built that puts this together (story S2.9, #103).

## Decision

**One image, built from `images/sandbox/Containerfile`.** It ships the four harnesses v1 supports, pinned exactly in `images/sandbox/harness/package.json`, with every dependency in its lockfile:
- Claude Code 2.1.316;
- Codex 0.114.0;
- Claude Agent SDK 0.3.316;
- OpenAI Agents SDK 0.21.0.

Both base images are pinned by digest: `node:22-bookworm-slim` (Claude Code needs Node 22) and `rust:1.99.1-slim-bookworm` for building the sandbox tools. The musl builds of the Claude binaries are removed, because the image is glibc. The image is about 1.1 GB. One image serves every job; a version change rebuilds all of it.

**The sandbox's programs are Rust, in `crates/sandbox-tools`.** They are not trusted core: they run as the sandbox's user next to the harness, and the broker checks everything that arrives through them. They use only the standard library.
- **`hyalos-forward <program> [<args>...]`** is the program a job runs. It does three things in order:
  - makes `$HOME` and `$CODEX_HOME` on the sandbox's empty `/tmp`, and copies the shipped Codex settings into the latter, since Codex needs that directory at start;
  - listens on `127.0.0.1:8089` and relays each connection, up to 64 at once, byte for byte to `/run/broker.sock`;
  - runs the harness, and exits as it did.

  It is the image's entrypoint too, but a substrate sets the entrypoint itself, so a job's command starts with it.
- **`hyalos-submit [<json>]`** posts a result of at most 1 MiB to `/hyalos/submit` through the forwarder, and succeeds only on a 2xx answer. The broker does not serve that path yet: validating results is S4.7 (#84), and until then a submission is refused with 404.

**The image sets every ADR-0016 setting.**
- **For all four harnesses:**
  - base URLs to the forwarder, and placeholder keys;
  - `HTTPS_PROXY` and `HTTP_PROXY` to the forwarder (in both cases), and `NO_PROXY=127.0.0.1,localhost`.
- **Claude Code and its SDK:**
  - `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`, `DISABLE_AUTOUPDATER`, `DISABLE_TELEMETRY` and `DISABLE_ERROR_REPORTING`, each set to 1;
  - `skipWebFetchPreflight` in `/etc/claude-code/managed-settings.json`. It is set for every step, not only those with web fetch, because skipping a check against `api.anthropic.com` costs nothing when web fetch is off.
- **Codex** (`/etc/hyalos/codex/config.toml`):
  - the forwarder as its model provider;
  - `analytics.enabled=false` and `web_search="disabled"`;
  - `sandbox_mode="danger-full-access"`, so the container is the boundary;
  - `approval_policy="never"`.
- **The OpenAI Agents SDK:** `OPENAI_AGENTS_DISABLE_TRACING=1`.

Claude Code's `--tools` depends on the step, so whoever builds the job's command sets it, not the image.

**The test checks two places, because the proxy hides lookups.** A harness that honors `HTTPS_PROXY` never resolves a name itself: its other traffic arrives on the broker channel as a proxy request. So `crates/sandbox-tools/tests/image.rs` checks both:
- **DNS:** it runs each harness with the container substrate's isolation (no network, read-only root, an empty `/tmp`, user 65534, no capabilities) under `hyalos-dns-log`, a test-only program in the image's `test` target that answers DNS on the sandbox's loopback and lists every name asked;
- **the channel:** it gives the sandbox a relay to the broker's socket that records the first line of every request.

The broker is the real one, run in the test, with both providers pointed at a scripted model on loopback, so no key and no model is involved. Each harness must:
- finish;
- reach its model through the broker;
- make no lookup;
- send nothing on the channel but model calls.

Two controls show the checks are not blind. With Codex's analytics turned back on, its tracking call arrives on the channel as `CONNECT ab.chatgpt.com:443`. With the proxy variables also cleared, it is a lookup the DNS log sees.

**CI runs the test when the image could change.** The `sandbox image` workflow builds both targets and runs the test, which then cannot skip, whenever any of these change:
- the image, including its pinned versions;
- the sandbox tools;
- the broker or the shared types;
- the lockfile;
- the workflow itself.

Elsewhere the test skips when the image is missing.

**A job socket path that is too long is refused at job start.** `JobDir::create` refuses a runtime directory whose job socket path would exceed `hyalos_types::wire::MAX_SOCKET_PATH` (100 bytes; a Unix socket path is limited to about 107). The error names the directory and the limit. The broker refuses the same paths with that same constant.

## Consequences

A job can run any of the four harnesses with nothing but a placeholder key and the broker's socket, and the image test shows none of them makes a call other than its model's. Changing a harness version means changing `package.json`, regenerating the lockfile with the image's own npm, and letting the workflow run. If a new version makes another call, the test fails and names it.

Not here:
- **Building a job's command from its step** (which harness, its `--tools`, the prompt). That comes with the first command that starts the engine.
- **The broker's allowlisting proxy (S3.5, #20).** Until it exists, the broker refuses every proxy request.
- **Result validation (S4.7, #84).**
- **Running the image test under gVisor.**
- **Images for other architectures.**
