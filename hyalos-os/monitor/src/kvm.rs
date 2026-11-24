//! The monitor: it boots the bare-metal kernel on KVM and answers its hypercalls. It gives the
//! machine one virtual CPU, 32 MiB of memory and nothing else: no devices, no interrupts, no disk,
//! no network. It treats the kernel inside as untrusted, so a kernel taken over by a guest still
//! has only the five requests below, each checked here. The kernel runs in ring 3 with no ring 0
//! code beneath it (ADR-001 of `docs/plans/hyalos-kernel-core`), so it has no clock and no
//! privileged instruction, and any fault ends the machine.

use crate::{Host, MAX_RESULT, Reason};
use kvm_bindings::{KVM_MAX_CPUID_ENTRIES, kvm_dtable, kvm_regs, kvm_segment, kvm_sregs, kvm_userspace_memory_region};
use kvm_ioctls::{Kvm, VcpuExit};
use std::{
    ptr,
    time::{Duration, Instant},
};

const RAM: u64 = 32 << 20;
const KERNEL_LOW: u64 = 0x10_0000;
const KERNEL_HIGH: u64 = 0x70_0000;
const GUEST_AT: u64 = 0x70_0000;
const GUEST_MAX: u64 = 0x10_0000;
const PORT: u16 = 0x1000;
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
        let at = unsafe { libc::mmap(ptr::null_mut(), RAM as usize, libc::PROT_READ | libc::PROT_WRITE, flags, -1, 0) };
        if at == libc::MAP_FAILED {
            return None;
        }
        if std::env::var_os("HYALOS_HUGE").is_some() {
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
        let every = libc::timeval { tv_sec: 0, tv_usec: micros };
        let timer = libc::itimerval { it_interval: every, it_value: every };
        // SAFETY: plain libc calls with valid arguments; the handler does nothing.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = tick as usize;
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

/// Copies the kernel image into memory and returns its entry point. An image is an ELF file, or
/// raw machine code placed at the kernel's base (the tests use that for hostile kernels).
fn load(kernel: &[u8], ram: &Ram) -> Option<(u64, Vec<(u64, u64, u32)>)> {
    let mut segments = Vec::new();
    if !kernel.starts_with(b"\x7fELF") {
        (kernel.len() as u64 <= KERNEL_HIGH - KERNEL_LOW).then_some(())?;
        ram.write(KERNEL_LOW, kernel)?;
        // A raw image stands for an attacker who already runs code in the kernel: give it everything.
        return Some((KERNEL_LOW, vec![(KERNEL_LOW, KERNEL_HIGH, 7)]));
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
    (KERNEL_LOW..KERNEL_HIGH).contains(&entry).then_some((entry, segments))
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
fn map(ram: &Ram, segments: &[(u64, u64, u32)], guest_len: u64) -> Option<()> {
    let mut small = vec![0u64; 4 * 512]; // 4 KiB pages for the first 8 MiB: kernel image and guest
    let mut page = |at: u64, entry: u64| {
        let slot = small.get_mut((at >> 12) as usize)?;
        // Two segments wanting one page with different rights is refused, not resolved.
        (*slot == 0 || *slot == at | entry).then(|| *slot = at | entry)
    };
    for &(start, end, flags) in segments {
        let entry = PRESENT | USER | if flags & 2 != 0 { WRITABLE } else { 0 } | if flags & 1 != 0 { 0 } else { NX };
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
            _ => 0,                                                       // the guard below the stack
        };
    }
    let bytes = |values: &[u64]| values.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>();
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
    let code = kvm_segment { limit: 0xffff_ffff, selector: 8 | 3, type_: 11, present: 1, dpl: 3, s: 1, l: 1, g: 1, ..Default::default() };
    let data = kvm_segment { selector: 16 | 3, type_: 3, db: 1, l: 0, ..code };
    sregs.cs = code;
    (sregs.ds, sregs.es, sregs.fs, sregs.gs, sregs.ss) = (data, data, data, data, data);
    (sregs.cr0, sregs.cr3, sregs.cr4, sregs.efer) = (CR0, 0x1000, CR4_PAE | CR4_TSD, EFER);
    let none = kvm_dtable { base: 0, limit: 0, ..Default::default() };
    (sregs.gdt, sregs.idt) = (none, none);
    // Bit 1 of `RFLAGS` is always set. Interrupts stay off.
    kvm_regs { rip: entry, rsp: RAM - 8, rflags: IOPL_3 | 2, rdi: GUEST_AT, rsi: guest_len, ..Default::default() }
}

enum Step {
    Reply(i64),
    Done(Vec<u8>, u64),
}

fn answer(ram: &Ram, host: &mut Host, at: u64) -> Result<Step, Reason> {
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
            let Some(record) = host.input(a as u32 as i32) else {
                return Ok(Step::Reply(-1));
            };
            if c < record.len() as u64 {
                return Ok(Step::Reply(-2));
            }
            if d < record.len() as u64 || !within(&HEAP, b, record.len() as u64) {
                return Err(Reason::BadPointer);
            }
            ram.write(b, record).ok_or(Reason::BadPointer)?;
            Ok(Step::Reply(record.len() as i64))
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

pub(crate) fn run(kernel: &[u8], guest: &[u8], mut host: Host, limit: Duration) -> Result<(Host, Vec<u8>, u64), Reason> {
    let no = |_| Reason::NoKvm;
    let trace = std::env::var_os("HYALOS_TRACE").is_some();
    let t0 = Instant::now();
    if guest.len() as u64 > GUEST_MAX {
        return Err(Reason::TooLarge);
    }
    let ram = Ram::new().ok_or(Reason::NoKvm)?;
    let (entry, segments) = load(kernel, &ram).ok_or(Reason::KernelFault)?;
    ram.write(GUEST_AT, guest).ok_or(Reason::TooLarge)?;
    map(&ram, &segments, guest.len() as u64).ok_or(Reason::KernelFault)?;

    let t_mem = t0.elapsed();
    let kvm = Kvm::new().map_err(no)?;
    let vm = kvm.create_vm().map_err(no)?;
    let region = kvm_userspace_memory_region {
        slot: 0,
        flags: 0,
        guest_phys_addr: 0,
        memory_size: RAM,
        userspace_addr: ram.0 as u64,
    };
    // SAFETY: the mapping outlives the machine: `ram` is dropped after `vm` and `vcpu`.
    unsafe { vm.set_user_memory_region(region) }.map_err(no)?;
    let mut vcpu = vm.create_vcpu(0).map_err(no)?;
    let cpuid = kvm.get_supported_cpuid(KVM_MAX_CPUID_ENTRIES).map_err(no)?;
    vcpu.set_cpuid2(&cpuid).map_err(no)?;

    // Start in 64-bit ring 3: no firmware, no boot loader, no 16-bit code, no ring 0 code.
    let mut sregs = vcpu.get_sregs().map_err(no)?;
    let regs = start_state(&mut sregs, entry, guest.len() as u64);
    vcpu.set_sregs(&sregs).map_err(no)?;
    vcpu.set_regs(&regs).map_err(no)?;

    let t_vm = t0.elapsed();
    let (start, _ticker) = (Instant::now(), Ticker::start());
    let (mut first, mut exits, mut in_host) = (None, 0u32, Duration::ZERO);
    loop {
        if start.elapsed() > limit {
            return Err(Reason::Timeout);
        }
        let at = match vcpu.run() {
            Ok(VcpuExit::IoOut(PORT, data)) => match <[u8; 4]>::try_from(&*data) {
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
        first.get_or_insert_with(|| t0.elapsed());
        exits += 1;
        let before = Instant::now();
        let step = answer(&ram, &mut host, at)?;
        in_host += before.elapsed();
        match step {
            Step::Reply(value) => ram.write(at + 40, &value.to_le_bytes()).ok_or(Reason::BadPointer)?,
            Step::Done(result, fuel) => {
                if trace {
                    let ms = |d: Duration| d.as_secs_f64() * 1e3;
                    eprintln!(
                        "trace ms: memory+load {:.2}, vm setup {:.2}, boot to first call {:.2}, calls {:.2} ({exits} exits, {:.2} in host), total {:.2}",
                        ms(t_mem), ms(t_vm - t_mem), ms(first.unwrap_or_default() - t_vm), ms(t0.elapsed() - first.unwrap_or_default()), ms(in_host), ms(t0.elapsed())
                    );
                }
                // Tearing a machine down takes the host tens of milliseconds, so it is not waited for.
                std::thread::spawn(move || {
                    // The deadline's signal must reach the thread running a virtual CPU, not this one.
                    // SAFETY: plain libc calls on a local, zeroed signal set.
                    unsafe {
                        let mut set: libc::sigset_t = std::mem::zeroed();
                        libc::sigemptyset(&mut set);
                        libc::sigaddset(&mut set, libc::SIGALRM);
                        libc::pthread_sigmask(libc::SIG_BLOCK, &set, ptr::null_mut());
                    }
                    drop((vcpu, vm, kvm, ram));
                });
                return Ok((host, result, fuel));
            }
        }
    }
}

/// Throws random requests at the monitor's request handler, as a kernel under an attacker's
/// control would, and counts what came back. A panic or a crash here is a bug in the wall itself.
pub(crate) fn fuzz(mut host: impl FnMut() -> Host, rounds: u64) -> Option<(u64, u64)> {
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
        let at = if wide % 4 == 0 { at } else { HEAP.start + at % span };
        let buffer = edge(RAM);
        let buffer = if wide % 2 == 0 { buffer } else { HEAP.start + edge(span) % (span + 64) };
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
        assert_eq!((sregs.cs.selector, sregs.cs.dpl, sregs.cs.l, sregs.cs.type_), (11, 3, 1, 11));
        for data in [sregs.ds, sregs.es, sregs.fs, sregs.gs, sregs.ss] {
            assert_eq!((data.selector, data.dpl, data.type_), (19, 3, 3));
        }
        assert_eq!(sregs.cr4, (1 << 5) | (1 << 2));
        assert_eq!((sregs.gdt.limit, sregs.idt.limit), (0, 0));
        assert_eq!(regs.rflags, 0x3002, "IOPL 3, interrupts off");
        assert_eq!((regs.rip, regs.rdi, regs.rsi), (0x10_1000, GUEST_AT, 5));
    }

    /// Every entry on the way to a reachable page carries the user bit with its old rights, and
    /// the page tables themselves stay unmapped.
    #[test]
    fn core_s_02_every_reachable_page_is_a_user_page_and_the_tables_are_unmapped() {
        let ram = Ram::new().unwrap();
        let segments = [(KERNEL_LOW, KERNEL_LOW + 0x2000, 5), (KERNEL_LOW + 0x2000, KERNEL_LOW + 0x3000, 6)];
        map(&ram, &segments, 10).unwrap();
        let entry = |at: u64| field(&ram.read(at, 8).unwrap(), 0).unwrap();
        assert_eq!((entry(0x1000), entry(0x2000)), (0x2007, 0x3007));
        let small = |at: u64| entry(0x4000 + (at >> 12) * 8);
        assert_eq!(small(KERNEL_LOW), KERNEL_LOW | 5, "code: read-only, executable");
        assert_eq!(small(KERNEL_LOW + 0x2000), (KERNEL_LOW + 0x2000) | 7 | NX, "data: writable, not executable");
        assert_eq!(small(GUEST_AT), GUEST_AT | 5 | NX, "the guest: read-only, not executable");
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
