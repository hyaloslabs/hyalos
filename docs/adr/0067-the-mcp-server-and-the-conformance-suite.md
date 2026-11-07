# ADR-0067: The MCP server, its two transports, and the conformance suite it passes

**Status:** Accepted

## Context

S5.1 (#29) is the first story of E5. [ADR-0010](0010-mcp-primary-interface-two-principals.md) makes an MCP server on revision 2026-07-28 the engine's primary interface. It listens locally, with no network listener unless the operator turns one on, and its protocol code sits behind an adapter because MCP revises yearly. The story's one acceptance is: "The official conformance suite passes."

The revision, as read on November 7, 2026 from [its specification](https://modelcontextprotocol.io/specification/2026-07-28) and the [changelog](https://modelcontextprotocol.io/specification/2026-07-28/changelog):
- **No sessions.** `initialize` and `Mcp-Session-Id` are gone.
- **Every request carries `_meta`.** It gives `io.modelcontextprotocol/protocolVersion` and `io.modelcontextprotocol/clientCapabilities`. A request without them is `-32602`, and another version is `-32022` naming the supported ones.
- **`server/discover` is required.**
- **Every result has `resultType`.** Discovery and list results also carry `ttlMs` and `cacheScope`.
- **Streamable HTTP adds headers.** It requires `MCP-Protocol-Version`, `Mcp-Method` and, on a call, `Mcp-Name`, each equal to the body, or `-32020`. GET is gone, so GET and DELETE are 405. A bad `Origin` is 403.
- **stdio stays.** It is newline-delimited JSON-RPC.
- **Authorization is optional.**

The [conformance suite](https://github.com/modelcontextprotocol/conformance) tests servers over HTTP only. It has a frozen list of what 2026-07-28 requires: 37 server scenarios. Most need features the engine has no use for yet:
- resources, prompts and completions;
- 15 scenarios for input-required results, the revision's replacement for server-sent sampling, elicitation and roots requests.

Many of the rest call fixture tools by fixed names from the suite's reference server, such as `test_simple_text`, `test_image_content` and `test_missing_capability`. A missing fixture fails rather than skips. An `--expected-failures` file lets a run exit 0 with listed failures, and a listed entry that passes fails the run.

## Decision

**A generic protocol layer in the core, its own tools outside it.** `hyalos::mcp` (`core/hyalos/src/mcp/`) answers one message at a time with nothing kept between calls. `Server::handle(message, headers, emit)` returns the JSON-RPC response and the HTTP status the revision gives it. The order of checks is:
1. not JSON-RPC (`-32700`, `-32600`, with no id when none could be read);
2. `_meta` (`-32602`);
3. on HTTP, the headers against the body (`-32020`);
4. the version (`-32022`, with `supported` and `requested`);
5. the method (`-32601`, 404; `initialize` is also told the supported revision);
6. the tool (`-32602`) and the client capabilities it declares it needs (`-32021` with `requiredCapabilities`, before it runs).

What a server offers is a `Tools`: its `Tool` definitions (name, title, description, input and output schema, `requires`) and `call`. `call` returns a `Called` (content blocks, structured content, `is_error`) and may report progress through `Progress`, which sends `notifications/progress` only when the request gave a `progressToken`. The server offers tools only: `capabilities` is `{"tools": {}}`, with no `listChanged`. Results are `resultType: complete` with the server's identity in `_meta`. Discovery and `tools/list` say `ttlMs` 300,000, `cacheScope` public. Every tool is on one page, so a cursor is refused. It never logs, so it never sends `notifications/message`. It has no part in the extensions (Tasks and MCP Apps come in S5.2 and S5.3), so it advertises none and ignores the client's.

**Two transports.**
- **stdio, the default.** `hyalos mcp` serves stdio: one message per line in and out until the input ends, with progress lines before the answer. A line over 1 MiB is refused and skipped. This is what a client that starts the server as a command speaks, such as `claude mcp add hyalos -- hyalos mcp`.
- **Streamable HTTP, opt-in.** `hyalos mcp --listen <address:port>` serves Streamable HTTP at `/mcp`, and only on a loopback address (`127.0.0.1` or `[::1]`); anything else is refused before binding. It allows one POST per message and one request per connection, with `Content-Length` bodies of at most 1 MiB, 16 KiB of headers and 32 connections at once. A call that reports progress, to a client that accepts a stream, is answered with server-sent events: the notifications, then the result. Everything else is JSON.
- **HTTP refusals.** A Host or Origin other than `localhost`, `127.0.0.1` or `[::1]` with this port is 403; that is the defence against DNS rebinding. Another path is 404, a method other than POST is 405, and a body that is not JSON is 415. `Mcp-Name` in the revision's `=?base64?…?=` form is decoded before it is compared.
- **No credential yet.** Neither transport authenticates the client. The driving agent's credential is S5.8's (#76). Until then, anyone who can start the binary as the operator, or reach the operator's loopback port, can use the server. That is why the server only reads and HTTP is off by default.

**Its first tool reads, and needs no secret.** `list_tasks` (`mcp_cmd`; `limit` 1 to 100, default 20) returns the newest tasks with each one's status and each of its jobs' outcomes, as structured content with an output schema. It returns only ids, statuses and times, never text, so nothing tainted reaches the driving agent (results with text are S5.10's). It reads the log with `hyalos_engine::read_state(path, &PublicKey)`: the log is checked against `engine.pub` and its state rebuilt from the latest snapshot. It never opens `engine.key`, so it reads while `hyalos run` writes. A missing key or log, or a log that does not verify, is a tool error that says so. Starting work is S5.2's.

**The suite passes, run against the protocol layer with the suite's fixtures.** `crates/mcp-conformance` (not core, never linked into the binary) serves the reference server's fixture tools through `hyalos::mcp::http` with the same names and content:
- text, image, audio, an embedded resource and mixed content;
- a tool error;
- progress;
- a tool that needs `sampling`;
- a logging probe.

`tests/conformance.rs` runs the suite, pinned at `@modelcontextprotocol/conformance` 0.2.0-alpha.12 with its lockfile, as `server --requirements 2026-07-28 --expected-failures expected-failures.yml`. It asserts that the run exits 0. The new `mcp conformance` CI job installs the suite with `npm ci --ignore-scripts` and sets `HYALOS_REQUIRE_MCP_CONFORMANCE`; without the suite the test skips. 0.2.0-alpha.12 is pinned rather than alpha.10, the release the requirement list was frozen from, because it is the one that runs that frozen list by name. The list is the same in both.

**What the expected failures are.** Every entry in `expected-failures.yml` is a feature the server does not offer, and the file is exact:
- completions;
- resources (four scenarios, the not-found warning and two caching checks);
- prompts (five scenarios and one caching check);
- every input-required scenario that calls one of its fixtures (12);
- the one stateless check that needs such a fixture to open a stream.

The input-required results are not offered because no engine tool asks the client for anything: approvals go through the operator channel, never the client ([ADR-0012](0012-humans-approve-only-irreversible-actions.md)). Every other required scenario passes. That covers the 24 stateless checks, the tool list and every tool-call scenario, concurrent streams, DNS rebinding, caching for tools and discovery, and the input-required checks that apply to a server without the feature.

## Consequences

S5.1's acceptance holds: the official suite passes every scenario 2026-07-28 requires, except those of features the server does not offer, which are listed one by one. Offering one later means implementing it and deleting its entries, and the suite fails a run until both are done. The protocol is checked twice. The suite runs end to end in its CI job, and `hyalos::mcp`'s unit tests check the same rules everywhere `cargo test` runs:
- `_meta`, versions, removed methods, notifications;
- capabilities before a tool runs;
- progress only with a token;
- headers against the body;
- stdio framing;
- loopback-only binding, Host and Origin, 405, streaming, base64 names.

`core/hyalos/tests/mcp.rs` runs the binary on stdio against a real signed log.

**What this does not decide:**
- **The driving agent's credential and its revocation** (S5.8). Until then the server only reads, and the HTTP listener is the operator's choice.
- **Starting jobs as MCP Tasks, or as plain tools where the client lacks the extension** (S5.2).
- **Approvals and results in the client** (S5.3, S5.4).
- **Marking tainted text in results** (S5.10).
- **A local socket.** ADR-0010 names a local socket. stdio is that here: a client starts the server as its own child, and no listener is open at all. A Unix-socket listener can be added behind the same `Server` if a client needs one.
- **Revisions other than 2026-07-28.** An older client is told which revision this is. Serving it would need the stateful handshake back, which no target client needs.

**Costs.** 838 more lines of trusted core:
- the protocol layer and its two transports (654 in `core/hyalos/src/mcp`, which gets a part budget of 1,000 in `cargo xtask lines`);
- the command and `list_tasks` (about 180);
- `read_state` (+4, engine).

Trusted core goes from 16,499 to 17,337 lines of 30,000. CI gains a job that needs npm's registry, pinned by the lockfile's integrity hashes.
