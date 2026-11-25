//! The host probe (S-03.3, `design.md` §2 of `docs/plans/hyalos-kernel-core`): which time and
//! randomness instructions this host's KVM leaves reachable to a taken-over kernel.
//!
//! Each instruction runs alone on a fresh machine with the real start state and CPUID list, then
//! writes to the monitor's port. A machine that reaches the port ran the instruction: `reachable`.
//! One that shuts down first faulted on it: `blocked`. Two controls run first, a `nop` that must
//! reach the port and a `ud2` that must fault, so a probe that cannot see either ending reports that
//! it could not test, and never `blocked`. A reachable instruction does not refuse a run (ADR-002,
//! ADR-009): replay still refuses a result it changed.

use crate::kvm::{self, Ending};
use std::ffi::CStr;

/// The instructions the probe reports, in the order it prints them, each as fixed machine code.
pub(crate) const INSTRUCTIONS: [(&str, &[u8]); 5] = [
    ("RDTSC", &[0x0f, 0x31]),
    ("RDTSCP", &[0x0f, 0x01, 0xf9]),
    ("RDRAND", &[0x48, 0x0f, 0xc7, 0xf0]), // rdrand rax
    ("RDSEED", &[0x48, 0x0f, 0xc7, 0xf8]), // rdseed rax
    ("RDPID", &[0xf3, 0x0f, 0xc7, 0xf8]),  // rdpid rax
];

/// The control that must reach the port, and the one that must fault.
const NOP: &[u8] = &[0x90];
const UD2: &[u8] = &[0x0f, 0x0b];

/// `mov dx, 0x1000` and `out dx, eax`: the kernel's one way to the monitor.
const TO_PORT: [u8; 5] = [0x66, 0xba, 0x00, 0x10, 0xef];

/// Where the probe finds KVM.
pub(crate) const KVM_DEVICE: &CStr = c"/dev/kvm";

/// One instruction followed by the write to the port.
fn sequence(instruction: &[u8]) -> Vec<u8> {
    [instruction, &TO_PORT].concat()
}

/// What a host showed: each instruction with `true` when it is blocked, in `INSTRUCTIONS`' order.
pub(crate) type Found = Vec<(&'static str, bool)>;

/// Runs the controls and then each instruction through `run`, which boots one machine per call.
/// Anything but a clean ending, and any control that ends the wrong way, is a cause, not a verdict.
pub(crate) fn measure(
    run: &mut dyn FnMut(&[u8]) -> Result<Ending, &'static str>,
) -> Result<Found, String> {
    if run(&sequence(NOP))? != Ending::Port {
        return Err(
            "the control (a nop) did not reach the monitor, so nothing can be shown blocked".into(),
        );
    }
    if run(&sequence(UD2))? != Ending::Fault {
        return Err(
            "the fault control (ud2) reached the monitor, so a fault cannot be seen".into(),
        );
    }
    INSTRUCTIONS
        .iter()
        .map(|&(name, code)| Ok((name, run(&sequence(code))? == Ending::Fault)))
        .collect()
}

/// Measures this host through the KVM device at `device`.
pub(crate) fn on_host(device: &CStr) -> Result<Found, String> {
    let shown = device.to_string_lossy();
    let kvm = kvm_ioctls::Kvm::new_with_path(device).map_err(|error| match error.errno() {
        libc::ENOENT => format!("{shown} does not exist, so this host has no KVM to test"),
        libc::EACCES | libc::EPERM => format!("no permission to open {shown}"),
        _ => format!("cannot open {shown}"),
    })?;
    measure(&mut |code| kvm::probe(&kvm, code))
}

/// What the command prints on standard output and standard error, and its exit status: `0` when
/// every instruction is blocked, `1` when any is reachable, `2` when it could not test.
pub(crate) fn report(found: &Result<Found, String>) -> (String, Option<String>, u8) {
    match found {
        Ok(found) => {
            let lines: String = found
                .iter()
                .map(|&(name, blocked)| {
                    format!("{name} {}\n", if blocked { "blocked" } else { "reachable" })
                })
                .collect();
            let reachable = found.iter().any(|&(_, blocked)| !blocked);
            (lines, None, u8::from(reachable))
        }
        Err(cause) => (String::new(), Some(format!("could not test: {cause}")), 2),
    }
}

/// `task-kernel probe-host`.
pub(crate) fn command() -> u8 {
    let (out, err, code) = report(&on_host(KVM_DEVICE));
    print!("{out}");
    if let Some(err) = err {
        eprintln!("{err}");
    }
    code
}
