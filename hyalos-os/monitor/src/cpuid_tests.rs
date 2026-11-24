//! The machine sees only the allowed CPUID list (S-03, `design.md` §2 of
//! `docs/plans/hyalos-kernel-core`).
//!
//! The self-test kernel reads CPUID from inside the machine and returns what it saw as the run's
//! result, so these tests check the list the kernel really sees, not only the one the monitor
//! builds. They then run `RDRAND` and `RDPID`, which KVM makes fault once their bits are clear on
//! a host that can trap them, and which run elsewhere (ADR-009 of the same pack).
//! `HYALOS_REQUIRE_KVM` turns a host without KVM into a failure.

use super::*;
use crate::profile_tests::selftest_kvm_backend;
use std::arch::x86_64::__cpuid_count;

/// The input byte that asks the self-test kernel for its CPUID report (`CPUID_REPORT` in
/// `kernel-image/src/main.rs`), and the leaves it reads, in order, each at subleaf 0.
const CPUID_REPORT: u8 = 0x80;
const REPORTED_LEAVES: [u32; 14] =
    [0, 1, 4, 7, 0xb, 0xd, 0x4000_0000, 0x4000_0001, 0x8000_0000, 0x8000_0001, 0x8000_0002, 0x8000_0003, 0x8000_0004, 0x8000_0008];
const BRAND_LEAVES: [u32; 3] = [0x8000_0002, 0x8000_0003, 0x8000_0004];

const PAE: u32 = 1 << 6; // leaf 1, EDX
const RDRAND: u32 = 1 << 30; // leaf 1, ECX
const RDSEED: u32 = 1 << 18; // leaf 7, EBX
const RDPID: u32 = 1 << 22; // leaf 7, ECX
const RDTSCP: u32 = 1 << 27; // leaf 0x8000_0001, EDX
const NX: u32 = 1 << 20; // leaf 0x8000_0001, EDX
const LONG_MODE: u32 = 1 << 29; // leaf 0x8000_0001, EDX

/// `EAX`, `EBX`, `ECX` and `EDX` of one leaf.
type Leaf = [u32; 4];

/// What CPUID shows the kernel inside the machine, by leaf.
fn seen(test: &str) -> Option<Vec<(u32, Leaf)>> {
    let kernel = selftest_kvm_backend(test)?;
    let report = execute(&kernel, &[CPUID_REPORT], vec![], Mode::Record { seed: 0 }).expect("the CPUID report ends with a result");
    let bytes = unhex(&report.result_hex).unwrap();
    assert_eq!(bytes.len(), REPORTED_LEAVES.len() * 16);
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    Some(REPORTED_LEAVES.iter().enumerate().map(|(i, &leaf)| (leaf, [0, 1, 2, 3].map(|w| word(i * 16 + w * 4)))).collect())
}

fn leaf(seen: &[(u32, Leaf)], number: u32) -> Leaf {
    seen.iter().find(|(n, _)| *n == number).unwrap().1
}

/// The host's own words for a leaf, from outside the machine.
fn host(number: u32) -> Leaf {
    let r = __cpuid_count(number, 0);
    [r.eax, r.ebx, r.ecx, r.edx]
}

/// Runs one self-test probe by name and returns how it ended.
fn probe(test: &str, name: &str) -> Option<Result<(), Reason>> {
    let kernel = selftest_kvm_backend(test)?;
    let index = PROBES.iter().position(|(n, _)| *n == name).unwrap_or_else(|| panic!("no probe {name}"));
    assert_eq!(PROBES[index].1, Reason::KernelFault, "{name}");
    Some(execute(&kernel, &[index as u8], vec![], Mode::Record { seed: 0 }).map(|_| ()))
}

/// Every leaf the kernel reads holds only the allowed list: the two highest leaf numbers and the
/// three start-state bits. A leaf outside the list reads as zero or as leaf `1`'s filtered data,
/// which is how KVM answers a leaf past the highest, so nothing in it is the host's.
#[test]
fn core_s_03_1_the_kernel_sees_only_the_allowed_list() {
    let Some(seen) = seen("core_s_03_1") else { return };
    assert_eq!(leaf(&seen, 0), [1, 0, 0, 0], "highest basic leaf 1, no vendor");
    assert_eq!(leaf(&seen, 1), [0, 0, 0, PAE], "leaf 1: only PAE");
    assert_eq!(leaf(&seen, 0x8000_0000), [0x8000_0001, 0, 0, 0], "highest extended leaf");
    let extended = leaf(&seen, 0x8000_0001);
    assert_eq!(extended[..3], [0, 0, 0]);
    assert_eq!(extended[3] & !(NX | LONG_MODE), 0, "leaf 0x8000_0001: only long mode and NX");
    for &(number, words) in &seen {
        if ![0, 1, 0x8000_0000, 0x8000_0001].contains(&number) {
            assert!(words == [0; 4] || words == leaf(&seen, 1), "leaf {number:#x} holds {words:x?}");
        }
    }
    assert_eq!(leaf(&seen, 1)[2] & RDRAND, 0, "RDRAND");
    assert_eq!(leaf(&seen, 7)[1] & RDSEED, 0, "RDSEED");
    assert_eq!(leaf(&seen, 7)[2] & RDPID, 0, "RDPID");
    assert_eq!(extended[3] & RDTSCP, 0, "RDTSCP");
    // Nothing that names the host reaches the machine: not its vendor, model or brand.
    let named: Vec<u32> = [host(0)[1..].to_vec(), vec![host(1)[0]], BRAND_LEAVES.iter().flat_map(|&n| host(n)).collect()].concat();
    for &(number, words) in &seen {
        for word in words {
            assert!(word == 0 || !named.contains(&word), "leaf {number:#x} holds the host's {word:#x}");
        }
    }
}

#[test]
fn core_s_03_1_e1_leaf_1_shows_no_rdrand() {
    let Some(seen) = seen("core_s_03_1_e1") else { return };
    assert_eq!(leaf(&seen, 1)[2] & RDRAND, 0);
}

/// The brand leaves are past the highest extended leaf, so KVM answers them with leaf `1`'s
/// filtered data, `PAE` alone. They never hold the host's brand string.
#[test]
fn core_s_03_1_e2_the_brand_leaves_hold_no_host_name() {
    let Some(seen) = seen("core_s_03_1_e2") else { return };
    let brand: Vec<u8> = BRAND_LEAVES.iter().flat_map(|&n| leaf(&seen, n)).flat_map(u32::to_le_bytes).collect();
    let host_brand: Vec<u8> = BRAND_LEAVES.iter().flat_map(|&n| host(n)).flat_map(u32::to_le_bytes).collect();
    assert!(brand.iter().all(|&b| b == 0 || b == PAE as u8), "{brand:x?}");
    assert_ne!(brand, host_brand);
    assert!(!brand.iter().any(u8::is_ascii_alphanumeric), "the brand leaves spell something: {brand:x?}");
}

/// Writes the host's CPU to the test output, past the harness's capture, so a CI log shows which
/// host a randomness probe ran on.
pub(crate) fn note_host(test: &str) -> Option<bool> {
    use std::io::Write;
    let (vendor, module) = host_cpu();
    let traps = traps_randomness(&vendor, module);
    let brand: String = BRAND_LEAVES.iter().flat_map(|&n| host(n)).flat_map(u32::to_le_bytes).filter(|&b| b != 0).map(char::from).collect();
    let claim = match traps {
        Some(true) => "RDRAND and RDPID must fault",
        Some(false) => "KVM cannot trap RDRAND or RDPID here",
        None => "cannot tell whether KVM traps RDRAND or RDPID, so a run is not a failure and nothing is proven",
    };
    let module = module.unwrap_or("KVM module unknown");
    let _ = writeln!(std::io::stderr(), "{test}: host {vendor} ({}), {module}: {claim}", brand.trim());
    traps
}

/// Runs a randomness probe and judges it on this host: a fault where the host traps it, and
/// otherwise a fault or the stated limit (ADR-009). Returns whether the fault was shown.
fn randomness(test: &str, name: &str) -> Option<bool> {
    let got = probe(test, name)?;
    let traps = note_host(test);
    match judge(name, got, Reason::KernelFault, traps) {
        Verdict::Pass => Some(true),
        Verdict::Reachable => Some(false),
        Verdict::Fail => panic!("{name}: {got:?} on a host where KVM traps it"),
    }
}

#[test]
fn core_s_03_2_rdrand_and_rdpid_fault_where_the_host_traps_them() {
    for name in ["randomness: hardware randomness (RDRAND)", "randomness: read the processor ID (RDPID)"] {
        randomness("core_s_03_2", name);
    }
}

#[test]
fn core_s_03_2_e1_rdrand_faults_where_the_host_traps_it() {
    randomness("core_s_03_2_e1", "randomness: hardware randomness (RDRAND)");
}

#[test]
fn core_s_03_2_e2_rdpid_faults_where_the_host_traps_it() {
    randomness("core_s_03_2_e2", "randomness: read the processor ID (RDPID)");
}

/// Only Intel with `kvm_intel` is claimed to trap; AMD with `kvm_amd` is known not to; anything
/// else, including a vendor and module that disagree, claims nothing.
#[test]
fn core_s_03_2_only_intel_kvm_is_claimed_to_trap() {
    assert_eq!(traps_randomness("GenuineIntel", Some("kvm_intel")), Some(true));
    assert_eq!(traps_randomness("AuthenticAMD", Some("kvm_amd")), Some(false));
    assert_eq!(traps_randomness("HygonGenuine", Some("kvm_amd")), Some(false));
    for (vendor, module) in [("GenuineIntel", None), ("GenuineIntel", Some("kvm_amd")), ("AuthenticAMD", Some("kvm_intel")), ("AuthenticAMD", None), ("", None), ("VIA VIA VIA ", Some("kvm_intel"))] {
        assert_eq!(traps_randomness(vendor, module), None, "{vendor} {module:?}");
    }
}

/// A randomness probe that runs fails on a host that traps it, and is the stated limit elsewhere.
/// Every other probe, and any other ending, still fails on every host.
#[test]
fn core_s_03_2_a_run_is_reachable_only_where_the_host_cannot_trap_it() {
    let rdrand = "randomness: hardware randomness (RDRAND)";
    let ran = Err(Reason::BadHypercall);
    assert_eq!(judge(rdrand, Err(Reason::KernelFault), Reason::KernelFault, Some(true)), Verdict::Pass);
    assert_eq!(judge(rdrand, ran, Reason::KernelFault, Some(true)), Verdict::Fail);
    assert_eq!(judge(rdrand, ran, Reason::KernelFault, Some(false)), Verdict::Reachable);
    assert_eq!(judge(rdrand, ran, Reason::KernelFault, None), Verdict::Reachable);
    assert_eq!(judge(rdrand, Err(Reason::Timeout), Reason::KernelFault, None), Verdict::Fail);
    assert_eq!(judge(rdrand, Ok(()), Reason::KernelFault, None), Verdict::Fail);
    assert_eq!(judge("privilege: halt (HLT)", ran, Reason::KernelFault, Some(false)), Verdict::Fail);
    assert_eq!(HOST_DEPENDENT.len(), 2);
    for name in HOST_DEPENDENT {
        assert!(PROBES.iter().any(|(n, want)| n == name && *want == Reason::KernelFault), "{name}");
    }
}
