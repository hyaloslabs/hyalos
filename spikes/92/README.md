# Spike #92: test rig

Throwaway tooling that produced the evidence in [`docs/planning/spike-92-findings.md`](../../docs/planning/spike-92-findings.md). It is **not product code**: it is not part of the Cargo workspace, not trusted core, and nothing depends on it.

## What it does

- **`probe.py`:** an MCP server on stdio that logs every message to `$PROBE_LOG` (default `probe.log`). It answers the 2025-11-25 `initialize` handshake and the 2026-07-28 `server/discover`, and offers one tool, `probe_echo`, that declares task support and an MCP Apps UI resource.
- **`sdk_initialize.py`:** the Python MCP SDK client as Hermes Agent uses it (`stdio_client`, `ClientSession`, `initialize()`, `list_tools()`), against `hyalos mcp`.
- **`sdk_discover.py`:** the same with the SDK's `discover()`, for the 2026-07-28 revision.

## Running it

Run everything from this directory (`cd spikes/92`), with `hyalos` built by `cargo build` at the repository root.

### Claude Code against the probe

This does not touch your own MCP settings:

```
cat > mcp.json <<EOF
{"mcpServers": {"probe": {"type": "stdio", "command": "python3", "args": ["$PWD/probe.py"], "env": {"PROBE_LOG": "$PWD/cc.log"}}}}
EOF
claude -p "Call the probe_echo tool once with text hello." --mcp-config mcp.json --strict-mcp-config --allowedTools mcp__probe__probe_echo < /dev/null
```

`cc.log` then holds each request with the client's `_meta`: its revision and declared capabilities.

### A throwaway Hyalos world

```
export SPIKE=$(mktemp -d) HYALOS=$(realpath ../../target/debug/hyalos)
mkdir -m 0700 $SPIKE/c $SPIKE/d $SPIKE/r
export HYALOS_CONFIG_DIR=$SPIKE/c HYALOS_DATA_DIR=$SPIKE/d HYALOS_RUNTIME_DIR=$SPIKE/r
PASS='a throwaway spike passphrase'
$HYALOS init
printf '%s\n%s\n' "$PASS" "$PASS" | $HYALOS operator init --passphrase-stdin
printf '%s\n' "$PASS" | $HYALOS agent issue spike --passphrase-stdin | grep -o 'hyalos-agent-[0-9a-f]*' > $SPIKE/cred
```

The scripts below read `$SPIKE/c`, `$SPIKE/d`, `$SPIKE/r` and `$SPIKE/cred` themselves.

### Claude Code against `hyalos mcp`

```
cat > hyalos-mcp.json <<EOF
{"mcpServers": {"hyalos": {"type": "stdio", "command": "$HYALOS", "args": ["mcp"], "env": {"HYALOS_CONFIG_DIR": "$SPIKE/c", "HYALOS_DATA_DIR": "$SPIKE/d", "HYALOS_RUNTIME_DIR": "$SPIKE/r", "HYALOS_AGENT_CREDENTIAL": "$(cat $SPIKE/cred)"}}}}
EOF
claude -p 'Use the hyalos MCP server: call list_tasks once, then job_status with task "1", and report exactly what each returned.' --mcp-config hyalos-mcp.json --strict-mcp-config --allowedTools "mcp__hyalos__list_tasks mcp__hyalos__job_status" < /dev/null
```

In a fresh world the answers are an empty list and a "there is no such task" tool error. That shows the plain call path and the credential working, not a task being read.

### The Python SDK clients

In a virtualenv with `mcp==1.26.0` (Hermes 0.19.0's pin) or `mcp==2.3.0`:

```
python -I sdk_initialize.py $HYALOS
python -I sdk_discover.py $HYALOS   # mcp 2.x only
```

Delete `$SPIKE`, `mcp.json`, `hyalos-mcp.json` and `cc.log` afterwards.
