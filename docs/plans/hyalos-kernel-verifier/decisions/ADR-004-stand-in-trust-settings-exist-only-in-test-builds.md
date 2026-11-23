# ADR-004: Stand-in trust settings exist only in test builds

**Status:** accepted (developer, 2026-11-23)

**Proposed by:** author (spec); accepted by the developer at the spec ADR checkpoint

## Context

Today, environment variables point the audit at a stand-in log and a stand-in provider root (`context.md` §2). The report marks such a run. In a program handed to customers, a wrapper script or a poisoned environment could set them, and a reader might miss the mark or see only the exit code.

## Options

1. **Keep the environment variables and rely on the report's mark.** Rejected: exit 0 would still mean a pass against a log the attacker chose.
2. **Replace them with a command-line flag.** Rejected: a wrapper can pass a flag as easily as it can set a variable.
3. **Compile them only into test builds, behind a Cargo feature.** Chosen.

## Decision

The stand-in log and stand-in provider settings are available only with the `test-trust` feature of `hyalos-audit`. `hyalos-verify`'s release build does not enable it, and its code then has no path that reads those variables. A test builds the release configuration, sets every such variable, and shows they have no effect.

## Consequences

- The verifier's own tests build with the feature, so release and test builds differ. CI checks both.
- A customer cannot rehearse against a stand-in with the released binary, and does not need to.
- The monitor's existing tests keep their stand-in through the feature.
- `attest`'s shared library reads the provider settings from the environment today (`spikes/task-kernel/attest/src/lib.rs:85`). The presentation check the verifier links must take its trust settings as a value and must not contain that read.
