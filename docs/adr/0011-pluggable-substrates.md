# ADR-0011: Pluggable substrates; best isolation by default; attestation recorded

**Status:** Proposed. Depends on [open question 4](../planning/open-questions.md) (macOS substrate maturity).

## Context

Isolation is becoming a cloud SKU, and hosts differ: some have KVM, some only containers, macOS has its own lightweight VMs.

## Decision

Sandboxes are a substrate interface. Drivers in order of preference: microVM (Firecracker or Cloud Hypervisor), gVisor, rootless container; on macOS, Apple's per-container lightweight VMs; hosted providers later. The engine picks the best available, records which one ran each job, and stores attestation evidence on the job's event when the substrate provides it. A sandbox has no network, only a socket to the broker.

## Consequences

Users get the strongest boundary their machine allows without configuring it. Each driver must pass the same contract suite.
