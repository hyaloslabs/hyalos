//! The host probe (S-03.3) and replay against `RDSEED` (S-03.4), from `docs/plans/hyalos-kernel-core`.
//!
//! The probe's report and exit status are tested with a faked host, so every ending is shown on any
//! machine, and on this host's KVM, where the probe must agree with how the self-test kernel meets
//! each instruction. `HYALOS_REQUIRE_KVM` turns a host without KVM into a failure.

use super::*;
use crate::cpuid_tests::measured_host;
use crate::kvm::Ending;
use crate::profile_tests::selftest_kvm_backend;
use std::process::Command;

/// The self-test kernel's bytes for a question `RDSEED` varies, and for one it does not touch
/// (`SEED_VARIES_QUESTION` and `SEED_IGNORED` in `kernel-image/src/main.rs`).
const SEED_VARIES_QUESTION: u8 = 0x81;
const SEED_IGNORED: u8 = 0x82;

/// A host whose machines end as `ending` says for each instruction, by name, with `nop` and `ud2`
/// for the two controls. Each sequence ends with the write to the port, which is cut off here.
fn faked(
    mut ending: impl FnMut(&str) -> Result<Ending, &'static str>,
) -> Result<probe_host::Found, String> {
    probe_host::measure(&mut |code| {
        let (instruction, port) = code.split_at(code.len() - 5);
        assert_eq!(
            port,
            [0x66, 0xba, 0x00, 0x10, 0xef],
            "every sequence ends at the port"
        );
        let name = match instruction {
            [0x90] => "nop",
            [0x0f, 0x0b] => "ud2",
            _ => {
                probe_host::INSTRUCTIONS
                    .iter()
                    .find(|(_, bytes)| *bytes == instruction)
                    .expect("a listed instruction")
                    .0
            }
        };
        ending(name)
    })
}

/// A host where the controls behave and each listed instruction ends as `blocked` says.
fn host_where(blocked: impl Fn(&str) -> bool) -> Result<probe_host::Found, String> {
    faked(|name| {
        Ok(match name {
            "nop" => Ending::Port,
            "ud2" => Ending::Fault,
            name if blocked(name) => Ending::Fault,
            _ => Ending::Port,
        })
    })
}

const NAMES: [&str; 5] = ["RDTSC", "RDTSCP", "RDRAND", "RDSEED", "RDPID"];

/// The self-test kernel's probe of each instruction the host probe reports.
fn kernel_probe(instruction: &str) -> &'static str {
    match instruction {
        "RDTSC" => "privilege: read the time counter (RDTSC)",
        "RDTSCP" => "privilege: read the time counter (RDTSCP)",
        _ => {
            HOST_DEPENDENT
                .iter()
                .find(|(_, i)| *i == instruction)
                .expect("a host-dependent probe")
                .0
        }
    }
}

/// On this host's KVM, the probe prints one line per instruction in its order, its exit status
/// follows the lines, and each verdict is how the self-test kernel, a real kernel image, meets the
/// same instruction: `blocked` where it faults, `reachable` where it reaches the unknown request.
/// The time counter is blocked on every host (S-02.2).
#[test]
fn core_s_03_3_the_probe_reports_each_instruction_as_the_kernel_meets_it() {
    let Some(kernel) = selftest_kvm_backend("core_s_03_3") else {
        return;
    };
    let found = measured_host("core_s_03_3");
    assert_eq!(
        found.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        NAMES
    );
    let (out, err, code) = probe_host::report(&Ok(found.clone()));
    assert_eq!(err, None);
    let lines: Vec<String> = found
        .iter()
        .map(|&(name, blocked)| format!("{name} {}", if blocked { "blocked" } else { "reachable" }))
        .collect();
    assert_eq!(out.lines().collect::<Vec<_>>(), lines);
    assert_eq!(code, u8::from(found.iter().any(|&(_, blocked)| !blocked)));
    for &(instruction, blocked) in &found {
        let name = kernel_probe(instruction);
        let index = PROBES.iter().position(|(n, _)| *n == name).unwrap();
        let got = execute(&kernel, &[index as u8], vec![], Mode::Record { seed: 0 }).map(|_| ());
        let want = if blocked {
            Reason::KernelFault
        } else {
            Reason::BadHypercall
        };
        assert_eq!(
            got,
            Err(want),
            "{instruction}: the probe and the kernel disagree"
        );
    }
    assert!(
        found[..2].iter().all(|&(_, blocked)| blocked),
        "the time counter is reachable"
    );
}

/// The built command, as an operator runs it: five lines on standard output, nothing on standard
/// error, and exit `0` when all are blocked or `1` when any is reachable.
#[test]
fn core_s_03_3_the_command_prints_five_lines_and_exits_0_or_1() {
    if selftest_kvm_backend("core_s_03_3 command").is_none() {
        return;
    }
    let program = env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("task-kernel");
    assert!(program.exists(), "the monitor is not built");
    let printed = Command::new(program)
        .arg("probe-host")
        .env_clear()
        .output()
        .unwrap();
    let out = String::from_utf8(printed.stdout).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 5, "{out}");
    let mut reachable = false;
    for (line, name) in lines.iter().zip(NAMES) {
        let verdict = line
            .strip_prefix(name)
            .and_then(|rest| rest.strip_prefix(' '));
        assert!(matches!(verdict, Some("blocked" | "reachable")), "{line}");
        reachable |= verdict == Some("reachable");
    }
    assert!(printed.stderr.is_empty());
    assert_eq!(printed.status.code(), Some(i32::from(reachable)));
}

#[test]
fn core_s_03_3_e1_a_host_where_every_instruction_faults_is_all_blocked_and_exits_0() {
    let (out, err, code) = probe_host::report(&host_where(|_| true));
    assert_eq!(
        out,
        "RDTSC blocked\nRDTSCP blocked\nRDRAND blocked\nRDSEED blocked\nRDPID blocked\n"
    );
    assert_eq!((err, code), (None, 0));
}

/// The development host's case, faked so it holds on any machine.
#[test]
fn core_s_03_3_e2_a_host_where_rdseed_runs_reports_it_reachable_and_exits_1() {
    let (out, err, code) = probe_host::report(&host_where(|name| name != "RDSEED"));
    assert_eq!(
        out,
        "RDTSC blocked\nRDTSCP blocked\nRDRAND blocked\nRDSEED reachable\nRDPID blocked\n"
    );
    assert_eq!((err, code), (None, 1));
}

/// Without the device, or without permission to open it, the probe names the cause, prints no
/// verdict and exits 2.
#[test]
fn core_s_03_3_e3_no_access_to_kvm_names_the_cause_and_exits_2() {
    let missing = c"/nonexistent/hyalos-probe/kvm";
    let (out, err, code) = probe_host::report(&probe_host::on_host(missing));
    assert_eq!((out.as_str(), code), ("", 2));
    assert_eq!(
        err.as_deref(),
        Some(
            "could not test: /nonexistent/hyalos-probe/kvm does not exist, so this host has no KVM to test"
        )
    );
    // A device this process may not open. Root may open it anyway, so then only the first case runs.
    let dir = env::temp_dir().join(format!("hyalos-probe-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let device = dir.join("kvm");
    fs::write(&device, b"").unwrap();
    fs::set_permissions(&device, std::os::unix::fs::PermissionsExt::from_mode(0o0)).unwrap();
    if fs::File::open(&device).is_err() {
        let path = std::ffi::CString::new(device.to_str().unwrap()).unwrap();
        let (out, err, code) = probe_host::report(&probe_host::on_host(&path));
        assert_eq!((out.as_str(), code), ("", 2));
        assert!(
            err.unwrap()
                .starts_with("could not test: no permission to open ")
        );
    }
    fs::remove_dir_all(&dir).unwrap();
}

/// A control that fails is exit 2 with no verdict, never `blocked`: a machine that cannot reach
/// the port would otherwise show every instruction blocked.
#[test]
fn core_s_03_3_a_failed_control_is_exit_2_and_never_blocked() {
    let every_machine_faults = faked(|_| Ok(Ending::Fault));
    let fault_unseen = faked(|_| Ok(Ending::Port));
    let broken_midway = faked(|name| match name {
        "nop" => Ok(Ending::Port),
        "RDSEED" => Err("KVM failed to run a probe machine"),
        _ => Ok(Ending::Fault),
    });
    for (found, cause) in [
        (
            every_machine_faults,
            "the control (a nop) did not reach the monitor, so nothing can be shown blocked",
        ),
        (
            fault_unseen,
            "the fault control (ud2) reached the monitor, so a fault cannot be seen",
        ),
        (broken_midway, "KVM failed to run a probe machine"),
    ] {
        let (out, err, code) = probe_host::report(&found);
        assert_eq!((out.as_str(), code), ("", 2));
        assert_eq!(err, Some(format!("could not test: {cause}")));
    }
}

/// Runs the self-test kernel's `RDSEED` question, or `None` where the host probe measured `RDSEED`
/// blocked: there the read faults before any question, which the function checks, so randomness
/// cannot reach a request at all.
fn rdseed_record(test: &str, kernel: &Backend, mode: u8) -> Option<Transcript> {
    let blocked = measured_host(test)
        .iter()
        .find(|(name, _)| *name == "RDSEED")
        .unwrap()
        .1;
    let made = execute(kernel, &[mode], vec![], Mode::Record { seed: 0 });
    if blocked {
        assert_eq!(
            made.map(|_| ()),
            Err(Reason::KernelFault),
            "RDSEED is blocked, yet the kernel asked"
        );
        return None;
    }
    let made = made.expect("the kernel reads RDSEED and asks");
    assert_eq!(
        (made.answers.len(), made.result_hex.as_str()),
        (1, "646f6e65"),
        "one question, result `done`"
    );
    Some(made)
}

/// A kernel that puts `RDSEED`'s value into its question asks a different question each run, and
/// replay refuses its record as diverged. One that reads it and changes nothing replays.
#[test]
fn core_s_03_4_randomness_cannot_change_a_verified_request() {
    let Some(kernel) = selftest_kvm_backend("core_s_03_4") else {
        return;
    };
    let Some(varied) = rdseed_record("core_s_03_4", &kernel, SEED_VARIES_QUESTION) else {
        return;
    };
    assert_eq!(
        replay(&kernel, &[SEED_VARIES_QUESTION], &varied),
        Err(Reason::ReplayDiverged)
    );
    let ignored = rdseed_record("core_s_03_4", &kernel, SEED_IGNORED).unwrap();
    assert_eq!(replay(&kernel, &[SEED_IGNORED], &ignored), Ok(()));
}

#[test]
fn core_s_03_4_e1_a_record_whose_question_rdseed_varied_is_refused_as_diverged() {
    let Some(kernel) = selftest_kvm_backend("core_s_03_4_e1") else {
        return;
    };
    let Some(record) = rdseed_record("core_s_03_4_e1", &kernel, SEED_VARIES_QUESTION) else {
        return;
    };
    // The premise: a second run asks another question, with the same result.
    let again = rdseed_record("core_s_03_4_e1", &kernel, SEED_VARIES_QUESTION).unwrap();
    assert_ne!(
        record.answers[0].question_sha256, again.answers[0].question_sha256,
        "RDSEED did not change the question"
    );
    assert_eq!(record.result_hex, again.result_hex);
    assert_eq!(
        replay(&kernel, &[SEED_VARIES_QUESTION], &record),
        Err(Reason::ReplayDiverged)
    );
}

#[test]
fn core_s_03_4_e2_a_kernel_that_reads_rdseed_and_changes_nothing_replays() {
    let Some(kernel) = selftest_kvm_backend("core_s_03_4_e2") else {
        return;
    };
    let Some(record) = rdseed_record("core_s_03_4_e2", &kernel, SEED_IGNORED) else {
        return;
    };
    assert_eq!(replay(&kernel, &[SEED_IGNORED], &record), Ok(()));
}

/// `RECORD.md` states that `RDSEED` may stay reachable, names the probe and its exits, and nowhere
/// says a taken-over kernel has no hardware randomness.
#[test]
fn core_4_1_the_record_format_states_that_rdseed_may_stay_reachable() {
    let record = include_str!("../../RECORD.md");
    assert!(record.contains("`RDSEED` may stay reachable"));
    assert!(record.contains("`task-kernel probe-host`"));
    assert!(record.contains(
        "does not claim that a taken-over kernel lacks hardware randomness on every host"
    ));
    let flat = record
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for claim in [
        "no hardware randomness",
        "without hardware randomness",
        "cannot reach hardware randomness",
        "has no randomness",
    ] {
        assert!(!flat.contains(claim), "RECORD.md claims: {claim}");
    }
}
