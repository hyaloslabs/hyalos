# ADR-003: A confined runner owns the machine

**Status:** accepted (developer, 2026-11-23)

**Proposed by:** author (spec); accepted by the developer at the spec ADR checkpoint

## Context

One monitor process owns the KVM machine, parses requests from a possibly hostile kernel, writes the operator's chain and starts the attestation tool and `gh` (`hyalos-os/monitor/src/live.rs:176`, `:202`). Those tools need the network and credentials, so that process cannot simply be locked down. v1's broker already confines itself with Landlock and refuses to run without it (`core/hyalos/src/confine.rs:67`, ADR-0121).

## Options

1. **Confine the whole monitor.** Rejected: a filter loose enough for the tools, the network and the chain leaves a compromised request handler nearly everything.
2. **Run the machine in a container or a separate service.** Rejected: it adds a runtime dependency to a program meant to be small and adds no boundary a child process lacks.
3. **Use `libseccomp`.** Rejected: it brings C code into the trusted base.
4. **Split into a holder and a machine runner, confined with Landlock and a seccomp allow-list built with the pure-Rust `seccompiler` crate.** Chosen.

## Decision

The holder stays unconfined and performs model calls, the effect and the chain. It starts one machine runner per KVM run. The runner opens KVM, gives up every file right with Landlock, installs a seccomp allow-list, and only then starts the virtual CPU. The two speak over one socket pair with a bounded, versioned message format. If either confinement cannot be applied, the run is refused.

## Consequences

- A bug in the request handler reaches the machine and the channel, and nothing else.
- The holder must treat every message as hostile, including the single-effect rule.
- `seccompiler` is a new dependency in `hyalos-os/deny.toml` and in the review scope. It depends only on `libc` when its optional JSON feature is off.
- The allow-list depends on the kernel and C library, so a host that needs another system call fails closed until it is reviewed and added.
