# Spike #92: test rig

Throwaway tooling that produced the evidence in [`docs/planning/spike-92-findings.md`](../../docs/planning/spike-92-findings.md). It is **not product code**: it is not part of the Cargo workspace, not trusted core, and nothing depends on it.

## What it does

- **`probe.py`:** an MCP server on stdio that logs every message to `$PROBE_LOG` (default `probe.log`). It answers the 2025-11-25 `initialize` handshake and the 2026-07-28 `server/discover`, and offers one tool, `probe_echo`, that declares task support and an MCP Apps UI resource.
- **`sdk_initialize.py`:** the Python MCP SDK client as Hermes Agent uses it (`stdio_client`, `ClientSession`, `initialize()`, `list_tools()`), against `hyalos mcp`.
- **`sdk_discover.py`:** the same with the SDK's `discover()`, for the 2026-07-28 revision.

## Running it

Claude Code against the probe, without touching your own MCP settings:

```
cat > mcp.json <<EOF
{"mcpServers": {"probe": {"type": "stdio", "command": "python3", "args": ["$PWD/probe.py"], "env": {"PROBE_LOG": "$PWD/cc.log"}}}}
EOF
claude -p "Call the probe_echo tool once with text hello." --mcp-config mcp.json --strict-mcp-config --allowedTools mcp__probe__probe_echo < /dev/null
```

`cc.log` then holds each request with the client's `_meta` (its revision and declared capabilities).

The SDK clients need a throwaway Hyalos world:
- `$SPIKE/c` and `$SPIKE/d`, each mode 0700, with `HYALOS_CONFIG_DIR`, `HYALOS_DATA_DIR` and `HYALOS_RUNTIME_DIR` pointing into `$SPIKE`;
- `hyalos init`, `hyalos operator init`, then `hyalos agent issue spike`;
- the printed credential written to `$SPIKE/cred`.

Then, in a virtualenv with `mcp==1.26.0` (Hermes 0.19.0's pin) or `mcp==2.3.0`:

```
SPIKE=$SPIKE python -I sdk_initialize.py path/to/hyalos
SPIKE=$SPIKE python -I sdk_discover.py path/to/hyalos   # mcp 2.x only
```
