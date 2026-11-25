//! The monitor: it boots the bare-metal kernel on KVM and answers its hypercalls. It gives the
//! machine one virtual CPU, 32 MiB of memory and nothing else: no devices, no interrupts, no disk,
//! no network. It treats the kernel inside as untrusted, so a kernel taken over by a guest still
//! has only the five requests below, each checked here. The kernel runs in ring 3 with no ring 0
//! code beneath it (ADR-001 of `docs/plans/hyalos-kernel-core`), so it has no clock and no
//! privileged instruction, and any fault ends the machine. CPUID shows it only an allowed list, so
//! it learns nothing of the host. Whether `RDRAND`, `RDSEED` and `RDPID` then fault depends on the
//! host's KVM, and `probe` is how the host probe measures it (ADR-002 and ADR-009 of that pack).
//!
//! A run's machine is built and run only in the machine runner (`runner.rs`, ADR-003 of that
//! pack), which asks the holder for every answer through `Calls`. The host probe's fixed sequences
//! are the one machine built in the monitor's own process.

// One of the files allowed `unsafe` (design.md §2 of docs/plans/hyalos-kernel-core).
#![allow(unsafe_code)]

use crate::{MAX_RESULT, Reason};
use kvm_bindings::{
    CpuId, KVM_MAX_CPUID_ENTRIES, Msrs, kvm_cpuid_entry2, kvm_dtable, kvm_msr_entry, kvm_regs,
    kvm_segment, kvm_sregs, kvm_userspace_memory_region,
};
use kvm_ioctls::{Kvm, VcpuExit, VcpuFd, VmFd};
use std::{
    io::Write as _,
    ptr,
    time::{Duration, Instant},
};

const RAM: u64 = 32 << 20;
const KERNEL_LOW: u64 = 0x10_0000;
const KERNEL_HIGH: u64 = 0x70_0000;
const GUEST_AT: u64 = 0x70_0000;
pub(crate) const GUEST_MAX: u64 = 0x10_0000;
const PORT: u16 = 0x1000;
const APIC_BASE: u32 = 0x1b; // the `IA32_APIC_BASE` model-specific register
// The only memory the monitor reads or writes on the kernel's behalf. A request block lives on the
// kernel's stack or heap, and a buffer in the heap (where the guest's memory is). The monitor's own
// view of memory ignores the kernel's page tables, so without this a request could aim the monitor
// at the kernel's code, the signed guest or the page tables.
const HEAP: std::ops::Range<u64> = 0x80_0000..0x180_0000;
const STACK: std::ops::Range<u64> = 0x1e0_0000..RAM;

fn within(range: &std::ops::Range<u64>, at: u64, len: u64) -> bool {
    at >= range.start && at.checked_add(len).is_some_and(|end| end <= range.end)
}

struct Ram(*mut u8);

impl Ram {
    fn new() -> Option<Self> {
        let flags = libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE;
        // SAFETY: a fresh anonymous mapping, owned by this value until it is dropped.
        let at = unsafe {
            libc::mmap(
                ptr::null_mut(),
                RAM as usize,
                libc::PROT_READ | libc::PROT_WRITE,
                flags,
                -1,
                0,
            )
        };
        if at == libc::MAP_FAILED {
            return None;
        }
        // Test builds only (S-05.4).
        #[cfg(feature = "test-switches")]
        if crate::switches::huge() {
            // SAFETY: advice on the mapping just made; failure only costs speed.
            unsafe { libc::madvise(at, RAM as usize, libc::MADV_HUGEPAGE) };
        }
        Some(Self(at.cast()))
    }

    fn range(at: u64, len: u64) -> Option<(usize, usize)> {
        (at.checked_add(len)? <= RAM).then_some((at as usize, len as usize))
    }

    fn read(&self, at: u64, len: u64) -> Option<Vec<u8>> {
        let (at, len) = Self::range(at, len)?;
        // SAFETY: the range is inside the mapping, and the virtual CPU is not running.
        Some(unsafe { std::slice::from_raw_parts(self.0.add(at), len) }.to_vec())
    }

    fn write(&self, at: u64, bytes: &[u8]) -> Option<()> {
        let (at, len) = Self::range(at, bytes.len() as u64)?;
        // SAFETY: as in `read`.
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), self.0.add(at), len) };
        Some(())
    }
}

// SAFETY: the mapping is owned by this value alone and is only freed once, wherever it is dropped.
unsafe impl Send for Ram {}

impl Drop for Ram {
    fn drop(&mut self) {
        // SAFETY: the mapping made in `new`.
        unsafe { libc::munmap(self.0.cast(), RAM as usize) };
    }
}

extern "C" fn tick(_: libc::c_int) {}

/// While this lives, a signal interrupts the virtual CPU every 10 ms so the deadline is checked.
struct Ticker;

impl Ticker {
    fn set(micros: i64) {
        let every = libc::timeval {
            tv_sec: 0,
            tv_usec: micros,
        };
        let timer = libc::itimerval {
            it_interval: every,
            it_value: every,
        };
        // SAFETY: plain libc calls with valid arguments; the handler does nothing.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = tick as extern "C" fn(libc::c_int) as usize;
            libc::sigaction(libc::SIGALRM, &action, ptr::null_mut());
            libc::setitimer(libc::ITIMER_REAL, &timer, ptr::null_mut());
        }
    }

    fn start() -> Self {
        Self::set(10_000);
        Self
    }
}

impl Drop for Ticker {
    fn drop(&mut self) {
        Self::set(0);
    }
}

fn field(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

/// A loaded segment: its first address, the address after it, and its ELF flags.
type Segment = (u64, u64, u32);

/// Copies the kernel image into memory and returns its entry point. An image is an ELF file. In a
/// build with the `test-switches` feature it may also be raw machine code placed at the kernel's
/// base, for the hostile-kernel tests; a release build refuses that (`None`, so `KernelFault`), so a
/// replaced kernel file never gets writable, executable memory (S-05.4, P7).
fn load(kernel: &[u8], ram: &Ram) -> Option<(u64, Vec<Segment>)> {
    let mut segments = Vec::new();
    if !kernel.starts_with(b"\x7fELF") {
        #[cfg(not(feature = "test-switches"))]
        return None;
        #[cfg(feature = "test-switches")]
        {
            (kernel.len() as u64 <= KERNEL_HIGH - KERNEL_LOW).then_some(())?;
            ram.write(KERNEL_LOW, kernel)?;
            // A raw image stands for an attacker who already runs code in the kernel: give it everything.
            return Some((KERNEL_LOW, vec![(KERNEL_LOW, KERNEL_HIGH, 7)]));
        }
    }
    let entry = field(kernel, 0x18)?;
    let table = field(kernel, 0x20)? as usize;
    let size = usize::from(u16::from_le_bytes(kernel.get(0x36..0x38)?.try_into().ok()?));
    let count = usize::from(u16::from_le_bytes(kernel.get(0x38..0x3a)?.try_into().ok()?));
    for index in 0..count {
        let header = kernel.get(table.checked_add(index.checked_mul(size)?)?..)?;
        if header.get(..4)? != 1u32.to_le_bytes() {
            continue;
        }
        let (offset, at) = (field(header, 0x08)? as usize, field(header, 0x18)?);
        let (in_file, in_memory) = (field(header, 0x20)? as usize, field(header, 0x28)?);
        if at < KERNEL_LOW || at.checked_add(in_memory)? > KERNEL_HIGH {
            return None;
        }
        ram.write(at, kernel.get(offset..offset.checked_add(in_file)?)?)?;
        let flags = u32::from_le_bytes(header.get(4..8)?.try_into().ok()?);
        segments.push((at, at + in_memory, flags));
    }
    (KERNEL_LOW..KERNEL_HIGH)
        .contains(&entry)
        .then_some((entry, segments))
}

// Page entry bits: present, writable, reachable from ring 3, a 2 MiB page, and not executable.
const PRESENT: u64 = 1;
const WRITABLE: u64 = 2;
const USER: u64 = 4;
const LARGE: u64 = 0x80;
const NX: u64 = 1 << 63;

/// Builds the kernel's address space. Code is read-only, everything else cannot be executed, and
/// what the kernel has no business touching is not mapped at all: the first megabyte (which holds
/// these page tables), the gap below the stack, and anything past the machine's memory. Every
/// entry on the way to a page the kernel may reach carries the user bit, since the kernel runs in
/// ring 3. The rights are the ones it had in ring 0.
fn map(ram: &Ram, segments: &[Segment], guest_len: u64) -> Option<()> {
    let mut small = vec![0u64; 4 * 512]; // 4 KiB pages for the first 8 MiB: kernel image and guest
    let mut page = |at: u64, entry: u64| {
        let slot = small.get_mut((at >> 12) as usize)?;
        // Two segments wanting one page with different rights is refused, not resolved.
        (*slot == 0 || *slot == at | entry).then(|| *slot = at | entry)
    };
    for &(start, end, flags) in segments {
        let entry = PRESENT
            | USER
            | if flags & 2 != 0 { WRITABLE } else { 0 }
            | if flags & 1 != 0 { 0 } else { NX };
        for at in ((start & !0xfff)..end).step_by(4096) {
            page(at, entry)?;
        }
    }
    for at in (GUEST_AT..GUEST_AT + guest_len).step_by(4096) {
        page(at, PRESENT | USER | NX)?; // the guest's bytes: read-only
    }
    let table = PRESENT | WRITABLE | USER;
    let mut large = vec![0u64; 512];
    for (index, slot) in large.iter_mut().enumerate().take((RAM >> 21) as usize) {
        *slot = match index {
            0..4 => (0x4000 + index as u64 * 0x1000) | table,
            4..12 | 15 => ((index as u64) << 21) | table | LARGE | NX, // heap, and the stack at the top
            _ => 0,                                                    // the guard below the stack
        };
    }
    let bytes = |values: &[u64]| {
        values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<u8>>()
    };
    ram.write(0x1000, &bytes(&[0x2000 | table]))?;
    ram.write(0x2000, &bytes(&[0x3000 | table]))?;
    ram.write(0x3000, &bytes(&large))?;
    ram.write(0x4000, &bytes(&small))
}

/// The machine's start state (`design.md` §2 of `docs/plans/hyalos-kernel-core`): 64-bit ring 3,
/// paging from the tables `map` built, `CR4.TSD` so the time counter faults, `IOPL` 3 so the
/// kernel's one `out` reaches the monitor, and empty descriptor tables, so a fault has no handler
/// and shuts the machine down.
fn start_state(sregs: &mut kvm_sregs, entry: u64, guest_len: u64) -> kvm_regs {
    const CR0: u64 = 0x8005_0033; // paging, write protect, protected mode and the FPU bits
    const CR4_TSD: u64 = 1 << 2;
    const CR4_PAE: u64 = 1 << 5;
    const EFER: u64 = 0xd00; // long mode, active, and no-execute
    const IOPL_3: u64 = 3 << 12;
    let code = kvm_segment {
        limit: 0xffff_ffff,
        selector: 8 | 3,
        type_: 11,
        present: 1,
        dpl: 3,
        s: 1,
        l: 1,
        g: 1,
        ..Default::default()
    };
    let data = kvm_segment {
        selector: 16 | 3,
        type_: 3,
        db: 1,
        l: 0,
        ..code
    };
    sregs.cs = code;
    (sregs.ds, sregs.es, sregs.fs, sregs.gs, sregs.ss) = (data, data, data, data, data);
    (sregs.cr0, sregs.cr3, sregs.cr4, sregs.efer) = (CR0, 0x1000, CR4_PAE | CR4_TSD, EFER);
    let none = kvm_dtable {
        base: 0,
        limit: 0,
        ..Default::default()
    };
    (sregs.gdt, sregs.idt) = (none, none);
    // Bit 1 of `RFLAGS` is always set. Interrupts stay off.
    kvm_regs {
        rip: entry,
        rsp: RAM - 8,
        rflags: IOPL_3 | 2,
        rdi: GUEST_AT,
        rsi: guest_len,
        ..Default::default()
    }
}

/// The only CPUID the machine sees (`design.md` §2 of `docs/plans/hyalos-kernel-core`, the CPUID
/// allow-list): four leaves, holding the highest basic and extended leaf numbers and the three
/// feature bits the start state needs, `PAE`, long mode and `NX`, each only if the host has it.
/// Every other bit and leaf reads as zero, or as leaf `1`'s filtered data where KVM answers a leaf
/// past the highest one with it. So the vendor, model, brand and topology stay hidden, and with the
/// `RDRAND`, `RDSEED`, `RDTSCP` and `RDPID` bits clear, a host whose KVM can trap them makes them
/// fault.
fn allowed_cpuid(supported: &[kvm_cpuid_entry2]) -> Vec<kvm_cpuid_entry2> {
    const PAE: u32 = 1 << 6; // leaf 1, EDX
    const NX: u32 = 1 << 20; // leaf 0x8000_0001, EDX
    const LONG_MODE: u32 = 1 << 29; // leaf 0x8000_0001, EDX
    let edx = |function: u32| {
        supported
            .iter()
            .find(|e| e.function == function && e.index == 0)
            .map_or(0, |e| e.edx)
    };
    let leaf = |function: u32, eax: u32, edx: u32| kvm_cpuid_entry2 {
        function,
        eax,
        edx,
        ..Default::default()
    };
    vec![
        leaf(0, 1, 0),
        leaf(1, 0, edx(1) & PAE),
        leaf(0x8000_0000, 0x8000_0001, 0),
        leaf(0x8000_0001, 0, edx(0x8000_0001) & (NX | LONG_MODE)),
    ]
}

enum Step {
    Reply(i64),
    Done(Vec<u8>, u64),
}

/// The heap's size: the most bytes a record written into the kernel's memory can have.
pub(crate) const RECORD_MAX: u64 = HEAP.end - HEAP.start;

/// The host side of the five requests, as `answer` asks it. In the machine runner every call is a
/// message to the holder (`runner.rs`), which keeps `Host` and its rules; the fuzzer calls a `Host`
/// directly.
pub(crate) trait Calls {
    /// A record's length and its bytes, or `None` when there is no record. The bytes may be left out
    /// only when the record is longer than `RECORD_MAX`, since then it cannot be written anyway.
    fn input(&mut self, index: i32) -> Result<Option<(u64, Vec<u8>)>, Reason>;
    fn pre_infer(&mut self, schema: i32) -> Result<usize, Reason>;
    fn infer(&mut self, schema: i32, question: &[u8]) -> Result<i32, Reason>;
    fn pre_effect(&mut self, kind: i32) -> Result<usize, Reason>;
    fn effect(&mut self, kind: i32, body: &[u8]) -> Result<(), Reason>;
}

impl<S: hyalos_audit::Source> Calls for hyalos_audit::Host<S> {
    fn input(&mut self, index: i32) -> Result<Option<(u64, Vec<u8>)>, Reason> {
        Ok(hyalos_audit::Host::input(self, index)
            .map(|record| (record.len() as u64, record.to_vec())))
    }
    fn pre_infer(&mut self, schema: i32) -> Result<usize, Reason> {
        hyalos_audit::Host::pre_infer(self, schema)
    }
    fn infer(&mut self, schema: i32, question: &[u8]) -> Result<i32, Reason> {
        hyalos_audit::Host::infer(self, schema, question)
    }
    fn pre_effect(&mut self, kind: i32) -> Result<usize, Reason> {
        hyalos_audit::Host::pre_effect(self, kind)
    }
    fn effect(&mut self, kind: i32, body: &[u8]) -> Result<(), Reason> {
        hyalos_audit::Host::effect(self, kind, body)
    }
}

fn answer(ram: &Ram, host: &mut impl Calls, at: u64) -> Result<Step, Reason> {
    if !within(&HEAP, at, 48) && !within(&STACK, at, 48) {
        return Err(Reason::BadPointer);
    }
    let request = ram.read(at, 48).ok_or(Reason::BadPointer)?;
    let word = |index: usize| field(&request, index * 8).unwrap_or(0);
    let (op, a, b, c, d) = (word(0), word(1), word(2), word(3), word(4));
    let bytes = |at: u64, len: u64, max: usize, inside: u64| {
        if len > max as u64 {
            return Err(Reason::TooLarge);
        }
        if inside == 0 || !within(&HEAP, at, len) {
            return Err(Reason::BadPointer);
        }
        ram.read(at, len).ok_or(Reason::BadPointer)
    };
    match op {
        1 => {
            let Some((len, record)) = host.input(a as u32 as i32)? else {
                return Ok(Step::Reply(-1));
            };
            if c < len {
                return Ok(Step::Reply(-2));
            }
            if d < len || !within(&HEAP, b, len) || record.len() as u64 != len {
                return Err(Reason::BadPointer);
            }
            ram.write(b, &record).ok_or(Reason::BadPointer)?;
            Ok(Step::Reply(len as i64))
        }
        2 => {
            let schema = a as u32 as i32;
            let max = host.pre_infer(schema)?;
            let question = bytes(b, c, max, d)?;
            Ok(Step::Reply(i64::from(host.infer(schema, &question)?)))
        }
        3 => {
            let kind = a as u32 as i32;
            let max = host.pre_effect(kind)?;
            // TEMPORARY (Slice 9 dry run, S-05.5.e2): a handler panic CI's fuzz step must fail on.
            assert!(b < u64::MAX - 64, "planted");
            let body = bytes(b, c, max, d)?;
            host.effect(kind, &body)?;
            Ok(Step::Reply(0))
        }
        4 => Ok(Step::Done(bytes(a, b, MAX_RESULT, c)?, d)),
        5 => Err(match a {
            1 => Reason::InvalidModule,
            2 => Reason::BadImport,
            3 => Reason::BadExport,
            4 => Reason::OutOfFuel,
            5 => Reason::GuestTrap,
            6 => Reason::NoResult,
            _ => Reason::KernelFault,
        }),
        _ => Err(Reason::BadHypercall),
    }
}

/// A machine with `ram` as its memory, the CPUID allow-list, the APIC base off and the start
/// state, ready to run from `entry`. `ram` must outlive both handles it returns.
fn machine(kvm: &Kvm, ram: &Ram, entry: u64, guest_len: u64) -> Result<(VmFd, VcpuFd), Reason> {
    let no = |_| Reason::NoKvm;
    let vm = kvm.create_vm().map_err(no)?;
    let region = kvm_userspace_memory_region {
        slot: 0,
        flags: 0,
        guest_phys_addr: 0,
        memory_size: RAM,
        userspace_addr: ram.0 as u64,
    };
    // SAFETY: the mapping outlives the machine: every caller drops `ram` after `vm` and `vcpu`.
    unsafe { vm.set_user_memory_region(region) }.map_err(no)?;
    let vcpu = vm.create_vcpu(0).map_err(no)?;
    let supported = kvm.get_supported_cpuid(KVM_MAX_CPUID_ENTRIES).map_err(no)?;
    let cpuid =
        CpuId::from_entries(&allowed_cpuid(supported.as_slice())).map_err(|_| Reason::NoKvm)?;
    vcpu.set_cpuid2(&cpuid).map_err(no)?;
    // KVM shows leaf 1's `APIC` bit while the APIC base says enabled, though the machine has no
    // interrupt controller. Disabling it keeps the bit clear, so CPUID holds only the list.
    let apic_off = Msrs::from_entries(&[kvm_msr_entry {
        index: APIC_BASE,
        data: 0,
        ..Default::default()
    }])
    .map_err(|_| Reason::NoKvm)?;
    if vcpu.set_msrs(&apic_off).map_err(no)? != 1 {
        return Err(Reason::NoKvm);
    }

    // Start in 64-bit ring 3: no firmware, no boot loader, no 16-bit code, no ring 0 code.
    let mut sregs = vcpu.get_sregs().map_err(no)?;
    let regs = start_state(&mut sregs, entry, guest_len);
    vcpu.set_sregs(&sregs).map_err(no)?;
    vcpu.set_regs(&regs).map_err(no)?;
    Ok((vm, vcpu))
}

/// How a host-probe machine ended (`design.md` §2 of `docs/plans/hyalos-kernel-core`, the host
/// probe).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ending {
    /// It reached the monitor's port, so everything before the `out` ran.
    Port,
    /// It shut down first: something before the port faulted.
    Fault,
}

/// Runs `code` alone on a fresh machine with the real start state and CPUID list, as the kernel
/// image would run. This loader places the code itself, read-only and executable at the kernel's
/// base, so the probe never runs an image from outside. `Err` says why the machine could not tell.
pub(crate) fn probe(kvm: &Kvm, code: &[u8]) -> Result<Ending, &'static str> {
    let ram = Ram::new().ok_or("no memory for a probe machine")?;
    ram.write(KERNEL_LOW, code)
        .ok_or("a probe sequence does not fit")?;
    map(&ram, &[(KERNEL_LOW, KERNEL_LOW + code.len() as u64, 5)], 0)
        .ok_or("a probe sequence does not fit")?;
    let (_vm, mut vcpu) =
        machine(kvm, &ram, KERNEL_LOW, 0).map_err(|_| "KVM refused the machine")?;
    let (start, _ticker) = (Instant::now(), Ticker::start());
    loop {
        if start.elapsed() > Duration::from_secs(2) {
            return Err("a probe machine did not end in time");
        }
        match vcpu.run() {
            Ok(VcpuExit::IoOut(PORT, _)) => return Ok(Ending::Port),
            Ok(VcpuExit::Hlt | VcpuExit::Shutdown) => return Ok(Ending::Fault),
            Ok(_) => return Err("a probe machine ended neither at the port nor in a fault"),
            Err(error) if error.errno() == libc::EINTR => continue,
            Err(_) => return Err("KVM failed to run a probe machine"),
        }
    }
}

/// A built machine: its virtual CPU, the machine, and the memory both use, dropped in that order.
type Held = (VcpuFd, VmFd, Ram);

/// Boots `kernel` with `guest` and runs it to its end, asking `calls` for every request, then
/// hands the outcome to `ended`. This is the machine runner's half of a run (`runner.rs`): it holds
/// no deadline, since the holder kills the runner when the run's time is up. `kvm` was opened
/// before any byte of the run arrived, and its failure is reported only after the image is checked,
/// so every reason stays the one a run gave when the monitor ran the machine itself. Tearing a
/// machine down takes the host tens of milliseconds, so it happens only after `ended` has reported.
/// `trace` prints the timings the holder's trace switch asks for (a build with the `test-switches`
/// feature only), in the fields the monitor printed before.
/// `seal` installs the runner's seccomp filter once the machine is built, given its virtual CPU, and
/// says whether it holds: no guest instruction runs before it does, and a machine it fails for
/// never runs (`KernelFault`).
pub(crate) fn machine_run<C: Calls>(
    kvm: Option<Kvm>,
    kernel: &[u8],
    guest: &[u8],
    calls: &mut C,
    trace: bool,
    seal: impl FnOnce(&VcpuFd) -> bool,
    ended: impl FnOnce(&mut C, Result<(Vec<u8>, u64), Reason>),
) {
    let mut held = None;
    let outcome = boot_and_run(kvm, kernel, guest, calls, trace, seal, &mut held);
    ended(calls, outcome);
    drop(held);
}

fn boot_and_run(
    kvm: Option<Kvm>,
    kernel: &[u8],
    guest: &[u8],
    calls: &mut impl Calls,
    trace: bool,
    seal: impl FnOnce(&VcpuFd) -> bool,
    held: &mut Option<Held>,
) -> Result<(Vec<u8>, u64), Reason> {
    let t0 = Instant::now();
    if guest.len() as u64 > GUEST_MAX {
        return Err(Reason::TooLarge);
    }
    let ram = Ram::new().ok_or(Reason::NoKvm)?;
    let (entry, segments) = load(kernel, &ram).ok_or(Reason::KernelFault)?;
    ram.write(GUEST_AT, guest).ok_or(Reason::TooLarge)?;
    map(&ram, &segments, guest.len() as u64).ok_or(Reason::KernelFault)?;
    let t_mem = t0.elapsed();
    let kvm = kvm.ok_or(Reason::NoKvm)?;
    let (vm, vcpu) = machine(&kvm, &ram, entry, guest.len() as u64)?;
    // Sealed: from here the runner may make only the system calls its filter allows (`runner.rs`).
    if !seal(&vcpu) {
        return Err(Reason::KernelFault);
    }
    let t_vm = t0.elapsed();
    let (vcpu, _, ram) = held.insert((vcpu, vm, ram));
    let mut first = true;
    loop {
        let at = match vcpu.run() {
            Ok(VcpuExit::IoOut(PORT, data)) => match <[u8; 4]>::try_from(data) {
                Ok(bytes) => u64::from(u32::from_le_bytes(bytes)),
                Err(_) => return Err(Reason::BadHypercall),
            },
            // With no descriptor tables, any fault in ring 3 (a privileged instruction, the time
            // counter, a page it may not touch) becomes a triple fault and a shutdown.
            Ok(VcpuExit::Hlt | VcpuExit::Shutdown) => return Err(Reason::KernelFault),
            Ok(_) => return Err(Reason::BadHypercall),
            Err(error) if error.errno() == libc::EINTR => continue,
            Err(_) => return Err(Reason::KernelFault),
        };
        if trace && first {
            let ms = |d: Duration| d.as_secs_f64() * 1e3;
            // The holder reads this line back and prints it only if it has this exact form.
            let _ = writeln!(
                std::io::stderr(),
                "trace ms: memory+load {:.2}, vm setup {:.2}, boot to first call {:.2}",
                ms(t_mem),
                ms(t_vm - t_mem),
                ms(t0.elapsed() - t_vm)
            );
        }
        first = false;
        match answer(ram, calls, at)? {
            Step::Reply(value) => ram
                .write(at + 40, &value.to_le_bytes())
                .ok_or(Reason::BadPointer)?,
            Step::Done(result, fuel) => return Ok((result, fuel)),
        }
    }
}

/// Throws random requests at the monitor's request handler, as a kernel under an attacker's
/// control would, and counts what came back. A panic or a crash here is a bug in the wall itself.
pub(crate) fn fuzz(mut host: impl FnMut() -> crate::Host, rounds: u64) -> Option<(u64, u64)> {
    let ram = Ram::new()?;
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let (mut replied, mut refused) = (0, 0);
    for _ in 0..rounds {
        // Bias every field towards the edges where bounds checks live.
        let mut edge = |limit: u64| match next() % 6 {
            0 => next(),
            1 => limit.wrapping_sub(next() % 64),
            2 => limit.wrapping_add(next() % 64),
            3 => u64::MAX - next() % 64,
            4 => next() % 64,
            _ => next() % limit.max(1),
        };
        let (wide, at) = (edge(8), edge(RAM));
        // Most blocks and half the buffers land in the heap, so the checks behind the range check
        // are reached too.
        let span = HEAP.end - HEAP.start;
        let at = if wide % 4 == 0 {
            at
        } else {
            HEAP.start + at % span
        };
        let buffer = edge(RAM);
        let buffer = if wide % 2 == 0 {
            buffer
        } else {
            HEAP.start + edge(span) % (span + 64)
        };
        let words = [edge(7) % 7, edge(4), buffer, edge(70_000), edge(RAM), 0];
        let block: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let _ = ram.write(at, &block);
        match answer(&ram, &mut host(), at) {
            Ok(_) => replied += 1,
            Err(_) => refused += 1,
        }
    }
    Some((replied, refused))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The start state is the one `design.md` §2 gives: ring 3 code and data, `CR4` with `PAE`
    /// and `TSD`, `IOPL` 3 with interrupts off, and no descriptor tables.
    #[test]
    fn core_s_02_the_machine_starts_in_ring_3_with_tsd_and_iopl_3() {
        let mut sregs = kvm_sregs::default();
        let regs = start_state(&mut sregs, 0x10_1000, 5);
        assert_eq!(
            (sregs.cs.selector, sregs.cs.dpl, sregs.cs.l, sregs.cs.type_),
            (11, 3, 1, 11)
        );
        for data in [sregs.ds, sregs.es, sregs.fs, sregs.gs, sregs.ss] {
            assert_eq!((data.selector, data.dpl, data.type_), (19, 3, 3));
        }
        assert_eq!(sregs.cr4, (1 << 5) | (1 << 2));
        assert_eq!((sregs.gdt.limit, sregs.idt.limit), (0, 0));
        assert_eq!(regs.rflags, 0x3002, "IOPL 3, interrupts off");
        assert_eq!((regs.rip, regs.rdi, regs.rsi), (0x10_1000, GUEST_AT, 5));
    }

    /// A host that offers every bit of every leaf, with a vendor and brand string of its own.
    fn generous_host() -> Vec<kvm_cpuid_entry2> {
        let all = |function: u32| kvm_cpuid_entry2 {
            function,
            eax: !0,
            ebx: !0,
            ecx: !0,
            edx: !0,
            ..Default::default()
        };
        let text = |function: u32, words: [&[u8; 4]; 4]| {
            let [eax, ebx, ecx, edx] = words.map(|w| u32::from_le_bytes(*w));
            kvm_cpuid_entry2 {
                function,
                eax,
                ebx,
                ecx,
                edx,
                ..Default::default()
            }
        };
        let mut host: Vec<_> = [
            1,
            4,
            7,
            0xb,
            0xd,
            0x4000_0000,
            0x4000_0001,
            0x8000_0001,
            0x8000_0008,
        ]
        .map(all)
        .to_vec();
        host.push(text(0, [b"\x20\0\0\0", b"Genu", b"ntel", b"ineI"]));
        host.push(text(
            0x8000_0000,
            [b"\x08\0\0\x80", b"\0\0\0\0", b"\0\0\0\0", b"\0\0\0\0"],
        ));
        host.push(text(0x8000_0002, [b"Inte", b"l(R)", b" Cor", b"e(TM"]));
        host.push(text(0x8000_0003, [b") i9", b"-139", b"00K ", b"CPU "]));
        host.push(text(
            0x8000_0004,
            [b"@ 3.", b"00GH", b"z\0\0\0", b"\0\0\0\0"],
        ));
        host
    }

    fn words(list: &[kvm_cpuid_entry2], function: u32) -> Option<[u32; 4]> {
        list.iter()
            .find(|e| e.function == function)
            .map(|e| [e.eax, e.ebx, e.ecx, e.edx])
    }

    /// From a host offering everything, the list keeps four leaves: the highest basic and
    /// extended leaf numbers, `PAE` in leaf 1, and long mode and `NX` in leaf `0x8000_0001`.
    #[test]
    fn core_s_03_1_the_list_holds_only_the_allowed_leaves_and_bits() {
        let list = allowed_cpuid(&generous_host());
        let leaves: Vec<_> = list
            .iter()
            .map(|e| (e.function, e.index, e.flags, e.padding))
            .collect();
        assert_eq!(
            leaves,
            [0, 1, 0x8000_0000, 0x8000_0001].map(|f| (f, 0, 0, [0; 3]))
        );
        assert_eq!(
            words(&list, 0),
            Some([1, 0, 0, 0]),
            "highest basic leaf 1, vendor zeroed"
        );
        assert_eq!(words(&list, 1), Some([0, 0, 0, 1 << 6]), "only PAE");
        assert_eq!(words(&list, 0x8000_0000), Some([0x8000_0001, 0, 0, 0]));
        assert_eq!(
            words(&list, 0x8000_0001),
            Some([0, 0, 0, (1 << 29) | (1 << 20)]),
            "only long mode and NX"
        );
        for hidden in [7, 0xb, 0xd, 0x4000_0000] {
            assert_eq!(words(&list, hidden), None, "leaf {hidden:#x}");
        }
    }

    /// Leaf 1's `RDRAND` bit is clear even when the host has it.
    #[test]
    fn core_s_03_1_e1_leaf_1_has_no_rdrand_bit() {
        let host = generous_host();
        assert_ne!(words(&host, 1).unwrap()[2] & (1 << 30), 0);
        assert_eq!(words(&allowed_cpuid(&host), 1).unwrap()[2] & (1 << 30), 0);
    }

    /// No brand leaf is given to the machine, and no word of the host's vendor or brand string
    /// appears anywhere in the list.
    #[test]
    fn core_s_03_1_e2_no_brand_leaf_and_no_host_name() {
        let host = generous_host();
        let list = allowed_cpuid(&host);
        let named: Vec<u32> = [0, 0x8000_0002, 0x8000_0003, 0x8000_0004]
            .iter()
            .flat_map(|&f| words(&host, f).unwrap()[1..].to_vec())
            .collect();
        for entry in &list {
            assert!(!(0x8000_0002..=0x8000_0004).contains(&entry.function));
            for word in [entry.eax, entry.ebx, entry.ecx, entry.edx] {
                // The message names nothing from the list, so no word of a host reaches the output.
                assert!(
                    word == 0 || !named.contains(&word),
                    "a leaf holds a word of the host's name"
                );
            }
        }
    }

    /// A bit the host lacks is not offered, so a host without long mode refuses the start state
    /// rather than pretending.
    #[test]
    fn core_s_03_1_a_bit_the_host_lacks_is_not_offered() {
        let host: Vec<_> = generous_host()
            .into_iter()
            .filter(|e| e.function != 0x8000_0001)
            .collect();
        assert_eq!(words(&allowed_cpuid(&host), 0x8000_0001), Some([0; 4]));
    }

    /// Every entry on the way to a reachable page carries the user bit with its old rights, and
    /// the page tables themselves stay unmapped.
    #[test]
    fn core_s_02_every_reachable_page_is_a_user_page_and_the_tables_are_unmapped() {
        let ram = Ram::new().unwrap();
        let segments = [
            (KERNEL_LOW, KERNEL_LOW + 0x2000, 5),
            (KERNEL_LOW + 0x2000, KERNEL_LOW + 0x3000, 6),
        ];
        map(&ram, &segments, 10).unwrap();
        let entry = |at: u64| field(&ram.read(at, 8).unwrap(), 0).unwrap();
        assert_eq!((entry(0x1000), entry(0x2000)), (0x2007, 0x3007));
        let small = |at: u64| entry(0x4000 + (at >> 12) * 8);
        assert_eq!(
            small(KERNEL_LOW),
            KERNEL_LOW | 5,
            "code: read-only, executable"
        );
        assert_eq!(
            small(KERNEL_LOW + 0x2000),
            (KERNEL_LOW + 0x2000) | 7 | NX,
            "data: writable, not executable"
        );
        assert_eq!(
            small(GUEST_AT),
            GUEST_AT | 5 | NX,
            "the guest: read-only, not executable"
        );
        for table in [0x1000, 0x2000, 0x3000, 0x4000, 0x5000, 0x6000, 0x7000] {
            assert_eq!(small(table), 0, "page table at {table:#x} is mapped");
        }
        for index in 0..16u64 {
            let large = entry(0x3000 + index * 8);
            match index {
                0..4 => assert_eq!(large, (0x4000 + index * 0x1000) | 7),
                4..12 | 15 => assert_eq!(large, (index << 21) | 0x87 | NX),
                _ => assert_eq!(large, 0),
            }
        }
    }
}
