# ADR-0057: Recording a job's attestation

**Status:** Proposed

## Context

[ADR-0011](0011-pluggable-substrates.md) says the engine "stores attestation evidence on the job's event when the substrate provides it". Attestation evidence is something an auditor can check without trusting the engine: a statement about what was started, signed by a root of trust beneath the host, such as a TPM quote or an AMD SEV-SNP or Intel TDX report. Story S2.6 (#14) asks that the job event carry the evidence, or record that none was available.

None of the drivers Hyalos has or plans for v1 offers any:
- a container, under `runc` or gVisor ([ADR-0056](0056-the-gvisor-substrate.md)), runs on the host's kernel or a kernel in a host process, and nothing below the host signs it;
- a Firecracker microVM (S2.4, #12) runs a guest the host builds and starts. Firecracker has no confidential-computing mode, so whatever it could report would be the host vouching for itself.

The host could hash what it started (the image, the kernel, the init) and record that. Those hashes would be the engine's word, which the signed log already is. Calling them attestation would tell an auditor that something beyond the engine stands behind the job when nothing does.

## Decision

**`job.started` records an attestation.** The event gains a required `attestation`, a `hyalos_types::Attestation`. It is one of:
- `{"none": "<reason code>"}`: the substrate offers no evidence, for the reason the code names;
- `"unrecorded"`: an event written before this field existed.

The schema goes to version 9. An older `job.started` is upcast to `"unrecorded"`, a constant ([ADR-0023](0023-event-schema-versions-and-upcasting.md)). Nothing else changes, and the state does not keep it, as for the substrate ([ADR-0043](0043-choosing-the-substrate-recording-it-and-doctor.md)).

**Every driver says what it can attest.** `Substrate::attestation` is required, with no default, so a new driver must state it. The container driver, gVisor, the local-process fake and the simulated world's substrate all answer `none` with `attestation.no_hardware_root`. `hyalos run` records the chosen driver's answer and prints it (`attestation: none (attestation.no_hardware_root)`), and `hyalos doctor` prints it beside the substrate it would choose.

**Evidence, when a substrate offers it, is a new decision.** Its format, where its bytes are kept (an event holds digests and codes, never large blobs, [ADR-0019](0019-typed-events-and-in-memory-projections.md)), and when it is known matter. A confidential VM's report exists only after the guest boots, which is after `job.started` is written. So the variant that carries evidence, likely a format code and a digest of the evidence in a separate event, is added with the first driver that has some, with its own schema version. The shape here leaves room for it: `Attestation` is a closed enum that refuses any variant it does not know.

## Consequences

An auditor reading a job sees which substrate ran it and that nothing beyond the engine attests to it, in the signed log, instead of having to infer that from the substrate's name. A job never claims evidence it does not have.

Every `job.started` grows by about 50 bytes. Logs written before version 9 still load, and their jobs read as `unrecorded`, which is distinct from `none`: the engine did not say, rather than said there was none.

Tests: the pinned payloads at version 9 and the upcast of versions 1 to 8 (`core/types/tests/schema.rs`), a refused unknown variant (`core/types/src/substrate.rs`), the recorded and printed attestation of a run (`core/hyalos/tests/run_job.rs`), and doctor's line.
