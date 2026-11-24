# ADR-002: Report a reachable RDSEED and allow runs

**Status:** accepted; amended by ADR-009, adds `RDRAND` and `RDPID` where KVM cannot trap them

**Proposed by:** developer

## Context

On the development host (Intel, WSL2, nested KVM), `RDSEED` returned changing values from ring 3 after its CPUID bit was cleared (`context.md` §2). `RDRAND`, `RDTSCP` and `RDPID` fault under the same setup. The cause is not explained, and it may be specific to nested hosts. Replay compares every run with its record, so randomness that changes a result is refused.

## Options

1. **Refuse any host where `RDSEED` is reachable.** Rejected: it would stop KVM runs on the development host and possibly other nested hosts, while replay already guards the result.
2. **Warn now, and add a setting that requires a blocked host.** Rejected for this pack: one more option to build and test, with no operator asking for it yet.
3. **Report it through a host probe, state the limit, and allow runs.** Chosen.

## Decision

The monitor hides `RDSEED` in CPUID. A host probe reports each instruction as `blocked` or `reachable`. A KVM run is not refused because `RDSEED` is reachable, and the operator documentation and review package state the limit.

## Consequences

- hyalOS must not claim that a taken-over kernel has no hardware randomness on every host.
- A taken-over kernel on such a host can obtain unpredictable values. It still cannot change a verified result without replay refusing it.
- The host probe must be run on bare-metal KVM to learn how far this limit reaches.
