# ADR-0136: hyalOS checks a draft contract under spike profiles, behind `spike-contract`

**Status:** Accepted

## Context

The Python guest spike (`docs/plans/hyalos-kernel-python`, #438) asks whether a hyalOS task can be a short Python script run by an interpreter compiled into the guest, against an SDK generated from a contract. Capsules (`hyalos-kernel-capsules`) will fix the real contract and needs the answer first.

Today the monitor enforces fixed limits only: schema `0`, effect kind `0`, one effect and size limits (`Host::pre_infer` and `Host::pre_effect` in `hyalos-os/audit/src/replay.rs`). A guest whose SDK leaves out `notify` can still call `effect`, so an SDK that hides a method is not a boundary. The spike needs the monitor, not the SDK, to refuse what a task's contract does not grant, on both backends.

The spike's pack decided where this lives and how it travels: everything in `hyalos-os/` compiles only with a `spike-contract` feature on `hyalos-profile`, `hyalos-audit` and `task-kernel`, and a default build is proven unchanged (its ADR-001 and ADR-006); a spike profile has profile 2's settings, and a runtime that needs other settings gets the next one (its ADR-002); the contract travels as input record 0, with the SDK and the script as records 1 and 2 (its ADR-003). Those decisions are the pack's. This ADR records what they change in hyalOS, as `CLAUDE.md` asks of a new reason, profile and record form.

`hyalos-os/ABI.md`'s `hyalos.abi/1` section says every input record is one line of UTF-8, and its digest is pinned by tests. A script, an SDK and a contract are several lines each.

## Decision

- **The feature.** `hyalos-profile`, `hyalos-audit` and `task-kernel` each have a `spike-contract` feature, off by default. `task-kernel`'s forwards to both others and `hyalos-audit`'s to `hyalos-profile`. Everything below compiles only with it. A default build's profiles, reasons, commands, reason table and kernel image are what they were, and its tests prove it: a record naming `hyalos.spike/1` is refused as an unknown profile, the channel's reason table is the same 29 entries, `spike-run` and `spike-task` are usage errors, and `HYALOS_SPIKE_PROFILE` changes nothing `run` does.
- **Spike profiles.** `hyalos.spike/1` is profile 2's settings, limits and `wasmi` under another name. `lookup` finds it only with the feature. A later `hyalos.spike/N` is a new profile, never an edit, and needs its own ADR. Every `hyalos.spike/` profile enforces a contract. Since `hyalos.spike/1` has profile 2's settings, the default kernel image runs it and the KVM backend is unchanged.
- **The draft contract.** Its canonical bytes are a JSON object with exactly the keys `asks` and `may_touch`, in that order, no whitespace, each a list of distinct ascending decimal integers the host supports (today only `0` for each), at most 4,096 bytes: `{"asks":[0],"may_touch":[0]}` grants schema `0` and effect `0`. Any other byte form, another key, a duplicate or an unsupported value is not a contract.
- **Where it is read.** Under a spike profile, `execute_with` parses input record 0 as the contract before the guest loads, so a run and its replay both read it from the record's own inputs. A missing record or one that is not a contract ends the run with `BadContract`. Nothing else is read for it.
- **The check.** `Host::pre_infer` refuses a schema the contract's `asks` lacks with `UnknownSchema`, and `Host::pre_effect` an effect kind its `may_touch` lacks with `UnknownEffect`, before the existing checks and before any guest bytes are read. Both backends answer every call through `Host`, so the check holds on both. One effect per run still holds.
- **`BadContract`** is a spike-only reason. It is appended last to the machine runner's reason table, so a default build's table keeps its 29 bytes. It is never in `RUNNER_REASONS`, because the holder decides it before any machine boots. `ABI.md` does not name it.
- **The record form.** Under a spike profile, records 0 to 2 are whole files and may hold newlines. Records 3 onward keep `ABI.md`'s form. `ABI.md`'s `hyalos.abi/1` section is not edited: this ADR is where the exception is stated. The task identity is the existing version 4 framing over the profile, the guest hash and every record, so the contract, the SDK and the script are bound into it with no new field, version or framing.
- **The commands.** `spike-run <guest> <contract> <sdk> <script> <data> <record> [seed]` runs under `hyalos.spike/1`, or the spike profile `HYALOS_SPIKE_PROFILE` names, and refuses before running when that variable names anything else. It prints the `ok:` line and then one timing line: the time from the command starting to the guest's first host call, as the holder sees it, and to the transcript being complete. Timing is never written into the record. `spike-task` prints the task `spike-run` would record, without running.

## Consequences

- A spike record names `hyalos.spike/1`, which no release build or published verifier knows, so none of them reads it. That is intended: the spike's records are evidence for the findings, not records a customer authorises.
- The spike shows the identity covers the contract, the SDK and the script. Signing or authorising that identity is capsules' concern.
- CI's `hyalos-os` job lints and runs every test a second time with the feature, so the pinned transcripts (profile 1 since the ring 3 slice, profile 2 from this change's base commit) and the hostile guests are shown unchanged by it. Its change filter also matches `spikes/438/`, where the spike's committed evidence will live.
- A spike profile with other settings than profile 2's (floats, memory, fuel) needs the kernel image built for it and the KVM backend to compare against that image's profile. The pack's design states that mechanism, and the slice that first needs it adds it with its own ADR.
- Capsules replaces the draft contract, its check and the spike profiles. Nothing here is a commitment to their form.
