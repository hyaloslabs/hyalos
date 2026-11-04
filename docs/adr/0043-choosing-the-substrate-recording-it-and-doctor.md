# ADR-0043: Choosing the substrate, recording it on the job, and `hyalos doctor`

**Status:** Proposed

## Context

[ADR-0011](0011-pluggable-substrates.md) decides that the engine picks the best substrate the host has, records which one ran each job, and refuses to start a job on a host that is not Linux, saying why. [ADR-0032](0032-the-substrate-interface-and-its-contract.md) gives drivers a `name`, an `isolation` level and `available`, but nothing chooses among them and `job.started` records only the task.

[ADR-0041](0041-an-init-in-the-sandbox-and-the-image-tests-further-checks.md) makes every container sandbox start under the runtime's init (`--init`). A Podman without `catatonit`, or without the init its `containers.conf` names, refuses every such sandbox. `available` only asks the runtime for its version, so that Podman looks ready and every job on it fails. The runtimes do not report whether their init is installed in a way that can be trusted: Docker reports the init's name even when the file is missing, and Podman does not report it at all.

[ADR-0038](0038-model-calls-from-the-sandbox-and-the-provider-key-file.md) asks the operator to set a spend cap on the provider key at the provider, but nothing reminds them.

This is story S2.3 (#11).

## Decision

**`job.started` names its substrate.** The event gains `substrate`, a `SubstrateName` (in `hyalos-types`): 1 to 64 characters of lowercase letters, digits, `.`, `_` and `-`, like a reason code, so a runtime's path or version never reaches the log. The event schema goes to version 6. A `job.started` written at an earlier version is upcast to `"substrate": "unrecorded"`, a constant ([ADR-0023](0023-event-schema-versions-and-upcasting.md)); at version 6 the field is required. The state does not keep it: the log is the record, and no rule depends on it, so `StateImage` does not change.

**A driver says what it records.** `Substrate::recorded_as` defaults to `name`. The container driver records `container.podman` or `container.docker`, by the runtime command's file name, or `container` for any other command. The audit then shows whether a job ran under a rootless Podman or a Docker daemon, which differ in strength ([ADR-0032](0032-the-substrate-interface-and-its-contract.md)).

**A driver says whether it is ready for an image.** `Substrate::ready(image)` defaults to `available`. The container driver checks, in order:
1. the image is a plain reference;
2. the runtime answers;
3. the runtime has the image (`image inspect`; jobs never pull);
4. the runtime starts one container from the image under `--init` and runs `true`. The probe uses the job sandbox's options that bear on starting (`--rm`, `--network none`, read-only root, user 65534, no capabilities, no new privileges, `--pull never`, no log) and leaves nothing behind.

Starting a container is the only check that is the same on both runtimes and shows what a job needs. Unlike `available`, `ready` may start this probe, never a job's sandbox. When the probe fails, the message gives the runtime's first line and what to install. For Podman that is `catatonit` (or an `init_path` in `containers.conf` that is installed); for Docker, Docker Engine's own `docker-init` (or `init-path` in `daemon.json`).

**Choosing.** `hyalos_substrate::choose(os, drivers, image)` first refuses any host whose `os` is not `linux`, before any driver is asked (`Unsupported`, whose message says that v1 starts jobs only on Linux, why, and that WSL2 counts as Linux). On Linux it asks every driver in order and chooses the ready one with the strongest isolation; among equals, the first. It returns every driver it asked with its answer (`Choice::tried`), so a refusal is reported and never silent. A driver that is not ready is never chosen. `detect(image)` is `choose` on this host among Podman then Docker. The engine calls `detect` before it starts a job and records `Choice::chosen_name` on `job.started`. No command starts a job yet: that is the end-to-end CLI job (#70).

**`hyalos doctor [--image <image>]`** prints:
- the host and whether it is supported;
- the image, by default `hyalos-sandbox:latest`, as [ADR-0040](0040-the-sandbox-image.md) builds it;
- each substrate with `ready` or the reason it is not;
- the one chosen, which each job will record;
- a reminder to set a spend limit on the dedicated API key at the provider, because a key cannot be scoped to one job.

It exits 0 if a job could run, 1 if not, and 2 for bad usage. It reads no key and writes nothing.

## Consequences

Every job from now on says which substrate ran it, in the signed log, and old logs still load with that field unrecorded. A Podman that cannot start an init is reported with what to install instead of failing every job. Detection now starts a short container, which takes about a second and needs the image to be present: a host without the sandbox image has no ready substrate, which is true, since no job could run there. Docker's and Podman's messages for a missing init differ by version, so the advice depends on which runtime is named, not on parsing the message. The probe is only as good as `true` being in the image, which holds for the sandbox image and `ubuntu:24.04`. A driver added later only has to implement `ready` and `recorded_as` to be chosen and recorded.
