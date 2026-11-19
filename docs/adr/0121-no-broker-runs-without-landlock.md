# ADR-0121: No broker runs without Landlock

**Status:** Proposed. Supersedes [ADR-0118](0118-the-broker-confines-itself-with-landlock.md)'s rule that a broker on a kernel without Landlock runs unconfined.

## Context

[ADR-0118](0118-the-broker-confines-itself-with-landlock.md) confines the broker with Landlock, so a compromised broker cannot read `engine.key` or `github-app.pem`. On a kernel without Landlock, it let the broker run unconfined, with a warning on standard error. That left the keys readable on exactly the hosts where the warning was easiest to miss. Every other control in Hyalos holds unless it can show it holds (ADR-0009). The operator asked for the same here.

## Decision

**No broker runs unless it confines itself.**
- **The engine checks first.** It asks the kernel before it starts a broker. Without Landlock, `BrokerProcess::launch_confined` refuses, with `LaunchError::NoLandlock`, which says what the host needs, and no job starts.
- **The broker checks again.** A broker that still finds Landlock not enforced when it confines itself exits before it serves.
- **A partial confinement still runs.** That is a Landlock older than the ABI the rules ask for: its first version already restricts reading and writing files, so the keys stay out of reach.
- **`hyalos doctor` reports the broker's confinement,** and fails on a host where no broker can run.
- **Setup names the requirement:** Linux 5.13 or later, with `landlock` in the kernel's LSM list.

## Consequences

- On every host where a job runs, a compromised broker cannot read the engine's keys. Known gap 1 in the threat model is closed.
- A kernel without Landlock, or with it turned off, runs no job, and `hyalos doctor` says why. The host has to be fixed; Hyalos offers no switch to run unconfined.
- Hyalos already runs only on Linux. The release binary is static musl (ADR-0081), and WSL2's kernel has Landlock.
