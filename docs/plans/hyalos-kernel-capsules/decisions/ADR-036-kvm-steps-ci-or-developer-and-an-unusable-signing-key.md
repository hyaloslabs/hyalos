# ADR-036: KVM steps proven by CI or by the developer, and an unusable signing key

**Status:** accepted (developer, 2026-11-27)

**Proposed by:** author (cold-read); accepted by the developer in cold-read round 7

## Context

This amends ADR-010, ADR-031 and ADR-035. ADR-035 names the developer as owner of the KVM legs in Slices 1, 2 and 8 only. Slice 1 also has the `spikes/438` `hostile.sh` and `measure.sh` KVM legs, Slices 2 and 3 run workspace tests under `HYALOS_REQUIRE_KVM=1`, and Slice 11 re-runs Slice 8's declared-limit run when the ceiling C is below 200. None of these has an owner on a host without `/dev/kvm`. CI's `hyalos-os` and `hyalos-os-spike` jobs make `/dev/kvm` usable and set `HYALOS_REQUIRE_KVM` (`.github/workflows/ci.yml:266-269`, `:292`, `:299`, `:363-366`, `:373`), but run no `spikes/438` script.

`notary public --dir` (Slice 10) has an outcome only for a missing `signing.key`. The key reader also refuses a key file that is open to group or others, a link, not a regular file, or the wrong length, each with a fixed message that holds no key bytes (`hyalos-os/attest/src/lib.rs:539-554`).

ADR-035 shrinks `questions` and `accepts.records` to C but leaves `accepts.bytes` and the digest guest's embedded contract bytes unstated. ADR-034 takes concurrency from the notary's code, which ADR-010's measurement wording still describes as measured.

## Options

1. **CI proves the workspace KVM tests; the developer runs every other KVM step.** Chosen.
2. **The developer runs every KVM step, workspace tests included.** Rejected: CI already runs those tests on KVM, so a second run proves nothing new.
3. **Executors must work on a KVM host.** Rejected: it shuts out executors the rest of the plan does not need to exclude.
4. **`notary public` prints the key reader's fixed message and exits 2.** Chosen.
5. **One fixed line for every unusable key.** Rejected: it hides which fault the operator must fix.
6. **Treat an unusable key as a missing one.** Rejected: the operator would be told to make a key that already exists.
7. **Exit 1 for an unusable key.** Rejected: exit 1 means refused elsewhere in the tools; an unusable key is a setup fault, as a missing one is.

## Decision

- A test run under `HYALOS_REQUIRE_KVM=1` in any slice is proven by the pull request's CI `hyalos-os` job (or `hyalos-os-spike` for the spike-feature run) when the executor's host has no `/dev/kvm`. The executor records the CI run in the progress file.
- Every other KVM step is a developer step under ADR-035's rule: the executor sets the slice `blocked`, naming the step, and the developer runs it and sets `done` in the pull request's last commit. These steps are Slice 1's `run.sh`, `hostile.sh` and `measure.sh` KVM legs, Slice 8's declared-limit run on KVM, and Slice 11's re-run of it when C is below 200.
- `notary public --dir <state>`, when `signing.key` exists but the key reader refuses it, prints that reader's fixed message on standard error, prints nothing on standard output, makes no key, and exits 2. A test covers each refusal: open mode, a link, not a regular file, too long, too short.
- When C is below 200, Slice 11 also sets triage's `accepts.bytes` to C × `record_bytes` and digest's to C × 16,384, and the digest guest's embedded copy of the contract's canonical bytes becomes the shrunk contract's bytes, as ADR-035's shrink requires.
- ADR-034's rule, concurrency taken from the notary's code and busy test, also amends ADR-010's measurement: concurrency is stated, not measured live.

## Consequences

Every KVM step has an owner on any host, and no slice is done before its KVM proof exists. An operator with a bad key file is told which fault to fix, and no key is made over it. The shrunk contracts and the guest stay consistent.
