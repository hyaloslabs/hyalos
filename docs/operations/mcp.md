# The MCP server

`hyalos mcp` is the engine's MCP server, on revision 2026-07-28 ([ADR-0010](../adr/0010-mcp-primary-interface-two-principals.md), [ADR-0067](../adr/0067-the-mcp-server-and-the-conformance-suite.md)). For now it has one tool, `list_tasks`. It returns the newest tasks in the event log, with each task's status and the outcome of each of its jobs. Starting work and approvals come in later stories.

It reads the log with the engine's public key only. It never opens `engine.key`, so it runs while `hyalos run` writes. Run `hyalos init` first.

## Issuing the agent a credential

Every call needs a credential you issued ([ADR-0068](../adr/0068-the-driving-agents-credential.md)). Issue one per agent:

```
hyalos agent issue claude-code
```

This prints the credential on stdout, once. The log keeps only its SHA-256, so it cannot be shown again: if you lose it, revoke it and issue another. Treat it like a password and give it only to the agent.

```
hyalos agent list            # every credential issued, by id and name, and whether it is revoked
hyalos agent revoke <id>     # the server refuses it from its next call; no restart needed
```

A name belongs to one credential in force at a time. To replace a credential, revoke the old one, then issue the new one under the same name. A revoked credential never works again.

Without a credential in force, the server answers every call 401 (`-32030`). If it cannot read the log to check, it answers 503 and says why on its stderr.

The credential lets the agent call the server. It cannot approve anything: approvals are written with the engine's key by your commands, never through the server.

## From a client that starts it (stdio)

This is the default, and there is no listener at all. The server takes the credential from `HYALOS_AGENT_CREDENTIAL`. In Claude Code:

```
claude mcp add hyalos -e HYALOS_AGENT_CREDENTIAL=hyalos-agent-… -- hyalos mcp
```

Set `HYALOS_DATA_DIR` and `HYALOS_CONFIG_DIR` in the client's server entry too if your log and key are not in the default places. The server checks the credential when it starts and exits with the reason if it is missing, unknown or revoked, so a wrong setup shows at once. Run `hyalos mcp` by hand to check it: it reads one JSON-RPC message per line on stdin and answers on stdout until stdin ends.

## Over HTTP on loopback

```
hyalos mcp --listen 127.0.0.1:8765
```

This serves Streamable HTTP at `http://127.0.0.1:8765/mcp`. Only a loopback address is accepted. A request whose Host or Origin is not that address and port is refused with 403, so a web page cannot reach the server by rebinding a name to 127.0.0.1.

Each request needs `Authorization: Bearer <credential>`. Without a credential in force it is 401 with `WWW-Authenticate: Bearer`. Any process on the machine can reach the port, so the credential is what keeps it out. Turn HTTP on only while you need it.

## Conformance

CI runs the official MCP conformance suite against the protocol layer on every pull request (job `mcp conformance`). To run it locally:

```
npm ci --ignore-scripts --prefix crates/mcp-conformance
HYALOS_REQUIRE_MCP_CONFORMANCE=1 cargo test -p hyalos-mcp-conformance
```

`crates/mcp-conformance/expected-failures.yml` lists the features the server does not offer: completions, resources, prompts and input-required results. Every other scenario the revision requires must pass.
