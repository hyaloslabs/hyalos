# ADR-0006: Plugins are Agent Skills and WASM components; MCP servers run contained

**Status:** Proposed

## Context

Agent Skills became an open standard with 40+ adopting clients and tens of thousands of public skills, many of which ship scripts that run with full agent permissions. WASI 0.3 (June 2026) made async WASM components practical, with Wasmtime 46 enabling it by default.

## Decision

A Hyalos plugin is an Agent Skill (SKILL.md) plus, optionally, a WASM component whose host imports are its capabilities, and a workflow. Skill scripts and third-party MCP servers run in sandboxes behind the broker. Hyalos itself ships as an Agent Skill plus an MCP server.

## Consequences

Hyalos runs the ecosystem people already have, safely, instead of asking for a new format. WASM components need WIT interfaces for host capabilities, versioned like the manifest.
