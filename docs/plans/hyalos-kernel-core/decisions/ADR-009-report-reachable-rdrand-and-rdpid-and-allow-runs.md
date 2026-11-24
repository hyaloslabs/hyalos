# ADR-009: Report reachable RDRAND and RDPID, and allow runs

**Status:** accepted (developer, 2026-11-24)

**Proposed by:** executor (Slice 3, blocked on CI's KVM host); chosen by the developer on PR #478

## Context

This amends ADR-002, which treats only `RDSEED` as host-dependent. S-03.2 says `RDRAND` and `RDPID` fault once their CPUID bits are clear, which held on the development host (Intel, WSL2, nested KVM; `context.md` §2). On CI's `ubuntu-24.04` runner, Slice 3's tests saw the allowed CPUID list with both bits clear, yet both instructions ran in ring 3 and reached the unknown request (`BadHypercall`), not `KernelFault`.

KVM can make them fault only through its Intel (VMX) backend: it sets `RDRAND` exiting and clears the `RDTSCP` enable, which also covers `RDPID`, when a guest's CPUID lacks them. AMD's SVM has no intercept for either, so on an AMD host they run whatever CPUID says. Replay compares every run with its record, so a value that changes a result is still refused.

## Options

1. **Treat `RDRAND` and `RDPID` as host-dependent, like `RDSEED`.** Chosen.
2. **Refuse KVM runs on hosts that cannot trap them.** Rejected: it would refuse every AMD host, CI's runner included, while replay already guards the result.

## Decision

The monitor still hides `RDRAND` and `RDPID` in CPUID (S-03.1). Whether a host traps them is host-dependent: the host probe (S-03.3) reports each as `blocked` or `reachable`, a KVM run is not refused because either is reachable, and the operator documentation and review package state the limit beside `RDSEED`'s.

S-03.2 and its examples are shown where the host's KVM traps the instructions: the tests require `KernelFault` on an Intel CPU with `kvm_intel` loaded. On AMD with `kvm_amd` they accept that the instructions run, and on any other host they cannot tell, so they accept either ending and do not claim the fault was shown. Any other ending fails everywhere. Each test writes the host's vendor, brand and KVM module to its output, so a log shows which host it ran on.

## Consequences

- hyalOS must not claim that a taken-over kernel cannot run `RDRAND` or `RDPID` on every host. On AMD it can obtain unpredictable values and the processor ID, though it still cannot change a verified result without replay refusing it.
- The processor ID `RDPID` returns may differ between runs or hosts. Like any value the kernel reads outside its record, it cannot change a verified result without replay refusing it.
- Slice 4's host probe reports `RDRAND` and `RDPID` from a measurement, alongside `RDSEED`, and Slice 12's package states this limit.
- The vendor check is a prediction, not a measurement: a nested Intel host whose outer hypervisor offers no `RDRAND` exiting would fail the tests until the host probe's measurement replaces it.
