# ADR-0086: The plugin runtime's crate and line budget

**Status:** Proposed

## Context

The trusted core has a budget of 30,000 lines ([ADR-0017](0017-workspace-layout-and-trusted-core-boundary.md)). At the end of M2 it stood at 23,740 lines (79.1%), and `core/hyalos` alone at 9,875. Splitting `run_cmd.rs` (#269) and the capability vocabulary ([ADR-0085](0085-the-capability-vocabulary.md)) add about 255 more. That leaves about 6,000 lines for M3.

M3 has to fit the following into those lines:
- the plugin runtime:
  - hosting WASM components in Wasmtime (S6.1);
  - installing skills and plugins (S6.2, S6.7);
  - verifying a release's Sigstore bundle (S7.12, [ADR-0007](0007-capability-based-marketplace-trust.md));
  - update checks (S7.10);
- everything else in M3:
  - running a third-party MCP server behind the broker (S6.3);
  - showing the capability diff, which ADR-0085 computes, before an install or update (S6.4);
  - composing workflows (S6.5);
  - install policy and revocation (S7.5, S7.7);
  - releasing an issue by policy (S4.9);
  - macOS VMs (S2.5).

Only two parts of the core have budgets of their own: the model protocols and MCP. Without one, the plugin runtime would find out it does not fit only when the whole core does, after most of it is written. The external review of November 9, 2026 (#289) and S6.1 (#36) both ask for a budget before its code lands.

## Decision

**The plugin runtime is its own crate, `hyalos-plugin`, in `core/plugin`.** It holds the Wasmtime host and the host imports that are a plugin's capabilities, install, Sigstore verification and update checks. So one path counts them all. It is linked into the `hyalos` binary, so it is trusted core ([ADR-0017](0017-workspace-layout-and-trusted-core-boundary.md)).

**Its dependency edges are in the layout table now.** It may depend only on `hyalos-types` among the workspace crates. Only the `hyalos` crate may depend on it:
- the `hyalos-broker` crate, which parses untrusted input, never links a plugin host;
- the `hyalos-engine` crate, which is deterministic ([ADR-0013](0013-deterministic-simulation-and-evidence-gated-releases.md)), never links Wasmtime.

**Which process hosts a plugin is not decided here.** The `hyalos` binary runs as both the engine and the broker ([ADR-0002](0002-one-binary-two-processes-sandboxes.md)), so these edges say which crates may call the runtime, not where it runs. ADR-0002 puts untrusted parsing in sandboxes or the broker, and a plugin's WASM and a downloaded Sigstore bundle are untrusted input. S6.1's ADR chooses the process and squares it with ADR-0002, whether by the Wasmtime sandbox as the boundary or by hosting in the broker or a sandbox. A crate boundary works for any of these.

The crate starts empty, so `cargo xtask layout` enforces these edges from the first line written. An xtask test checks that the broker and the engine are refused it, and that it is refused the engine.

**Its budget is 3,000 lines** (`plugin/src` in `PART_BUDGETS`), and `cargo xtask lines` reports it from today (`0 / 3000`). The number is half of what M3 has left. The other half, about 3,000 lines, covers the rest of M3: the broker's containment of MCP servers, the commands that show a diff and install, workflow composition, install policy, and the operator's commands. Wasmtime, Sigstore's library and their dependencies are third-party code and not counted ([ADR-0017](0017-workspace-layout-and-trusted-core-boundary.md)); `cargo deny` and review cover them.

**If M3 does not fit, in this order:**
1. Cut within the part, which is where most of the slack is. A capability host import is the smallest code that enforces it, and verification is one pinned library's call ([ADR-0007](0007-capability-based-marketplace-trust.md)), not a reimplementation.
2. Move `hyalos doctor` (about 130 counted lines) into a separate program outside `core/`, since it holds no secret and decides nothing. While it is in the `hyalos` binary it runs in that process and counts. This frees little and adds a second program beside the one binary of [ADR-0002](0002-one-binary-two-processes-sandboxes.md), so it is worth it only to close a small gap. The thin CLI (`tools_cmd`, about 180) starts jobs through the engine in its own process, so at most its formatting could move. `setup_ask.rs` and `app_manifest.rs` stay, because they handle the App's key and the setup page's code.
3. Otherwise raise the cap, by an ADR that says what the extra lines buy. An overrun of more than a few hundred lines ends here.

Not on the list:
- Removing the by-hand GitHub App path from `hyalos setup`: [ADR-0084](0084-setup-creates-the-github-app-from-a-manifest.md) keeps it for GitHub Enterprise Server and for when the setup page is down, so only an ADR superseding that could.
- Splitting `run_cmd.rs` (#269): it makes the file easier to review but saves no lines.

Changing the part's number, like changing the cap, is a reviewed change to `crates/xtask` with a reason, as the model protocols' and MCP's budgets were.

## Consequences

The runtime's size is visible on every pull request from its first line, and the stories that build it share one number. A story that would push the part over its budget fails CI. It must then cut, or make the case for more.

A new core crate adds a build unit and a `Cargo.toml` to review. In return, the trust boundary of the most exposed code in M3 is checked by the build. The broker's crate, which parses untrusted input, and the engine's, which must stay deterministic, cannot link a WASM host by accident. Where a plugin runs is still S6.1's to decide against ADR-0002.

S6.1 (#36) starts with its budget in place, and the box it lists for that is met by this decision.

Tests: `crates/xtask`, `only_the_binary_may_depend_on_the_plugin_runtime`; `cargo xtask lines` reports `core/plugin/src`.
