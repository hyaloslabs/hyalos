# ADR-001: The kernel runs in ring 3

**Status:** accepted (developer, 2026-11-23)

**Proposed by:** author (spec); accepted by the developer at the spec ADR checkpoint

## Context

The parent's brief suggested setting `CR4.TSD` to take the time counter away from a taken-over kernel. A measurement on 2026-11-23 (`context.md` §2) showed that this does nothing while the kernel runs in ring 0: `RDTSC` still runs, and the kernel can clear the bit itself. Ring 0 also lets a taken-over kernel load its own page tables and undo the memory protection the monitor built (`hyalos-os/monitor/src/kvm.rs:144`).

## Options

1. **Keep ring 0 and set `CR4.TSD`.** Rejected: measured to have no effect.
2. **Keep ring 0 and rely on KVM to intercept the time counter.** Rejected: the monitor has no KVM control to make `RDTSC` exit, and a ring 0 kernel could still change its page tables.
3. **Put a small ring 0 stub under an unprivileged kernel.** Rejected: it adds privileged code to review and handlers to get right, and the kernel needs none. A fault can simply end the machine.
4. **Start the whole kernel in ring 3, with `CR4.TSD` set, user-accessible pages and no ring 0 code.** Chosen.

## Decision

The monitor starts the kernel image directly in 64-bit ring 3 with `CR4.TSD` set. The machine has no ring 0 code and no fault handlers, so any fault ends in a shutdown, which the monitor reports as `KernelFault`. The kernel leaves through its existing port, allowed by `IOPL` 3, and the monitor still refuses every other port.

## Consequences

- A taken-over kernel loses the clock, control registers, descriptor tables, model-specific registers and its page tables.
- `IOPL` 3 lets the kernel use any port, and each use exits to the monitor, which accepts only port `0x1000`.
- `HLT` faults in ring 3. The monitor already treats a shutdown as `KernelFault`.
- Ring 3 in long mode with no handlers is measured only in 32-bit protected mode so far. The first machine slice must prove it, and stops if a committed guest's transcript changes.
