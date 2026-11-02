# ADR-0006: Plugins are Agent Skills and WASM components; MCP servers run contained

**Status:** Proposed. Local-install mode added in the November 1, 2026 planning session; folded in below.

## Context

Agent Skills became an open standard with 40+ adopting clients and tens of thousands of public skills, many of which ship scripts that run with full agent permissions. WASI 0.3 (June 2026) made async WASM components practical, with Wasmtime 46 enabling it by default.

## Decision

A Hyalos plugin is an Agent Skill (SKILL.md) plus, optionally, a WASM component whose host imports are its capabilities, and a workflow. Skill scripts and third-party MCP servers run in sandboxes behind the broker. Hyalos itself ships as an Agent Skill plus an MCP server.

**Local install.** Most public skills were never published through a Hyalos pipeline and have no capability manifest. These install in local mode: they start with **zero capabilities**, the operator sees what the skill asks for as a capability diff, and every capability granted needs a **human** approval ([ADR-0012](0012-humans-approve-only-irreversible-actions.md)). The install is recorded on the log as local, with no signature or transparency-log entry. Every update is re-diffed and any added capability needs a human again. Releases that did go through the pipeline keep the automated path in [ADR-0007](0007-capability-based-marketplace-trust.md).

## Consequences

Hyalos runs the ecosystem people already have, safely, instead of asking for a new format. The price is human attention for each local skill's first grants, so the capability diff has to be easy to read. WASM components need WIT interfaces for host capabilities, versioned like the manifest.
