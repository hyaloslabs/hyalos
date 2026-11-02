# Spike #72: test rig

Throwaway tooling that produced the evidence in [`docs/planning/spike-72-findings.md`](../../docs/planning/spike-72-findings.md). It is **not product code**: it is not part of the Cargo workspace, not trusted core, and nothing depends on it. Delete it when the real substrate and broker make it redundant.

## What it does

A harness (Claude Code, Codex, or either Agent SDK) runs in a Docker container with `--network none`. A `socat` forwarder in the container relays a loopback port to a Unix socket on the host. A small Node server on that socket stands in for the Hyalos broker.

| Piece | Role |
| --- | --- |
| `stub-broker.mjs` | The broker stand-in. Scripted replies with no model, or passthrough to the real APIs (it swaps the placeholder key for the real one, removes provider-hosted tools, and never logs the key). Also an allowlisting CONNECT and HTTP proxy, and an injector of provider-shaped errors. |
| `image/` | The sandbox image (forwarder, `hyalos-submit`) and the DNS-sinkhole image. |
| `start-broker.sh` | Starts the stand-in. |
| `run-claude.sh`, `run-codex.sh`, `run-sdk.sh` | Run one harness or SDK in the sandbox against the stand-in. |
| `fault.sh` | Injects an error (rate limit, overload, budget exhausted) and reports how the harness reacted. |
| `sinkhole.sh` | Starts an internal network with a logging DNS sink, to list the hostnames a harness tries to reach. |
| `summarize.py` | Prints what the stand-in saw. |
| `sdk/` | The two Agent SDK test scripts. |

## Prerequisites

Docker, Node 20 or later, and the Claude Code and Codex binaries, which are mounted read-only into the container. `CLAUDE_BIN` (default `~/.local/bin/claude`) and `CODEX_VENDOR_DIR` (default the npm-global Linux x64 vendor directory) point at them. Tested with Claude Code 2.1.316, Codex 0.114.0, Claude Agent SDK 0.3.316, OpenAI Agents SDK 0.21.0 and Docker 29.1.3 on Ubuntu 24.04. Other versions may behave differently; that is part of what the findings record.

## Run it

```bash
cd spikes/72
docker build -t hyalos-spike72 image          # the sandbox image
(cd sdk && npm install)                       # only for the SDK tests

PASSTHROUGH=none ./start-broker.sh            # scripted replies: no keys, no model, no spend
./run-claude.sh                               # Claude Code completes a scripted task
./run-codex.sh                                # Codex completes a scripted task
python3 summarize.py                          # what the stand-in saw
```

For live runs, put `ANTHROPIC_API_KEY` and `OPENAI_API_KEY` in the repository's `.env` (it is git-ignored) and start the broker with `./start-broker.sh`. The keys are read into the broker's own process only; the container receives a placeholder. Use small models and a spend-capped key.

Useful variations:

```bash
./run-claude.sh --model claude-haiku-5-5 --tools "Bash"
./run-codex.sh -c model=gpt-5-mini -c analytics.enabled=false -c web_search=disabled
./run-sdk.sh claude        # or: openai
./fault.sh claude "402 budget exhausted" '{"status":402,"count":-1,"type":"billing_error"}'
./sinkhole.sh up           # then: NETARGS="--network spike72net --dns 172.29.72.2" ./run-claude.sh
```

`ALLOW_HOSTS=example.com,other.org ./start-broker.sh` sets the proxy allowlist. Set `EXTRA_DOCKER="-e HTTPS_PROXY=http://127.0.0.1:8089 -e NO_PROXY=127.0.0.1,localhost"` to send a harness's other traffic through it.

## Known gaps

- Scripted mode (`PASSTHROUGH=none`) works with Claude Code, Codex and the Claude Agent SDK. It does not work with the OpenAI Agents SDK, whose stream parser rejects the scripted events; run that SDK live.
- Only the OpenAI Responses and Anthropic Messages endpoints are handled. Anything else gets a 404 and is logged.

## Cautions

- `start-broker.sh` stops any running `stub-broker.mjs` first. The Unix socket lives at `/tmp/s72/broker.sock`; a socket path is limited to about 107 characters, so keep it short.
- Logs and request bodies go to `run/`, which is git-ignored. They contain prompts and model output, so do not commit them.
- The stand-in's allowlisting proxy is a demonstration of the traffic path, not a hardened design. It allows any port on an allowed host, does not check what a name resolves to, and has no size or time limits. The real broker's version is S3.5 (#20).
- The broker holds the real keys in passthrough mode, so `start-broker.sh` makes `/tmp/s72` private (mode 700). Do not loosen it.
- Never put a key in these files or on a command line.
