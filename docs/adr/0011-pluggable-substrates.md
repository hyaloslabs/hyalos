# ADR-0011: Pluggable substrates; best isolation by default; attestation recorded

**Status:** Proposed. The original open question on the macOS substrate was settled in the November 1, 2026 planning session; folded in below. The broker channel on macOS needs a spike to confirm.

## Context

Isolation is becoming a cloud SKU, and hosts differ: some have KVM, some only containers, macOS has its own lightweight VMs.

## Decision

Sandboxes are a substrate interface. Drivers in order of preference: microVM (Firecracker or Cloud Hypervisor), gVisor, rootless container; on macOS, Apple's per-container lightweight VMs; hosted providers later. The engine picks the best available, records which one ran each job, and stores attestation evidence on the job's event when the substrate provides it. A sandbox has no network, only a socket to the broker.

**macOS.** v1 starts on containers running inside a Linux VM (Docker Desktop or similar), so the host is already behind a VM boundary. Apple's lightweight VMs (S2.5) move to M3, where their maturity is reviewed. macOS is recorded on the job event as its own tier so nobody assumes Linux-level guarantees.

**macOS broker channel.** A container VM may not pass an arbitrary host Unix socket through. If a spike confirms that, M1 uses a network endpoint: the container gets an internal network whose only reachable endpoint is the broker, authenticated with a per-job token. On macOS "no network" then weakens to "one authenticated endpoint," and the tier label says so. The longer-term option is running the engine and broker inside the same Linux VM as the containers, so sockets and gVisor work as on Linux.

## Consequences

Users get the strongest boundary their machine allows without configuring it. Each driver must pass the same contract suite. macOS users get a weaker, clearly labelled guarantee until the in-VM option or Apple's VMs land.
