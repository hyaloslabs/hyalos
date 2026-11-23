# ADR-003: v1 is the control plane, and hyalOS runs its bounded steps

**Status:** accepted (developer, 2026-11-22)

**Proposed by:** author (parent-level pass), after the developer confirmed that v1 and hyalOS are converging; accepted by the developer

## Context

The parent pack treated joining the existing engine as its last pack. With the two converging, the capsule contract cannot be fixed first, because it overlaps with four things v1 already has:

| v1 has | hyalOS has | The risk if each is designed alone |
|---|---|---|
| A signed manifest of a job's actions, hosts, tools, budget and result (ADR-0031) | A task contract | Two permission languages |
| A broker that holds provider keys (ADR-0038) | An `attest` tool that reads its own key file | Two credential holders |
| A signed, hash-chained event log (ADR-0019) | A record checked against a public log | Two accounts of one run, with no link between them |
| Cedar policy and operator approvals (ADR-0008, ADR-0103) | The same outsider rule, stated in the parent | Two admission paths |

The trusted core has 1,406 lines left, and ADR-001 keeps hyalOS out of it.

v1's `Substrate` starts a sandbox from an image, a command, an environment, a broker socket and a process limit (`core/substrate/src/spec.rs:34`). A hyalOS task has none of these.

## Options

1. **Keep two products side by side.** Rejected: it duplicates all four rows above.
2. **Make hyalOS a `Substrate` driver.** Rejected: most of `SandboxSpec` would be meaningless, and a driver would imply the task has a general sandbox with a broker socket.
3. **Replace v1 with hyalOS.** Rejected: building and testing code needs a real computer, as the parent states.
4. **v1 stays the control plane and runs a hyalOS task as a distinct kind of step.** Chosen.

## Decision

- **One control plane.** The v1 engine admits, starts, tracks and ends hyalOS tasks. Coding jobs stay on the microVM substrate.
- **A separate step kind.** A small adapter in the engine process starts the hyalOS monitor as its own process. It does not implement `Substrate`.
- **One admission path.** Cedar policy and the operator's approval rule decide whether a signed contract may run. The contract states what a task may do. It does not become a second policy language, and the manifest does not grow task fields.
- **One credential store.** Provider and connector credentials stay in v1's store. A task's monitor receives only the one credential that task needs, and never through its environment.
- **Two records, linked.** The event log is the operator's account, and the hyalOS record is the evidence for outsiders. The log stores the record's hash and verdict, never its task data. A verifier never needs the event log.
- **First joint use.** The candidate is v1's reader step, which already turns untrusted issue text into a typed result. It is the closest existing match for a capsule. The engine work confirms or replaces this candidate.

## Consequences

- This decision must be accepted before the capsule contract is fixed. The integration build stays last.
- A new event bumps `SCHEMA_VERSION`, under ADR-0019 and ADR-0023.
- The adapter counts toward the 30,000-line budget, so the engine pack must set its size or remove code to pay for it.
- Handing a credential to the monitor needs a design that holds under ADR-0002 and ADR-0038. Attested calls need the credential inside the TLS prover, so the broker cannot simply relay them.
- v1 and hyalOS can still ship at different times, because a coding job does not depend on hyalOS.
