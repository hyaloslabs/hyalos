# ADR-0087: Plugins run in the broker, with capabilities as host imports

**Status:** Accepted

## Context

[ADR-0006](0006-plugins-are-skills-and-wasm-components.md) makes a plugin an Agent Skill plus, optionally, a WASM component whose host imports are its capabilities. [ADR-0085](0085-the-capability-vocabulary.md) defines those capabilities. [ADR-0086](0086-the-plugin-runtime-s-crate-and-line-budget.md) puts the runtime in `hyalos-plugin`, with a budget of 3,000 lines, and leaves which process hosts a plugin to this story (S6.1, #36).

The `hyalos` binary runs as two processes ([ADR-0002](0002-one-binary-two-processes-sandboxes.md)):
- the engine, which holds the long-lived secrets: the manifest and log keys, and the GitHub App key;
- the broker, which holds live jobs' tokens and the provider keys, parses untrusted input, and enforces every capability a manifest grants.

ADR-0002 puts untrusted parsing in sandboxes or the broker, never the engine. A plugin's component is untrusted code. The operator chose the broker over the two alternatives:
- the engine, which would put plugin code beside its keys behind Wasmtime alone;
- a sandbox per plugin, which would pay a container start on every call.

## Decision

**A plugin runs in the broker process.** The broker already holds the gates a plugin's calls must pass. A Wasmtime escape would reach what a broker compromise reaches: live jobs' tokens and the capped provider key, never the engine's keys. This ADR builds the runtime and its contract. Wiring it into the broker comes with install (S6.2), when there are plugins to load.

The process is not the crate. The broker process runs code from the `hyalos` crate as well as the `hyalos-broker` crate, and the runtime is reached only from the `hyalos` crate's broker side. So ADR-0086's rule stands: the `hyalos-broker` crate never links a WASM host.

**One instance serves one job.** An instance keeps its memory and globals between calls, so the broker makes a fresh one for each job, with that job's grant and a `Host` built for that job. Nothing a plugin holds carries from one job to the next.

**The runtime is Wasmtime, pinned exactly (`=50.0.1`), with components only and no default features.** It provides no WASI: no clock, no files, no sockets, no environment, no randomness. A plugin's limits bound what it can cost the broker, and passing one stops it:

| Limit | Value | Past it |
| --- | --- | --- |
| Memory, across all the plugin's memories | 64 MiB | the grow traps |
| Core instances, memories and tables, each | 16 | instantiation fails |
| Elements in one table | 10,000 | the grow traps |
| Fuel, for instantiation and for each run | 2,000,000,000 units, about a second of work | `plugin.out_of_fuel` |
| Bytes the broker reads for one host call's arguments | 1 MiB | the call traps |
| Host calls in one run | 256 | the call gets `plugin.too_many_calls`, and the `Host` is not asked |

Fuel counts the plugin's own instructions, not the broker's work inside a host call. The argument and call caps bound that work.

**Capabilities are host imports in `hyalos:capability@1.0.0`.** There is one interface per field a plugin can call:

| Interface | Function | Field |
| --- | --- | --- |
| `egress` | `get: func(host: string, path: string) -> result<list<u8>, string>` | `egress_hosts` |
| `tools` | `call: func(name: string, input: string) -> result<string, string>` | `tools` |
| `forge` | `act: func(action: string, input: string) -> result<string, string>` | `actions` |

`data_flows` has no interface. A flow names where data may go. The broker's `Host`, built for one job, holds that job's manifest and applies its flows to what these calls send, as the broker does for a sandbox ([ADR-0055](0055-data-flows-name-where-the-jobs-data-may-go.md)). There is no model interface in this version, so a plugin cannot call a model.

A plugin exports `hyalos:plugin/run@1.0.0`, with `run: func(input: string) -> result<string, string>`.

**Only what the grant covers is linked.** The runtime links an interface only when the plugin's grant holds at least one name in its field. Before instantiating, it compares every import of the component with what it linked. An import of an interface the grant does not cover, of anything outside `hyalos:capability`, or of another version, is refused with `plugin.undeclared_import`, and the component never instantiates. An import with the right name and another type is `plugin.bad_import`. So a component cannot reach a capability it was not granted, even to be refused.

**Every call is checked against the grant's names.** A call names what it acts on: a host, a tool or an action. A name the grant does not hold gets `capability.not_granted` back as the call's error, and the broker's `Host` is never asked. A granted name goes to the `Host`, which is the broker's own gates, so the job's manifest still bounds it ([ADR-0004](0004-one-signed-manifest-per-job.md)).

**What a plugin returns is plugin-authored text.** `run`'s answer, success or error, comes back as `PluginText` carrying the plugin's grant. So wherever it goes, it bounds the job it steers ([ADR-0066](0066-plugin-authored-text-and-the-grant-that-bounds-it.md)).

**Failures are reason codes, never Wasmtime's text.** A trap or compile error can quote names the plugin wrote. The codes are `plugin.invalid`, `plugin.undeclared_import`, `plugin.bad_import`, `plugin.no_entry`, `plugin.out_of_fuel`, `plugin.trapped`, `plugin.too_many_calls` and `plugin.runtime`.

## Consequences

A component's reach is decided at link time, from its grant. A plugin that asks for nothing gets nothing it can call, and a component built against an interface it was not granted does not load. The per-call check means a plugin granted one host cannot use another, whatever its code says.

The broker's attack surface grows by a WASM engine. That is the cost the operator accepted for one process and no container per call. Wasmtime is pinned, so an upgrade is a reviewed change, and `cargo deny` reports its advisories. Building it adds about 40 seconds to a clean build, and three duplicate crate versions that `cargo deny` warns about but allows.

The three interfaces are small on purpose. A plugin needing more, such as a model call, a clock or files, needs a new interface and an ADR, as a new capability field does ([ADR-0085](0085-the-capability-vocabulary.md)). The limits are constants for now; making them policy is a later decision.

The `Host` methods take a plugin's arguments as plain strings. They are plugin-authored, and the broker's implementation (S6.2) must not echo or log them. Giving them a tainted type is that story's to decide, when there is an implementation to hold to it.

Tests (`core/plugin/tests/host.rs`):
- A component importing a capability it was not granted fails to instantiate; granted it, it instantiates.
- Imports outside `hyalos:capability`, of another version (a semver-compatible one included), or of the wrong type are refused; a component with no entry cannot run.
- A call reaches the host only for a granted name, for egress and for tools; another name gets `capability.not_granted`, and the host never hears of it.
- A run's host calls stop at the cap, and the count starts again with each run; an argument past its cap traps before the host is asked.
- Memory is bounded across all a plugin's memories: one memory within the cap grows, two that together pass it trap. Tables and instances are bounded too.
- A plugin that never stops runs out of fuel.
- Only a component is loaded.
