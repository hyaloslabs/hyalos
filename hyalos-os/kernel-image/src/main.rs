//! hyalOS kernel v0, bare metal: a throwaway spike. It boots straight into 64-bit ring 3 on KVM with
//! no Linux, no drivers, no interrupts, no devices and no ring 0 code beneath it. It holds a WebAssembly interpreter and one
//! way out: a hypercall on port 0x1000 carrying one of five requests (read, infer, effect, return,
//! fault). The monitor outside treats this kernel as untrusted and checks every request.

#![no_std]
#![no_main]
// One of the files allowed `unsafe` (design.md §2 of docs/plans/hyalos-kernel-core).
#![allow(unsafe_code)]

extern crate alloc;

use core::{
    alloc::{GlobalAlloc, Layout},
    arch::asm,
    panic::PanicInfo,
    ptr,
    sync::atomic::{AtomicUsize, Ordering},
};
use hyalos_profile::{PROFILE_1, Profile};
use wasmi::{
    Caller, Config, Engine, Error, Extern, ExternType, Linker, Module, Store, StoreLimits,
    StoreLimitsBuilder, TrapCode,
};

/// The guest interface this kernel implements (`ABI.md`). The monitor holds the same name as
/// `hyalos_audit::ABI`, and a test keeps the two and the document equal.
#[allow(dead_code)]
const ABI: &str = "hyalos.abi/1";

// The one definition both backends build from, so the two produce the same transcript.
const PROFILE: Profile = PROFILE_1;

const HEAP_START: usize = 0x80_0000;
const HEAP_END: usize = 0x180_0000;

const READ: u64 = 1;
const INFER: u64 = 2;
const EFFECT: u64 = 3;
const RETURN: u64 = 4;
const FAULT: u64 = 5;

const INVALID_MODULE: u64 = 1;
const BAD_IMPORT: u64 = 2;
const BAD_EXPORT: u64 = 3;
const OUT_OF_FUEL: u64 = 4;
const GUEST_TRAP: u64 = 5;
const NO_RESULT: u64 = 6;
const KERNEL_PANIC: u64 = 7;

struct Bump(AtomicUsize);

unsafe impl GlobalAlloc for Bump {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let start = (self.0.load(Ordering::Relaxed) + layout.align() - 1) & !(layout.align() - 1);
        match start.checked_add(layout.size()) {
            Some(end) if end <= HEAP_END => {
                self.0.store(end, Ordering::Relaxed);
                start as *mut u8
            }
            _ => ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}

    unsafe fn realloc(&self, old: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // Grow the newest allocation in place; a task is short, so nothing else is reclaimed.
        if old as usize + layout.size() == self.0.load(Ordering::Relaxed) {
            return match (old as usize).checked_add(new_size) {
                Some(end) if end <= HEAP_END => {
                    self.0.store(end, Ordering::Relaxed);
                    old
                }
                _ => ptr::null_mut(),
            };
        }
        let Ok(new_layout) = Layout::from_size_align(new_size, layout.align()) else {
            return ptr::null_mut();
        };
        let new = unsafe { self.alloc(new_layout) };
        if !new.is_null() {
            unsafe { ptr::copy_nonoverlapping(old, new, layout.size().min(new_size)) };
        }
        new
    }
}

#[global_allocator]
static HEAP: Bump = Bump(AtomicUsize::new(HEAP_START));

#[repr(C)]
struct Request {
    op: u64,
    a: u64,
    b: u64,
    c: u64,
    d: u64,
    ret: i64,
}

fn hypercall(op: u64, a: u64, b: u64, c: u64, d: u64) -> i64 {
    let mut request = Request {
        op,
        a,
        b,
        c,
        d,
        ret: 0,
    };
    let address = &raw mut request as u32;
    unsafe {
        asm!("out dx, eax", in("dx") 0x1000u16, in("eax") address, options(nostack, preserves_flags));
        ptr::read_volatile(&raw const request.ret)
    }
}

fn halt(code: u64) -> ! {
    hypercall(FAULT, code, 0, 0, 0);
    // The monitor ends the run at the request above. Should it not, `hlt` faults in ring 3, and a
    // fault ends the machine too.
    loop {
        unsafe { asm!("hlt", options(nomem, nostack)) };
    }
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    halt(KERNEL_PANIC)
}

struct Host {
    limits: StoreLimits,
    result: Option<(u32, u32)>,
}

/// The guest's memory as an address the monitor can read, and how many bytes follow `ptr` in it.
fn span(caller: &mut Caller<'_, Host>, ptr: i32) -> (u64, u64) {
    let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
        return (0, 0);
    };
    let data = memory.data_mut(caller);
    let ptr = ptr as u32 as usize;
    if ptr > data.len() {
        return (0, 0);
    }
    (
        data.as_mut_ptr() as u64 + ptr as u64,
        (data.len() - ptr) as u64,
    )
}

fn fits(available: u64, len: i32) -> u64 {
    u64::from(u64::from(len as u32) <= available)
}

fn linker(engine: &Engine) -> Result<Linker<Host>, u64> {
    let mut linker = Linker::new(engine);
    let bad = |_| INVALID_MODULE;
    linker
        .func_wrap(
            "hyalos",
            "read",
            |mut c: Caller<'_, Host>, index: i32, ptr: i32, cap: i32| -> i32 {
                let (address, available) = span(&mut c, ptr);
                hypercall(
                    READ,
                    u64::from(index as u32),
                    address,
                    u64::from(cap as u32),
                    available,
                ) as i32
            },
        )
        .map_err(bad)?;
    linker
        .func_wrap(
            "hyalos",
            "infer",
            |mut c: Caller<'_, Host>, schema: i32, ptr: i32, len: i32| -> i32 {
                let (address, available) = span(&mut c, ptr);
                hypercall(
                    INFER,
                    u64::from(schema as u32),
                    address,
                    u64::from(len as u32),
                    fits(available, len),
                ) as i32
            },
        )
        .map_err(bad)?;
    linker
        .func_wrap(
            "hyalos",
            "effect",
            |mut c: Caller<'_, Host>, kind: i32, ptr: i32, len: i32| -> i32 {
                let (address, available) = span(&mut c, ptr);
                hypercall(
                    EFFECT,
                    u64::from(kind as u32),
                    address,
                    u64::from(len as u32),
                    fits(available, len),
                ) as i32
            },
        )
        .map_err(bad)?;
    linker
        .func_wrap(
            "hyalos",
            "return",
            |mut c: Caller<'_, Host>, ptr: i32, len: i32| -> Result<(), Error> {
                c.data_mut().result = Some((ptr as u32, len as u32));
                Err(Error::i32_exit(0))
            },
        )
        .map_err(bad)?;
    Ok(linker)
}

/// This backend's one mapping from a profile to its interpreter configuration.
fn config(profile: &Profile) -> Config {
    let mut config = Config::default();
    config
        .consume_fuel(profile.consume_fuel)
        .floats(profile.floats)
        .allow_start_fn(profile.start_function)
        .ignore_custom_sections(profile.ignore_custom_sections)
        .wasm_memory64(profile.memory64)
        .wasm_multi_memory(profile.multi_memory)
        .wasm_tail_call(profile.tail_call)
        .wasm_wide_arithmetic(profile.wide_arithmetic)
        .wasm_custom_page_sizes(profile.custom_page_sizes);
    config
}

fn run(guest: &[u8]) -> u64 {
    let profile = &PROFILE;
    let engine = Engine::new(&config(profile));
    let Ok(module) = Module::new(&engine, guest) else {
        return INVALID_MODULE;
    };
    for import in module.imports() {
        let allowed = import.module() == "hyalos"
            && ["read", "infer", "effect", "return"].contains(&import.name())
            && matches!(import.ty(), ExternType::Func(_));
        if !allowed {
            return BAD_IMPORT;
        }
    }
    let has = |name: &str, func: bool| {
        module.exports().any(|e| {
            e.name() == name
                && if func {
                    matches!(e.ty(), ExternType::Func(_))
                } else {
                    matches!(e.ty(), ExternType::Memory(_))
                }
        })
    };
    if !has("run", true) || !has("memory", false) {
        return BAD_EXPORT;
    }
    let limits = StoreLimitsBuilder::new()
        .memory_size(profile.max_memory)
        .memories(profile.memories)
        .tables(profile.tables)
        .table_elements(profile.table_elements)
        .instances(profile.instances)
        .trap_on_grow_failure(profile.trap_on_grow_failure)
        .build();
    let mut store = Store::new(
        &engine,
        Host {
            limits,
            result: None,
        },
    );
    store.limiter(|h| &mut h.limits);
    if store.set_fuel(profile.fuel).is_err() {
        return INVALID_MODULE;
    }
    let linker = match linker(&engine) {
        Ok(linker) => linker,
        Err(code) => return code,
    };
    let Ok(instance) = linker.instantiate_and_start(&mut store, &module) else {
        return BAD_IMPORT;
    };
    let Ok(run) = instance.get_typed_func::<(), ()>(&store, "run") else {
        return BAD_EXPORT;
    };
    match run.call(&mut store, ()) {
        Ok(()) => return NO_RESULT,
        Err(error) if error.i32_exit_status() == Some(0) => {}
        Err(error) if error.as_trap_code() == Some(TrapCode::OutOfFuel) => return OUT_OF_FUEL,
        Err(_) => return GUEST_TRAP,
    }
    let Ok(left) = store.get_fuel() else {
        return GUEST_TRAP;
    };
    let (Some((ptr, len)), Some(memory)) =
        (store.data().result, instance.get_memory(&store, "memory"))
    else {
        return NO_RESULT;
    };
    let data = memory.data(&store);
    let inside = (ptr as usize)
        .checked_add(len as usize)
        .is_some_and(|end| end <= data.len());
    let address = if inside {
        data.as_ptr() as u64 + u64::from(ptr)
    } else {
        0
    };
    hypercall(
        RETURN,
        address,
        u64::from(len),
        u64::from(inside),
        profile.fuel - left,
    );
    NO_RESULT
}

/// The monitor enters here in 64-bit ring 3 with the guest's bytes in the first two arguments.
///
/// # Safety
///
/// Only the monitor calls this, once, as the image's entry point, with `guest` pointing at `len`
/// bytes it placed in the kernel's memory and maps read-only for the whole run (`kvm.rs`, `map`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start(guest: *const u8, len: usize) -> ! {
    // SAFETY: the monitor placed `len` bytes at `guest`, mapped read-only for the whole run.
    let guest = unsafe { core::slice::from_raw_parts(guest, len) };
    #[cfg(feature = "selftest")]
    probe(guest);
    halt(run(guest))
}

/// Page tables the self-test kernel's `CR3` probe builds for itself: a PML4, a PDPT and a page
/// directory. The kernel is loaded at its physical address, so their addresses are physical too.
#[cfg(feature = "selftest")]
#[repr(C, align(4096))]
struct Tables([[u64; 512]; 3]);

#[cfg(feature = "selftest")]
static mut TABLES: Tables = Tables([[0; 512]; 3]);

/// The byte that asks the self-test kernel to report what CPUID shows it, not to probe.
#[cfg(feature = "selftest")]
const CPUID_REPORT: u8 = 0x80;

/// The CPUID leaves the report reads, each at subleaf 0: the allowed ones, and those that would
/// show the host's features, topology, brand, address size or hypervisor. The monitor's tests hold
/// the same list.
#[cfg(feature = "selftest")]
const REPORTED_LEAVES: [u32; 14] = [
    0,
    1,
    4,
    7,
    0xb,
    0xd,
    0x4000_0000,
    0x4000_0001,
    0x8000_0000,
    0x8000_0001,
    0x8000_0002,
    0x8000_0003,
    0x8000_0004,
    0x8000_0008,
];

/// Returns, as the run's result, `EAX`, `EBX`, `ECX` and `EDX` of each reported leaf, little-endian.
#[cfg(feature = "selftest")]
fn report_cpuid() {
    let out = HEAP_START as *mut u32;
    for (at, &leaf) in REPORTED_LEAVES.iter().enumerate() {
        let r = core::arch::x86_64::__cpuid_count(leaf, 0);
        for (word, value) in [r.eax, r.ebx, r.ecx, r.edx].into_iter().enumerate() {
            // SAFETY: 14 leaves of 16 bytes stay inside the heap (`HEAP_START..HEAP_END`), which is
            // mapped writable for the kernel, and nothing else uses it: the run ends with this report.
            unsafe { ptr::write_volatile(out.add(at * 4 + word), value) };
        }
    }
    hypercall(
        RETURN,
        HEAP_START as u64,
        (REPORTED_LEAVES.len() * 16) as u64,
        1,
        0,
    );
}

/// The bytes that ask the self-test kernel to read `RDSEED` and then ask one question: one whose
/// bytes hold the value read, and one whose bytes do not (S-03.4).
#[cfg(feature = "selftest")]
const SEED_VARIES_QUESTION: u8 = 0x81;
#[cfg(feature = "selftest")]
const SEED_IGNORED: u8 = 0x82;

/// Reads `RDSEED` until it gives a value, as a taken-over kernel could. Where the host blocks it,
/// the first read faults and ends the machine. A host that never gives a value ends the run too.
#[cfg(feature = "selftest")]
fn rdseed() -> u64 {
    for _ in 0..1_000_000 {
        let (value, ready): (u64, u8);
        // SAFETY: reads a register and the carry flag, and touches no memory.
        unsafe {
            asm!("rdseed {v}", "setc {r}", v = out(reg) value, r = out(reg_byte) ready, options(nomem, nostack))
        };
        if ready != 0 {
            return value;
        }
    }
    halt(KERNEL_PANIC)
}

/// Asks one question of schema 0 and ends the run with a fixed result, so the question is the
/// only thing `RDSEED` can change. With `vary`, the question holds the value read in hex.
#[cfg(feature = "selftest")]
fn ask_after_rdseed(vary: bool) {
    let seed = core::hint::black_box(rdseed());
    let mut question = *b"rdseed 0000000000000000";
    if vary {
        for (at, byte) in question[7..].iter_mut().enumerate() {
            *byte = b"0123456789abcdef"[((seed >> (60 - 4 * at)) & 0xf) as usize];
        }
    }
    let at = HEAP_START as *mut u8;
    // SAFETY: the question and the result are a few bytes at the start of the heap, which is mapped
    // writable for the kernel, and nothing else uses it: the run ends with these requests.
    unsafe {
        ptr::copy_nonoverlapping(question.as_ptr(), at, question.len());
        hypercall(INFER, 0, HEAP_START as u64, question.len() as u64, 1);
        ptr::copy_nonoverlapping(b"done".as_ptr(), at, 4);
    }
    hypercall(RETURN, HEAP_START as u64, 4, 1, 0);
}

/// Built only for the monitor's tests: tries one thing the address space or ring 3 must stop,
/// chosen by the first input byte, then sends a request the monitor calls unknown. Dying first is
/// the pass. The monitor's `PROBES` names each one by its byte. `CPUID_REPORT` instead ends the
/// run with what CPUID shows, and `SEED_VARIES_QUESTION` and `SEED_IGNORED` with one question.
#[cfg(feature = "selftest")]
fn probe(guest: &[u8]) {
    let table = [0u64; 2]; // an empty descriptor table to load
    unsafe {
        match guest.first().copied().unwrap_or(0) {
            1 => ptr::write_volatile(
                _start as unsafe extern "C" fn(*const u8, usize) -> ! as usize as *mut u8,
                0x90,
            ), // change its own code
            2 => {
                ptr::write_volatile(HEAP_START as *mut u8, 0xc3); // run bytes it wrote
                core::mem::transmute::<usize, extern "C" fn()>(HEAP_START)();
            }
            3 => ptr::write_volatile(0x1000 as *mut u64, 0), // change its page tables
            4 => ptr::write_volatile(0x1c0_0000 as *mut u8, 1), // run off the stack
            5 => drop(ptr::read_volatile(8 as *const u8)),   // follow a null pointer
            6 => ptr::write_volatile(guest.as_ptr() as *mut u8, 0), // change the signed guest
            7 => drop(ptr::read_volatile(0x200_0000 as *const u8)), // read past its memory
            8 => asm!("rdtsc", out("eax") _, out("edx") _, options(nomem, nostack)), // read the clock
            9 => asm!("rdtscp", out("eax") _, out("edx") _, out("ecx") _, options(nomem, nostack)),
            10 => {
                // Clear `CR4.TSD`, leaving `PAE`. Were the write allowed, the unknown request
                // would end the run before the clock is read, so a fault here is at the write.
                asm!("mov cr4, {}", in(reg) 1u64 << 5, options(nostack));
                hypercall(9, 0, 0, 0, 0);
                asm!("rdtsc", out("eax") _, out("edx") _, options(nomem, nostack));
            }
            11 => {
                // Load page tables of its own that map all 32 MiB writable and executable. They
                // map this code and the stack, so were the load allowed, the unknown request below
                // would end the run. A fault here is at the load.
                let tables = &raw mut TABLES as *mut [u64; 512];
                let base = tables as u64;
                (*tables)[0] = (base + 0x1000) | 3;
                (*tables.add(1))[0] = (base + 0x2000) | 3;
                for (index, entry) in (*tables.add(2)).iter_mut().take(16).enumerate() {
                    *entry = ((index as u64) << 21) | 0x83;
                }
                asm!("mov cr3, {}", in(reg) base, options(nostack));
            }
            12 => asm!("mov cr0, {}", in(reg) 0x8004_0033u64, options(nostack)), // turn write protect off
            13 => asm!("lgdt [{}]", in(reg) table.as_ptr(), options(nostack)),
            14 => asm!("lidt [{}]", in(reg) table.as_ptr(), options(nostack)),
            // Write the time counter's model-specific register.
            15 => {
                asm!("wrmsr", in("ecx") 0x10u32, in("eax") 0u32, in("edx") 0u32, options(nostack))
            }
            16 => asm!("hlt", options(nomem, nostack)),
            17 => asm!("rdrand rax", out("rax") _, options(nomem, nostack)), // hardware randomness
            18 => asm!("rdpid rax", out("rax") _, options(nomem, nostack)), // read the processor ID
            19 => asm!("rdseed rax", out("rax") _, options(nomem, nostack)), // hardware seed
            CPUID_REPORT => report_cpuid(),
            SEED_VARIES_QUESTION => ask_after_rdseed(true),
            SEED_IGNORED => ask_after_rdseed(false),
            _ => {}
        }
    }
    hypercall(9, 0, 0, 0, 0);
}
