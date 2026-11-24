# PRD — hyalOS: a kernel and monitor ready for independent review

## Problem

hyalOS's containment claim rests on a kernel and monitor that are still a spike:

1. **A taken-over kernel keeps full CPU privilege.** It runs in ring 0, so it can read the time counter and change its own control registers and page tables. Setting `CR4.TSD` alone does not stop this (measured, `context.md` §2).
2. **It can reach hardware randomness.** The monitor passes the host's full CPUID list. On the development host, `RDSEED` still returned changing values after its CPUID bit was cleared and the code dropped to ring 3.
3. **The monitor is not confined.** One process owns the KVM machine, the operator's chain and the tools that hold the path to the model and the notification channel. A bug in its request handling would have all of that.
4. **The guest interface is not fixed.** It is a short document, and no test stops it from drifting.
5. **The code is not ready to review.** It has no line budget or `unsafe` rules, carries lint debt, and has test-only behaviour reachable in a normal build.
6. **Most of the trusted code is not ours.** The pinned `wasmi 2.0.2` has no public audit we could find, one search found no published code audit of TLSNotary, and no review scope says who covers them (#412).

## Users and jobs

- **Operator:** runs hyalOS tasks on their own machine and answers for what they do.
- **Independent reviewer:** a security engineer who did not write this code.
- **Capsules author:** builds the task contract on the guest interface.

**Jobs.**

- Operator: "When a guest exploits the interpreter, I want the damage to stop at the machine, so I can run tasks on input I do not trust."
- Independent reviewer: "When I am handed hyalOS, I want its scope, threats, limits and build steps in one place, so I can spend my time on the code."
- Capsules author: "When I build on the guest interface, I want it to be versioned and tested, so my work does not break silently."

## Goals

- G1. The guest interface is named, versioned and held by tests on both backends.
- G2. A taken-over kernel has no CPU privilege and no clock.
- G3. Hardware randomness is closed where the host allows it, and measured and stated where it does not.
- G4. A compromise of the code that owns the machine reaches no credential, file, network or program.
- G5. hyalOS's own code meets stated rules for size, `unsafe`, lint and test-only behaviour.
- G6. Every third-party component the security claim depends on is listed as covered or excluded, with a reason, and an upstream advisory reaches us.
- G7. An independent reviewer can start from one package.

## Non-goals

- The task contract, connectors, or any new guest call.
- Python guests (#438).
- Changing the execution profile or the record format. A change to either belongs to its own version.
- Binding the kernel image's hash into the record.
- Evaluating seL4 (parked by parent ADR-002).
- Hosts other than x86-64 Linux with KVM.
- v1's trusted-core review, which #63 keeps.
- Joining v1's engine.

## Success measures

- M1. Every hostile-kernel probe in S-02 and S-03 ends in a fault or is reported by the host probe.
- M2. Every committed guest gives byte-identical transcripts before and after the kernel drops privilege.
- M3. The machine runner cannot open a file, connect a socket or start a program, shown by a hostile test.
- M4. The review scope lists every component in S-06.1, and CI fails when a listed component's version changes without its row changing.
- M5. A person with no project context can build and run the review package's commands from a clean checkout.

## Non-functional requirements

| Quality | Target |
|---|---|
| Performance | The KVM backend's median cold start (boot to first call, as `HYALOS_TRACE` reports it, in a release-profile build over 21 runs on one host, with the `test-switches` feature once Slice 8 gates `HYALOS_TRACE`) is at most 20% slower than the baseline measured in the first slice that changes the machine |
| Reliability | A confinement or probe failure refuses the run. It never falls back to an unconfined one |
| Security | Fail closed on every check. The machine runner holds no secret and no descriptor except its KVM handles and its one channel |
| Privacy | none: this work adds no new data and no new place that stores it |
| Accessibility | none: command-line and documentation only |
| Observability | The host probe prints one line per instruction. No secret or task input reaches an error message |
| Compatibility / portability | x86-64 Linux with KVM, Landlock and seccomp. Existing records still audit and replay unchanged |
| Maintainability | hyalOS's own code stays within the budget set by S-05.1, and every `unsafe` block carries a `SAFETY` comment |

## Assumptions

| Assumption | How to validate | If false |
|---|---|---|
| The kernel image can run in long-mode ring 3 with no fault handlers | Passed in a throwaway build on 2026-11-23 (`context.md` §2). The first machine slice repeats it | Stop. Ring 3 needs a design change |
| `RDSEED` cannot be blocked from userspace on some hosts | Run the host probe on bare-metal KVM and on nested hosts | If it is blocked everywhere but WSL2, the stated limit narrows to named hosts |
| The monitor's KVM loop works under a seccomp allow-list | The confinement slice runs every committed guest under it | Stop, and record the system call needed |
| A separate review firm or person can be engaged | The developer decides | The pack ends at a ready package either way |
| Nothing depends on the kernel image's bytes staying the same | No record hashes it (`context.md` §1) | Stop before changing the image |

## Story map

| Fix the interface | Remove privilege | Close randomness | Confine the monitor | Make it reviewable | Scope third parties | Hand off |
|---|---|---|---|---|---|---|
| S-01 | S-02 | S-03 | S-04 | S-05 | S-06 | S-07 |

## Stories and acceptance criteria

### S-01 The guest interface is fixed

As a capsules author, I want a named, tested guest interface, so that my work does not break silently.

- **S-01.1** Given the interface document names `hyalos.abi/1`, when the conformance suite runs on both backends, then every call's success and error results match the document and the two backends give byte-identical transcripts.
- **S-01.2** Given a change to a call's name, signature, limit or error value, when the tests run, then they fail until the interface version and document change with it.
- **S-01.3** Given a guest with an import, export or feature outside the interface, when either backend loads it, then both refuse it with the same reason.

### S-02 A taken-over kernel has no privilege or clock

As an operator, I want a taken-over kernel to hold no CPU privilege, so that it cannot change its machine or read the time.

- **S-02.1** Given any committed guest, when it runs on the unprivileged kernel, then its transcript is byte-identical to the one from the privileged kernel at the base commit.
- **S-02.2** Given code in the kernel, when it runs `RDTSC` or `RDTSCP`, then the machine faults and the run ends with `KernelFault`.
- **S-02.3** Given code in the kernel, when it writes a control register, loads a descriptor table, writes a model-specific register or halts, then the machine faults and the run ends with `KernelFault`.
- **S-02.4** Given the memory probes the self-test kernel already holds, when they run on the unprivileged kernel, then each still ends in a fault.

### S-03 Hardware randomness is closed or stated

As an operator, I want to know whether my host lets a taken-over kernel reach hardware randomness, so that I do not rely on a property my machine lacks.

- **S-03.1** Given a machine, when the kernel reads CPUID, then it sees only an allowed list, with the `RDRAND`, `RDSEED`, `RDTSCP` and `RDPID` bits clear and no host-identifying data in any leaf.
- **S-03.2** Given code in the kernel, when it runs `RDRAND` or `RDPID`, then the machine faults and the run ends with `KernelFault`.
- **S-03.3** Given a host, when the operator runs the host probe, then it prints `blocked` or `reachable` for each instruction, and exits 0 when all are blocked, 1 when any is reachable and 2 when it could not test.
- **S-03.4** Given a kernel that uses `RDSEED` to change an answer's request, when the record is replayed, then replay refuses it as diverged.

### S-04 The monitor is confined

As an operator, I want the code that owns the machine confined, so that a bug in it reaches nothing else.

- **S-04.1** Given a live or recorded run on KVM, when the machine starts, then it runs in a separate machine runner that holds no credential and communicates with the holder over one channel.
- **S-04.2** Given hostile code in the machine runner, when it tries to open a file, connect a socket, start a program or signal another process, then the attempt fails or the runner is killed, nothing outside the runner changes, and the holder ends the run with no further effect.
- **S-04.3** Given a host without Landlock or seccomp, when a KVM run starts, then it is refused before the guest runs.
- **S-04.4** Given a malformed, oversized or out-of-order message on the channel, when the holder reads it, then the run ends with a refusal and no effect is performed.
- **S-04.5** Given a `live` run against a real repository, when no KVM kernel is configured, then it is refused before the chain is written or the guest runs.

### S-05 hyalOS's code is reviewable

As an independent reviewer, I want the code held to stated rules, so that I can trust what I am reading.

- **S-05.1** Given hyalOS's own code, when `cargo xtask lines` runs, then it reports the code against a stated budget and fails above it.
- **S-05.2** Given hyalOS's crates, when CI runs, then `unsafe` is forbidden outside a named list of files, and each `unsafe` block in them has a `SAFETY` comment.
- **S-05.3** Given hyalOS's workspace, when CI runs, then `cargo fmt --check` and `clippy -D warnings` pass on every crate.
- **S-05.4** Given a release build, when any test-only input is set (`HYALOS_SELFTEST_KERNEL`, `HYALOS_TRACE`, `HYALOS_HUGE`, or a raw kernel image), then it has no effect or the run is refused.
- **S-05.5** Given the request handler, when CI runs its fuzzer with a fixed seed, then it finishes its rounds with no panic or crash.

### S-06 Third-party components are scoped

As an independent reviewer, I want every component the claim depends on listed, so that I know what was and was not examined.

- **S-06.1** Given the review scope, when it is read, then it lists `wasmi`, the KVM crates, TLSNotary and the cryptographic crates the audit relies on, each with its version, upstream audit status, `covered` or `excluded`, and a reason.
- **S-06.2** Given a dependency in the scope changes version, when CI runs, then it fails until the scope's row changes.
- **S-06.3** Given a published advisory against a dependency, when CI's dependency check runs, then it fails and names the advisory.
- **S-06.4** Given TLSNotary publishes a release, when someone proposes moving the pin, then the written policy states the evidence required before the move.

### S-07 A reviewer can start from one package

As an independent reviewer, I want one package, so that I can start without asking the authors.

- **S-07.1** Given the review package, when it is read, then it holds the scope, the threat model, the guest interface, the stated limits, the commit under review and the commands to build and test.
- **S-07.2** Given a clean checkout and the package's commands, when they run, then they build hyalOS and every listed test passes or names why it skipped.
- **S-07.3** Given the package, when it is published, then it states how findings are reported, and that each must be fixed or recorded as an ADR before a release claims the review.

## Rules and examples

### S-01 The guest interface is fixed

Rule: both backends behave as the document says.

| ID | Given | When | Then |
|---|---|---|---|
| S-01.1.e1 | A guest that calls each of the four calls | It runs on both backends | Byte-identical transcripts |
| S-01.1.e2 | `read` with too small a buffer | It runs on both backends | Each returns `-2` |

Rule: the interface cannot change silently.

| ID | Given | When | Then |
|---|---|---|---|
| S-01.2.e1 | The interface unchanged | Tests run | Pass |
| S-01.2.e2 | The `infer` question limit raised without a version change | Tests run | Fail |

Rule: anything outside the interface is refused alike.

| ID | Given | When | Then |
|---|---|---|---|
| S-01.3.e1 | A guest importing `hyalos.clock` | Both backends load it | Both refuse with `BadImport` |
| S-01.3.e2 | A guest with no `run` export | Both backends load it | Both refuse with `BadExport` |

### S-02 A taken-over kernel has no privilege or clock

Rule: dropping privilege changes no result.

| ID | Given | When | Then |
|---|---|---|---|
| S-02.1.e1 | The committed digest guest and fixture inputs | It runs | The transcript equals the base commit's |
| S-02.1.e2 | A guest that runs out of fuel | It runs | The same `OutOfFuel` reason as the base commit |

Rule: the time counter is out of reach.

| ID | Given | When | Then |
|---|---|---|---|
| S-02.2.e1 | Kernel code running `RDTSC` | It executes | `KernelFault` |
| S-02.2.e2 | Kernel code clearing `CR4.TSD`, then running `RDTSC` | It executes | `KernelFault`, at the write |

Rule: privileged instructions fault.

| ID | Given | When | Then |
|---|---|---|---|
| S-02.3.e1 | Kernel code loading a new page table through `CR3` | It executes | `KernelFault` |
| S-02.3.e2 | Kernel code running `WRMSR` | It executes | `KernelFault` |

Rule: memory protection still holds.

| ID | Given | When | Then |
|---|---|---|---|
| S-02.4.e1 | The probe that changes the kernel's own code | It runs | `KernelFault` |
| S-02.4.e2 | The control probe that does nothing | It runs | `BadHypercall`, so the machine did boot |

### S-03 Hardware randomness is closed or stated

Rule: the kernel sees only allowed CPU features.

| ID | Given | When | Then |
|---|---|---|---|
| S-03.1.e1 | CPUID leaf 1 | The kernel reads it | The `RDRAND` bit is 0 |
| S-03.1.e2 | The host's brand string leaves | The kernel reads them | They hold zeros, not the host's name |

Rule: blocked instructions fault.

| ID | Given | When | Then |
|---|---|---|---|
| S-03.2.e1 | Kernel code running `RDRAND` | It executes | `KernelFault` |
| S-03.2.e2 | Kernel code running `RDPID` | It executes | `KernelFault` |

Rule: the host probe reports what it measured.

| ID | Given | When the probe runs | Then |
|---|---|---|---|
| S-03.3.e1 | A host where every instruction faults | It finishes | All `blocked`, exit 0 |
| S-03.3.e2 | A host where `RDSEED` runs, as the development host does | It finishes | `RDSEED reachable`, exit 1 |
| S-03.3.e3 | No access to `/dev/kvm` | It starts | Names the cause, exit 2 |

Rule: randomness cannot change a verified result.

| ID | Given | When | Then |
|---|---|---|---|
| S-03.4.e1 | A record whose answer's request was varied by `RDSEED` | It is replayed | Refused as diverged |
| S-03.4.e2 | A kernel that reads `RDSEED` and changes nothing | It is replayed | Replay passes, since the transcript is unchanged |

### S-04 The monitor is confined

Rule: the machine has its own process.

| ID | Given | When | Then |
|---|---|---|---|
| S-04.1.e1 | A KVM run | The machine starts | The vCPU runs in a child with only its KVM handles and the channel open |
| S-04.1.e2 | The child exits mid-run | The holder sees the channel close | The run ends with a refusal, and no effect follows |

Rule: the child can reach nothing else.

| ID | Given | When hostile code in the child | Then |
|---|---|---|---|
| S-04.2.e1 | A confined child | Opens the operator's home directory | The open fails, or the runner is killed |
| S-04.2.e2 | A confined child | Connects a socket | The runner is killed |
| S-04.2.e3 | A confined child | Starts a program | The runner is killed |

Rule: no confinement, no run.

| ID | Given | When | Then |
|---|---|---|---|
| S-04.3.e1 | A host without Landlock | A KVM run starts | Refused, naming Landlock |
| S-04.3.e2 | A seccomp filter that fails to install | A KVM run starts | Refused before the guest runs |

Rule: the holder trusts nothing on the channel.

| ID | Given | When the holder reads | Then |
|---|---|---|---|
| S-04.4.e1 | A message over the size limit | It arrives | Refused |
| S-04.4.e2 | A second effect request | It arrives | Refused, as today's single-effect rule |

Rule: a real run never executes a guest outside the machine.

| ID | Given | When | Then |
|---|---|---|---|
| S-04.5.e1 | `HYALOS_KERNEL` unset and a real repository | `live` starts | Refused, with no new chain link |
| S-04.5.e2 | `HYALOS_KERNEL` unset and `stub` as the repository | `live` starts | It runs, since nothing real is reached |

### S-05 hyalOS's code is reviewable

Rule: the code has a budget.

| ID | Given | When `cargo xtask lines` runs | Then |
|---|---|---|---|
| S-05.1.e1 | Code within the budget | It finishes | It reports the count and passes |
| S-05.1.e2 | Code one line over | It finishes | It fails, naming the budget |

Rule: `unsafe` is named and explained.

| ID | Given | When CI runs | Then |
|---|---|---|---|
| S-05.2.e1 | `unsafe` in a listed file with a `SAFETY` comment | The check runs | Pass |
| S-05.2.e2 | `unsafe` in an unlisted file | The check runs | Fail |

Rule: the workspace is lint-clean.

| ID | Given | When CI runs | Then |
|---|---|---|---|
| S-05.3.e1 | The workspace as merged | `fmt` and `clippy` run | Pass |
| S-05.3.e2 | A new clippy warning | `clippy` runs | Fail |

Rule: test-only inputs do nothing in a release build.

| ID | Given a release build | When | Then |
|---|---|---|---|
| S-05.4.e1 | `HYALOS_SELFTEST_KERNEL` set | `attacks` runs | The file is not read |
| S-05.4.e2 | A raw, non-ELF kernel image | A run starts | Refused with `KernelFault` |

Rule: the request handler survives hostile requests.

| ID | Given | When | Then |
|---|---|---|---|
| S-05.5.e1 | The fixed seed | The fuzzer finishes its rounds | No panic or crash |
| S-05.5.e2 | A panic introduced in the handler | The fuzzer runs | CI fails |

### S-06 Third-party components are scoped

Rule: every component has a stated position.

| ID | Given | When the scope is checked | Then |
|---|---|---|---|
| S-06.1.e1 | `wasmi 2.0.2` | Its row is read | It says the public audits cover `0.31.0` and `0.36.0`–`0.38.0`, not this version |
| S-06.1.e2 | A listed component with no reason | The check runs | Fail |

Rule: the scope follows the lockfile.

| ID | Given | When CI runs | Then |
|---|---|---|---|
| S-06.2.e1 | Versions equal to the scope | The check runs | Pass |
| S-06.2.e2 | `wasmi` moved without a scope change | The check runs | Fail |

Rule: an advisory stops the build.

| ID | Given | When CI runs | Then |
|---|---|---|---|
| S-06.3.e1 | No advisory | The dependency check runs | Pass |
| S-06.3.e2 | An advisory against a pinned crate | The dependency check runs | Fail, naming it |

Rule: a pin moves only on stated evidence.

| ID | Given | When the policy is applied | Then |
|---|---|---|---|
| S-06.4.e1 | A release with passing forgery, canary and differential evidence | The move is reviewed | It may proceed |
| S-06.4.e2 | A release that changes the presentation format | The move is reviewed | It needs a new record decision first |

### S-07 A reviewer can start from one package

Rule: the package stands alone.

| ID | Given | When | Then |
|---|---|---|---|
| S-07.1.e1 | The package | Its required sections are checked | All are present |
| S-07.1.e2 | A package naming no commit | The check runs | Fail |

Rule: its commands work from clean.

| ID | Given | When | Then |
|---|---|---|---|
| S-07.2.e1 | A clean checkout on a KVM host | The commands run | Build and tests pass |
| S-07.2.e2 | A host with no KVM | The commands run | KVM tests name why they skipped |

Rule: findings have a path to closure.

| ID | Given | When | Then |
|---|---|---|---|
| S-07.3.e1 | A finding that is fixed | It is closed | It cites the fixing commit |
| S-07.3.e2 | A finding that is accepted | It is closed | It cites an ADR |

## Failure modes

| Failure | Trigger | Expected behaviour | What the user sees | Criterion |
|---|---|---|---|---|
| Ring 3 changes a result | The kernel relied on privilege | The slice stops before merging | A differing transcript | S-02.1 |
| `RDSEED` stays open | The host cannot intercept it | The probe reports it, and replay still guards the result | `RDSEED reachable` | S-03.3 |
| Confinement is unavailable | An old kernel, or a container blocking it | The run is refused | The missing feature's name | S-04.3 |
| The machine runner dies | A bug or a kill | The run ends, and no effect follows | A refusal | S-04.1 |
| A pin moves unnoticed | A lockfile update | CI fails | The component's name | S-06.2 |
| An upstream advisory | A new `wasmi` or TLSNotary flaw | CI fails | The advisory's ID | S-06.3 |
