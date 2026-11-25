# ADR-0137: `hyalos.spike/2` allows deterministic floats, and a spike kernel image names its own profile

**Status:** Accepted

## Context

The Python guest spike (`docs/plans/hyalos-kernel-python`, #438) runs the same triage script under two interpreters. MicroPython runs under `hyalos.spike/1`, which has profile 2's settings (ADR-0136). Monty, a Python interpreter in Rust, does not. Its guest contains float instructions, which profiles 1 and 2 refuse at load. It also needs more memory and fuel than profile 2's 4 MiB and 200,000,000. ADR-0136 says a later `hyalos.spike/N` is a new profile with its own ADR. It also says a spike profile with settings other than profile 2's needs a kernel image built for it, and needs the KVM backend to compare a run's profile against that image's.

Floats are why profiles 1 and 2 refuse them: on x86, `f32` zero divided by zero gives the NaN `0xffc00000` where other hosts give `0x7fc00000`, and a transcript must not depend on the host. `wasmi` 2.0.3 has a Cargo feature, `deterministic`, that canonicalises every NaN a float operation produces. A Cargo feature applies to the whole build, and one monitor build runs every profile it knows, so the setting cannot be confined to one profile (the pack's ADR-007).

Until now the KVM backend ran a profile only when its settings were `CURRENT`'s (`on_backend` in `hyalos-os/monitor/src/main.rs`), because there was one kernel image and it was built for `CURRENT`.

## Decision

- **The profile.** `hyalos.spike/2` is `hyalos.spike/1` with three changes. `floats` is true. `max_memory` is 5 MiB, the smallest whole number of MiB at which Monty's seed-0 triage run on the issue fixture ends with a result. `fuel` is 830,000,000, twice that run's fuel (413,707,242) rounded up to the next ten million. It enforces a contract, as every spike profile does. `lookup` finds it only with `spike-contract`. It is never edited. A test pins its fields, and another reruns the fixture to check the memory and fuel rule.
- **Deterministic floats.** The `spike-contract` features of `hyalos-audit` and `task-kernel` turn on `wasmi/deterministic` for the whole build. A default build does not. Profiles 1 and 2 still refuse every float instruction at load, so their transcripts and fuel are unchanged. The pinned-transcript tests run with the feature on to prove it. A test runs an `f32` 0/0 guest under `hyalos.spike/2` on both backends. It requires the same transcript and the bits `0x7fc00000`. It fails on the process backend when the feature is off.
- **The kernel image feature.** The kernel image gains `spike-profile-2`. It sets the image's `PROFILE` to `hyalos.spike/2` and forwards to `hyalos-profile/spike-contract` and `wasmi/deterministic`. Its layout is the default one. `spikes/438/kernel.sh hyalos.spike/2` builds it into its own target directory, `target-spike-2`, so it never replaces the default image.
- **The image names its profile.** A `spike-profile-2` image carries, in its own bytes, the marker `\0hyalos-kernel-profile=`, then the profile's name, then a zero byte. `run` names the marker, so the linker keeps it. The marker adds no hypercall, and `hyalos-os/ABI.md` is unchanged.
- **The monitor reads it.** In a `spike-contract` build, the monitor reads the marker from the image in `HYALOS_KERNEL`:
  - An image with no marker is the default image, built for `CURRENT`.
  - An image whose marker names a profile this build knows is built for that profile.
  - An image with more than one marker, a marker with no zero byte after the name, or a marker that names an unknown profile is refused before boot, with "the kernel image names no profile this build knows" and no record.

  `on_backend` compares the run's profile with the image's profile by `same_settings`, not with `CURRENT`. A run under another profile ends `ReplayDiverged`, as before. A default build reads no marker and keeps comparing with `CURRENT`.
- **No spike-only layout.** Monty's guest is 6,945,056 bytes, past `GUEST_MAX` (1 MiB). So on the `spike-profile-2` image, Monty is refused with `TooLarge` before boot, and the spike runs Monty on the process backend only. The pack's stop condition allows this, and `spikes/438/README.md` records it. A layout that held the guest would still meet the kernel stack overflow of issue #503. The monitor's memory guards (`HEAP`, `STACK`, `GUEST_MAX`) are not changed.

## Consequences

- A record naming `hyalos.spike/2` replays only in a `spike-contract` build. On KVM it replays only on a `spike-profile-2` image. No release build or published verifier reads it.
- Every spike build now computes floats with canonical NaNs, including its runs under profiles 1 and 2, which have no float instructions to change. Capsules, if it allows floats, inherits this: the setting is per build, not per profile.
- A default build pointed at a `spike-profile-2` image does not read its marker. It runs profile 2 on an image whose interpreter allows floats and larger limits, and so loses the KVM guarantee that a float module is refused. An operator should not point a default monitor at a spike image. Nothing stops them, since the image is chosen by path.
- Monty on KVM needs a spike-only layout larger than the default machine, and the fix for #503. Neither is in this change. The layout's guards would have to be derived from that layout, never widened on their own.
- Capsules replaces the spike profiles, the marker and the image feature. Nothing here commits it to their form.
