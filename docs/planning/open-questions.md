# Open questions

The November 1, 2026 planning session settled the original four questions and spikes #72 and #73 settled the forwarder and offline Sigstore ones; the answers are in ADR-0002, 0005, 0007, 0011, 0012 and 0016. Two features were deferred instead of settled, and neither blocks anything: macOS hosts (ADR-0011, issue #71) and signing in with a chat subscription (ADR-0016, issue #75). What remains is a fact we cannot settle by deciding. When it is answered, fold the answer into a new ADR or the PRD, then delete the entry here.

1. **Client support for MCP extensions ([ADR-0010](../adr/0010-mcp-primary-interface-two-principals.md)):** do Claude Code and Hermes advertise the Tasks and MCP Apps extensions? The ADR already serves clients without them, so the answer changes the M2 demo plan, not the ADR. Record it in the PRD's M2 milestone note; a new ADR supersedes ADR-0010 only if the result invalidates the fallback. Settled by a spike in E5.
