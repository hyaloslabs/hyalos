# Spike #72: a real harness through a loopback forwarder in a no-network sandbox

Working material, run November 1, 2026. It answers open question 3 in part. What it settled is folded into [ADR-0016](../adr/0016-agent-runtime-and-broker-protocols.md); this file keeps the evidence and the method.

## Question

Does a real harness (Claude Code or Codex) work when its only route out is a loopback port inside a sandbox with no network, relayed to the broker's channel, with a placeholder key in the sandbox?

## Method

- **Sandbox:** an Ubuntu 24.04 container run with `--network none --user 1000:1000 --read-only --tmpfs /tmp`. The only interface is loopback. The harness binary is mounted read-only from the host.
- **Forwarder:** `socat TCP-LISTEN:8089,bind=127.0.0.1,fork,reuseaddr UNIX-CONNECT:/run/broker.sock`, started by the image entrypoint. A Unix socket on the host is the broker's channel and is the only thing mounted in.
- **Broker stand-in:** a small Node server on that socket. In scripted mode it speaks Anthropic Messages and OpenAI Responses (streaming) with no model, replaying one tool call that runs `hyalos-submit`, then a final answer. In passthrough mode (OpenAI only) it behaves as the real broker would: it drops the placeholder credential, adds the real key on the upstream call, removes provider-hosted tools, and never logs the key.
- **`hyalos-submit`:** a shell script that POSTs a JSON result to the forwarder.
- **Traffic observation:** an internal Docker network with a DNS sinkhole that logs every name a harness looks up. It records names asked for, not connections by IP.
- **Versions:** Claude Code 2.1.316, Codex 0.114.0, Docker 29.1.3 on an Ubuntu 24.04 host under WSL2.

## Results

| Check | Claude Code | Codex |
| --- | --- | --- |
| Completes with base URL on the forwarder and a placeholder key (scripted broker) | Yes, 0.8 s | Yes, 1.1 s with the settings below |
| Completes a task against a live model | Not run: no Anthropic key was available | Yes: `gpt-5-mini`, two upstream calls, both 200, about 6,600 tokens |
| Placeholder reaches the broker as | `x-api-key` header | `Authorization: Bearer` header |
| Sends tool definitions on every request | Yes | Yes |
| Other requests through the base URL | None | None |
| Direct outbound connection from the sandbox | Fails (no route, DNS fails) | Fails |

The live Codex task wrote a file, read it back, and submitted a typed result through `hyalos-submit`. The broker received the result, and the sandbox never held anything but the placeholder.

## Background traffic and the settings that stop it

| Harness | Without settings | Setting | With setting |
| --- | --- | --- | --- |
| Claude Code | Looks up `api.anthropic.com` three times despite the base URL | `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`, `DISABLE_AUTOUPDATER=1`, `DISABLE_TELEMETRY=1`, `DISABLE_ERROR_REPORTING=1` | No lookups |
| Codex | Looks up `ab.chatgpt.com` 11 times and waits about 5 s at startup | `-c analytics.enabled=false` | No lookups, startup 1.1 s |

The Claude Code settings are documented in its environment-variables reference. The Codex setting is named in the binary's own help text. `-c feedback.enabled=false` made no further difference.

## Built-in web access

- **Claude Code** lists `WebFetch` and `WebSearch` in its default request as tools available on demand, together with `Agent`, `Workflow` and `ScheduleWakeup`. `--tools "Bash,Read,Edit"` removes all of them. `--disallowedTools WebFetch WebSearch` removes only the web tools. `--bare` sends only `Bash`, `Edit` and `Read`.
- **Codex** advertises `web_search` as a provider-hosted tool: the provider runs it, outside the sandbox's network, and a query string can carry data out. `-c web_search=disabled` removes it. These forms did not work: `web_search=false` and `tools.web_search=false` (rejected), `--disable web_search_request` (no effect).
- **Broker-side removal works.** With Codex advertising `web_search`, the passthrough broker removed it from the upstream request. The live call returned 200 and Codex completed the task.

## Other findings

- A Unix socket path is limited to about 107 characters. A long working-directory path made the broker fail to bind. Per-job socket paths in the real substrate must be short.
- Codex needs its home directory (`CODEX_HOME`) to exist before it starts.
- The container is the sandbox, so Codex ran with `--sandbox danger-full-access`. Codex's own sandbox nested inside the container was not tried.
- Claude Code requests `/v1/messages?beta=true`, so the broker's path matching must accept a query string. It selected its own model (`claude-opus-5-5`), so the broker needs a model allowlist or mapping.
- Claude Code 2.1.316 and Codex 0.114.0 are single binaries that mount cleanly into a stock Ubuntu image. Claude Code is glibc-linked and Codex is static musl.

## Not tested

- Claude Code against a live model.
- The providers' Agent SDKs, which ADR-0016 lists as supported.
- Needed non-model traffic through the egress allowlist, for example a proxy setting that points at the forwarder.
- Real streaming timing, retries, rate-limit errors and prompt caching. The scripted broker sends each response in one burst, and the forwarder is a plain TCP relay.
- Codex's `responses_websockets` feature, which is still under development. A TCP relay should carry it if the broker accepts the upgrade.
- macOS and other substrates. This ran on Linux with Docker.

The rig is about 150 lines and is not in the repository, which holds docs only for now.
