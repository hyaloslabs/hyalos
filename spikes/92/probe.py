#!/usr/bin/env python3
"""A probe MCP server on stdio: logs every message a client sends and answers enough of both the
2025-11-25 (initialize) and 2026-07-28 (stateless, _meta) revisions that the client goes on to list
and call tools. It offers one tool, `probe_echo`, that also declares task support and an MCP Apps UI
resource, so a client that supports either shows it in what it sends."""
import json
import os
import sys

LOG = os.environ.get("PROBE_LOG", "probe.log")


def log(direction, message):
    with open(LOG, "a") as f:
        f.write(json.dumps({"dir": direction, "msg": message}) + "\n")


TOOL = {
    "name": "probe_echo",
    "title": "Probe echo",
    "description": "Echoes its text argument. Call it once with text 'hello'.",
    "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]},
    "execution": {"taskSupport": "optional"},
    "_meta": {"ui": {"resourceUri": "ui://probe/view.html"}, "ui/resourceUri": "ui://probe/view.html"},
}


def reply(id_, result=None, error=None):
    out = {"jsonrpc": "2.0", "id": id_}
    if error is not None:
        out["error"] = error
    else:
        out["result"] = result
    log("out", out)
    sys.stdout.write(json.dumps(out) + "\n")
    sys.stdout.flush()


for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        msg = json.loads(line)
    except ValueError:
        log("in-raw", line)
        continue
    log("in", msg)
    method, id_ = msg.get("method"), msg.get("id")
    if id_ is None:
        continue
    if method == "initialize":
        asked = msg.get("params", {}).get("protocolVersion", "2025-11-25")
        reply(id_, {
            "protocolVersion": asked,
            "capabilities": {
                "tools": {},
                "resources": {},
                "tasks": {"requests": {"tools": {"call": {}}}},
                "extensions": {"io.modelcontextprotocol/tasks": {}, "io.modelcontextprotocol/ui": {}},
            },
            "serverInfo": {"name": "probe", "version": "0"},
        })
    elif method == "server/discover":
        reply(id_, {
            "supportedVersions": ["2026-07-28", "2025-11-25"],
            "capabilities": {"tools": {}, "resources": {}, "extensions": {"io.modelcontextprotocol/tasks": {}, "io.modelcontextprotocol/ui": {}}},
            "serverInfo": {"name": "probe", "version": "0"},
        })
    elif method == "tools/list":
        reply(id_, {"tools": [TOOL]})
    elif method == "tools/call":
        text = msg.get("params", {}).get("arguments", {}).get("text", "")
        reply(id_, {"content": [{"type": "text", "text": f"echo: {text}"}], "isError": False})
    elif method == "resources/read":
        reply(id_, {"contents": [{"uri": "ui://probe/view.html", "mimeType": "text/html;profile=mcp-app", "text": "<p>probe</p>"}]})
    elif method in ("resources/list", "prompts/list", "resources/templates/list"):
        key = {"resources/list": "resources", "prompts/list": "prompts", "resources/templates/list": "resourceTemplates"}[method]
        reply(id_, {key: []})
    elif method == "ping":
        reply(id_, {})
    else:
        reply(id_, error={"code": -32601, "message": f"probe: {method} not offered"})
