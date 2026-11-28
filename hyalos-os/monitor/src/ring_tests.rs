//! The kernel runs in ring 3 (S-02, ADR-001 of `docs/plans/hyalos-kernel-core`).
//!
//! The committed guest gives the transcript it gave on the ring 0 kernel, and a taken-over kernel
//! that reads the clock, runs a privileged instruction or touches memory it may not ends the run
//! with `KernelFault`. The probes run on the self-test kernel, chosen by the first input byte.
//! `HYALOS_REQUIRE_KVM` turns a host without KVM into a failure.

use super::*;
use crate::profile_tests::{kvm_backend, selftest_kvm_backend};
use std::path::Path;

/// SHA-256 of `task-kernel run`'s transcript file for the committed digest guest on
/// `evidence/fixtures/issues.tsv`, by seed, from the ring 0 kernel at this slice's base commit
/// (`9c6706b`). Both backends gave these bytes there, under `hyalos.profile/1` on `wasmi` 2.0.2.
const BASE: &[(u64, &str)] = &[
    (
        0,
        "24d971b2d51171575deeb4f4fa808111a68412c64889eb83a12daca6712e46a5",
    ),
    (
        7,
        "0c7b1259200088d28cf74dfc143a2ec7f7d940b26d5bcac56ec442ef3431cbc2",
    ),
];

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}

/// The bytes `task-kernel run` writes for a transcript.
fn written(t: &Transcript) -> Vec<u8> {
    serde_json::to_vec_pretty(t).unwrap()
}

/// Runs one self-test probe by name and returns how the run ended, beside how it must end.
fn probe(kernel: &Backend, name: &str) -> (Result<(), Reason>, Reason) {
    let index = PROBES
        .iter()
        .position(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("no probe {name}"));
    let got = execute(kernel, &[index as u8], vec![], Mode::Record { seed: 0 }).map(|_| ());
    (got, PROBES[index].1)
}

/// Asserts each named probe ends in a kernel fault on the self-test kernel.
fn faults(test: &str, names: &[&str]) {
    let Some(kernel) = selftest_kvm_backend(test) else {
        return;
    };
    for name in names {
        let (got, want) = probe(&kernel, name);
        assert_eq!(want, Reason::KernelFault, "{name}");
        assert_eq!(got, Err(Reason::KernelFault), "{name}");
    }
}

#[test]
fn core_s_02_1_e1_the_digest_guest_gives_the_base_commits_transcript_in_ring_3() {
    let Some(kvm) = kvm_backend("core_s_02_1_e1") else {
        return;
    };
    let guest = fs::read(root().join("guest/digest.wasm")).unwrap();
    let inputs = read_inputs(
        root()
            .join("evidence/fixtures/issues.tsv")
            .to_str()
            .unwrap(),
    )
    .unwrap();
    for &(seed, base) in BASE {
        // Under the profile the pins were made with, on this build's `wasmi` (ADR-0134).
        let ring_3 = under_profile_1(&kvm, &guest, inputs.clone(), seed);
        let process = under_profile_1(&Backend::Process, &guest, inputs.clone(), seed);
        assert_eq!(sha(&written(&ring_3)), base, "seed {seed}");
        assert_eq!(written(&ring_3), written(&process), "seed {seed}");
        // A new run differs only in the profile it names and the task that name is part of.
        let new = execute(&kvm, &guest, inputs.clone(), Mode::Record { seed }).unwrap();
        assert_eq!(new.profile.as_deref(), Some("hyalos.profile/2"));
        assert_eq!(
            written(&Transcript {
                profile: ring_3.profile.clone(),
                task: ring_3.task.clone(),
                ..new.clone()
            }),
            written(&ring_3),
            "seed {seed}"
        );
        assert_eq!(
            written(&new),
            written(
                &execute(
                    &Backend::Process,
                    &guest,
                    inputs.clone(),
                    Mode::Record { seed }
                )
                .unwrap()
            ),
            "seed {seed}"
        );
    }
}

/// A version 4 run that names `hyalos.profile/1`, as new runs did before `hyalos.profile/2`.
fn under_profile_1(backend: &Backend, guest: &[u8], inputs: Vec<Vec<u8>>, seed: u64) -> Transcript {
    hyalos_audit::execute_with(
        CURRENT_VERSION,
        Some(hyalos_profile::PROFILE_1.name),
        guest,
        inputs,
        Mode::Record { seed },
        |guest, host, profile| on_backend(backend, guest, host, profile),
    )
    .unwrap()
}

/// S-01.3.e2 (Slice 1 of `docs/plans/hyalos-kernel-capsules`): on the kernel image built with
/// `portable-dispatch`, the pinned profile 1 transcripts keep their hash on KVM.
#[test]
fn capsules_s_01_3_e2_profile_1_pins_hold_on_the_rebuilt_image() {
    let Some(kvm) = kvm_backend("capsules_s_01_3_e2") else {
        return;
    };
    let guest = fs::read(root().join("guest/digest.wasm")).unwrap();
    let issues = root().join("evidence/fixtures/issues.tsv");
    let inputs = read_inputs(issues.to_str().unwrap()).unwrap();
    for &(seed, base) in BASE {
        let t = under_profile_1(&kvm, &guest, inputs.clone(), seed);
        assert_eq!(sha(&written(&t)), base, "seed {seed}");
    }
}

#[test]
fn core_s_02_1_e2_a_guest_out_of_fuel_ends_as_on_the_base_commit() {
    let Some(kvm) = kvm_backend("core_s_02_1_e2") else {
        return;
    };
    let wasm = wat::parse_str(with_run("(loop $l (br $l))")).unwrap();
    assert_eq!(
        execute(&kvm, &wasm, vec![], Mode::Record { seed: 0 }).err(),
        Some(Reason::OutOfFuel)
    );
    assert_eq!(
        execute(&Backend::Process, &wasm, vec![], Mode::Record { seed: 0 }).err(),
        Some(Reason::OutOfFuel)
    );
}

/// Every hostile guest of the `attacks` list ends as it did on the ring 0 kernel, where the
/// `attacks` command passed all of them, and as on the process backend.
#[test]
fn core_s_02_1_every_hostile_guest_ends_as_before_in_ring_3() {
    let Some(kvm) = kvm_backend("core_s_02_1") else {
        return;
    };
    for (name, source, want) in attacks() {
        let wasm = wat::parse_str(&source).unwrap();
        let ring_3 = execute(&kvm, &wasm, vec![], Mode::Record { seed: 0 }).map(|t| written(&t));
        assert_eq!(ring_3, Err(want), "{name}");
        assert_eq!(
            ring_3,
            execute(&Backend::Process, &wasm, vec![], Mode::Record { seed: 0 })
                .map(|t| written(&t)),
            "{name}"
        );
    }
}

#[test]
fn core_s_02_2_e1_reading_the_time_counter_faults() {
    faults(
        "core_s_02_2_e1",
        &["privilege: read the time counter (RDTSC)"],
    );
}

#[test]
fn core_s_02_2_rdtsc_and_rdtscp_fault() {
    faults(
        "core_s_02_2",
        &[
            "privilege: read the time counter (RDTSC)",
            "privilege: read the time counter (RDTSCP)",
        ],
    );
}

/// The probe sends an unknown request between the write and the clock read, so a write that went
/// through would end the run with `BadHypercall`. `KernelFault` means the write itself faulted.
#[test]
fn core_s_02_2_e2_clearing_cr4_tsd_faults_at_the_write() {
    faults(
        "core_s_02_2_e2",
        &["privilege: clear CR4.TSD, then read the time counter"],
    );
}

/// The probe loads page tables of its own that map all memory writable and executable, then
/// sends an unknown request, so a load that went through would end the run with `BadHypercall`.
#[test]
fn core_s_02_3_e1_loading_other_page_tables_faults() {
    faults(
        "core_s_02_3_e1",
        &["privilege: load other page tables (CR3)"],
    );
}

#[test]
fn core_s_02_3_e2_writing_a_model_specific_register_faults() {
    faults(
        "core_s_02_3_e2",
        &["privilege: write a model-specific register (WRMSR)"],
    );
}

#[test]
fn core_s_02_3_control_registers_descriptor_tables_msrs_and_hlt_fault() {
    faults(
        "core_s_02_3",
        &[
            "privilege: clear CR4.TSD, then read the time counter",
            "privilege: load other page tables (CR3)",
            "privilege: turn write protect off (CR0)",
            "privilege: load a descriptor table (LGDT)",
            "privilege: load an interrupt table (LIDT)",
            "privilege: write a model-specific register (WRMSR)",
            "privilege: halt (HLT)",
        ],
    );
}

#[test]
fn core_s_02_4_e1_changing_its_own_code_faults() {
    faults("core_s_02_4_e1", &["memory: change its own code"]);
}

#[test]
fn core_s_02_4_e2_the_control_probe_reaches_the_monitor() {
    let Some(kernel) = selftest_kvm_backend("core_s_02_4_e2") else {
        return;
    };
    let (got, want) = probe(&kernel, "memory: nothing (control: must survive)");
    assert_eq!(
        (got, want),
        (Err(Reason::BadHypercall), Reason::BadHypercall)
    );
}

/// Every probe the self-test kernel held before this slice: the control survives and each
/// memory probe faults.
#[test]
fn core_s_02_4_every_memory_probe_still_faults() {
    let Some(kernel) = selftest_kvm_backend("core_s_02_4") else {
        return;
    };
    let memory: Vec<_> = PROBES
        .iter()
        .filter(|(name, _)| name.starts_with("memory: "))
        .collect();
    assert_eq!(memory.len(), 8);
    for &&(name, want) in &memory {
        assert_eq!(probe(&kernel, name), (Err(want), want), "{name}");
    }
}

/// Every probe on the list ends as it says, and every privilege probe is a fault. A randomness
/// probe may instead run on a host the host probe did not measure as blocking it (ADR-009), judged
/// as Slice 3's tests are.
#[test]
fn core_s_02_every_probe_ends_as_listed() {
    let Some(kernel) = selftest_kvm_backend("core_s_02") else {
        return;
    };
    let found = crate::cpuid_tests::measured_host("core_s_02");
    for &(name, want) in PROBES {
        assert!(
            want == Reason::KernelFault || name.starts_with("memory: nothing"),
            "{name}"
        );
        let got = probe(&kernel, name).0;
        assert_ne!(
            judge(name, got, want, blocked_here(name, Some(&found))),
            Verdict::Fail,
            "{name}: {got:?}"
        );
    }
}

/// The raw hostile kernels the `attacks` command runs end as they did on the ring 0 kernel. Since
/// the holder keeps the deadline (Slice 6), the one that spins until it is here too: before, the
/// deadline's signal went to the process, and in a test harness it reached another thread.
#[cfg(feature = "test-switches")]
#[test]
fn core_s_02_every_hostile_kernel_ends_as_before_in_ring_3() {
    if kvm_backend("core_s_02 hostile kernels").is_none() {
        return;
    }
    let inputs = read_inputs(
        root()
            .join("evidence/fixtures/issues.tsv")
            .to_str()
            .unwrap(),
    )
    .unwrap();
    for (name, kernel, want) in hostile_kernels() {
        let hostile = Backend::Kvm {
            kernel,
            limit: Duration::from_secs(1),
        };
        assert_eq!(
            execute(&hostile, &[], inputs.clone(), Mode::Record { seed: 0 }).map(|_| ()),
            Err(want),
            "{name}"
        );
    }
}
