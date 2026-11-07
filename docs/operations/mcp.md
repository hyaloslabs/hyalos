# The MCP server

`hyalos mcp` is the engine's MCP server, on revision 2026-07-28 ([ADR-0010](../adr/0010-mcp-primary-interface-two-principals.md), [ADR-0067](../adr/0067-the-mcp-server-and-the-conformance-suite.md)). For now it has one tool, `list_tasks`. It returns the newest tasks in the event log, with each task's status and the outcome of each of its jobs. Starting work and approvals come in later stories.

It reads the log with the engine's public key only. It never opens `engine.key`, so it runs while `hyalos run` writes. Run `hyalos init` first: without `engine.pub` and `events.db`, `list_tasks` says what is missing.

## From a client that starts it (stdio)

This is the default, and there is no listener at all. In Claude Code:

```
claude mcp add hyalos -- hyalos mcp
```

Set `HYALOS_DATA_DIR` and `HYALOS_CONFIG_DIR` in the client's server entry if your log and key are not in the default places. Run `hyalos mcp` by hand to check it: it reads one JSON-RPC message per line on stdin and answers on stdout until stdin ends.

## Over HTTP on loopback

```
hyalos mcp --listen 127.0.0.1:8765
```

This serves Streamable HTTP at `http://127.0.0.1:8765/mcp`. Only a loopback address is accepted. A request whose Host or Origin is not that address and port is refused with 403, so a web page cannot reach the server by rebinding a name to 127.0.0.1.

There is no authentication yet. Any process of any user on the machine that can reach the port can call `list_tasks`. That only reads ids and statuses, but turn HTTP on only while you need it. The driving agent's credential comes with S5.8.

## Conformance

CI runs the official MCP conformance suite against the protocol layer on every pull request (job `mcp conformance`). To run it locally:

```
npm ci --ignore-scripts --prefix crates/mcp-conformance
HYALOS_REQUIRE_MCP_CONFORMANCE=1 cargo test -p hyalos-mcp-conformance
```

`crates/mcp-conformance/expected-failures.yml` lists the features the server does not offer: completions, resources, prompts and input-required results. Every other scenario the revision requires must pass.
