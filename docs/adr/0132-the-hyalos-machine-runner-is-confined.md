# ADR-0132: The hyalOS machine runner is confined

**Status:** Proposed

## Context

hyalOS's monitor runs every KVM machine in a separate process, the machine runner, which it starts with an empty environment and one socket as its channel (ADR-003 of [`docs/plans/hyalos-kernel-core`](../plans/hyalos-kernel-core/decisions/ADR-003-a-confined-runner-owns-the-machine.md)). The runner reads the first parse of every request from a kernel that a hostile guest may have taken over. Before this change nothing stopped code that took over the runner from doing whatever its user may: opening the operator's files, connecting to the network, starting a program or signalling another process. The holder, which starts it, keeps the chain and the tools that hold the path to the model and to GitHub, so a runner that can reach the host can reach those too.

v1's broker already refuses to run without Landlock ([ADR-0121](0121-no-broker-runs-without-landlock.md)). hyalOS is its own workspace ([ADR-0130](0130-hyalos-os-is-its-own-workspace.md)), so the plan pack's decision needs a repository record of what the runner may do once confined, and of what happens on a host that cannot confine it.

## Decision

**No guest instruction runs in a runner that is not confined by both Landlock and a seccomp allow-list.**

- **Order.** The runner closes every descriptor it inherited, opens `/dev/kvm`, and applies Landlock before any byte of the run arrives. It then reads the `Boot` frame, builds the machine, installs its seccomp filter, and only then runs the virtual CPU.
- **Landlock** handles every file right the kernel's Landlock has, with no rule granting any back, and where the kernel has them, TCP and the scopes for signals and abstract sockets. The first ABI's file rights are required; a kernel that cannot enforce them refuses. Descriptors opened before it, KVM and the channel, keep working.
- **Seccomp** is an allow-list built with the pure-Rust `seccompiler` crate (`=0.5.0`, JSON feature off, depending only on `libc`). Anything else kills the runner with `SIGSYS`. The list was measured, by tracing a runner from the moment its filter holds on every path the tests and the `attacks` command take, and one call a trace cannot see was added from reading the code:

  | System call | Restriction |
  | --- | --- |
  | `ioctl` | Only `KVM_RUN`, only on the virtual CPU |
  | `recvfrom` | Only the channel |
  | `sendto` | Only the channel, with no address |
  | `write` | Only standard error, only under `--trace` |
  | `mmap` | Only anonymous, never executable |
  | `munmap`, `brk` | None: memory for frames |
  | `clock_gettime` | Only `CLOCK_MONOTONIC`: the standard library's `Instant`, a system call only where the host's clock source has no vDSO path |
  | `close` | None: tearing the machine down |
  | `exit_group` | None |

  A new entry needs a reason that holds: none may reach a file, a network, another process or executable memory. A test pins the list.
- **No fallback.** Before a `live` run writes its intent link, the holder runs `machine-runner --check`, which opens KVM, applies both confinements and exits. A host without Landlock is refused with a message naming Landlock, and one whose filter does not install is refused too, with no chain link. A runner that then fails to confine itself sends only its refusal and never runs the machine, and the run ends in `KernelFault`, an existing reason, so no record changes shape. A runner killed by its filter also ends the run in `KernelFault`, with no further effect.
- **Tests.** A hostile runner and injected confinement failures exist only in a build with the monitor's off-by-default `test-switches` feature. CI's `hyalos-os` job runs them with `HYALOS_REQUIRE_LANDLOCK` and `HYALOS_REQUIRE_KVM` set, so they cannot skip.

## Consequences

- Code that takes over the runner reaches the machine and its one channel, and nothing else. The holder still treats every message as hostile.
- hyalOS needs Linux with KVM, Landlock and seccomp. A host without them runs no KVM run, and nothing offers a switch to run unconfined.
- The list depends on the kernel, the C library and the standard library's socket calls, so a host or toolchain that needs another system call fails closed until the call is reviewed and added.
- `landlock` (`=0.4.4`) and `seccompiler` are new dependencies of the monitor, accepted by `hyalos-os/deny.toml`, and belong in the review scope.
- A runner killed by its filter may leave a core dump wherever the host's `core_pattern` sends it, and the runner's memory holds the task's inputs.
