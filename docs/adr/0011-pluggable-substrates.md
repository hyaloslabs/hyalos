# ADR-0011: Pluggable substrates; best isolation by default; attestation recorded

**Status:** Accepted on November 1, 2026, after review against ADR-0002, 0004 and 0016 and spike #72, with macOS left out of v1. The clarifications from that review are folded in below.

## Context

Isolation is becoming a cloud SKU, and hosts differ: some have KVM, some only containers, macOS has its own lightweight VMs.

## Decision

Sandboxes are a substrate interface. Drivers in order of preference: microVM (Firecracker or Cloud Hypervisor), gVisor, rootless container; hosted providers later. The engine picks the best available, records which one ran each job, and stores attestation evidence on the job's event when the substrate provides it. A sandbox has no network, only a socket to the broker ([ADR-0016](0016-agent-runtime-and-broker-protocols.md) describes the forwarder inside it).

**v1 runs on Linux hosts only.** WSL2 counts as Linux: its drivers are detected like any other host's, and a driver the host lacks is skipped. On any other host the engine refuses to start a job and says why, instead of running on a boundary nobody has tested.

**macOS, including Apple's per-container lightweight VMs, is out of scope for v1** and is not scheduled. The broker channel sits behind the substrate interface, so a later driver can bring its own transport, but nothing is built or tested for it. Two starting points are recorded for when it is taken up. The first runs the engine and broker inside the same Linux VM as the containers, so sockets and gVisor work as on Linux. The second gives the container an internal network whose only reachable endpoint is the broker, authenticated with a per-job token; that weakens "no network" to "one authenticated endpoint" and would have to be recorded on the job as its own, weaker tier. Whether Docker Desktop and similar can pass a host Unix socket into a container was never tested. Supporting macOS needs a new ADR.

## Consequences

Users on Linux get the strongest boundary their machine allows without configuring it. Each driver must pass the same contract suite. People on macOS or native Windows cannot run jobs in v1, which narrows the audience, and the install-to-first-job target is measured on Linux only. The interface is built for more drivers, but only Linux drivers are written and tested, so a driver for another operating system is new work that must pass the contract suite and be decided in its own ADR.
