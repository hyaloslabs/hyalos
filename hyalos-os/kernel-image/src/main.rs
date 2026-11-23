//! hyalOS kernel v0, bare metal: a throwaway spike. It boots straight into 64-bit mode on KVM with
//! no Linux, no drivers, no interrupts and no devices. It holds a WebAssembly interpreter and one
//! way out: a hypercall on port 0x1000 carrying one of five requests (read, infer, effect, return,
//! fault). The monitor outside treats this kernel as untrusted and checks every request.

#![no_std]
#![no_main]

extern crate alloc;

use core::{
    alloc::{GlobalAlloc, Layout},
    arch::asm,
    panic::PanicInfo,
    ptr,
    sync::atomic::{AtomicUsize, Ordering},
};
use wasmi::{
    Caller, Config, Engine, Error, Extern, ExternType, Linker, Module, Store, StoreLimits,
    StoreLimitsBuilder, TrapCode,
};

// These must match the process kernel, or the two would not produce the same transcript.
const FUEL: u64 = 200_000_000;
const MAX_MEMORY: usize = 4 * 1024 * 1024;

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
    let mut request = Request { op, a, b, c, d, ret: 0 };
    let address = &raw mut request as u32;
    unsafe {
        asm!("out dx, eax", in("dx") 0x1000u16, in("eax") address, options(nostack, preserves_flags));
        ptr::read_volatile(&raw const request.ret)
    }
}

fn halt(code: u64) -> ! {
    hypercall(FAULT, code, 0, 0, 0);
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
    (data.as_mut_ptr() as u64 + ptr as u64, (data.len() - ptr) as u64)
}

fn fits(available: u64, len: i32) -> u64 {
    u64::from(u64::from(len as u32) <= available)
}

fn linker(engine: &Engine) -> Result<Linker<Host>, u64> {
    let mut linker = Linker::new(engine);
    let bad = |_| INVALID_MODULE;
    linker
        .func_wrap("hyalos", "read", |mut c: Caller<'_, Host>, index: i32, ptr: i32, cap: i32| -> i32 {
            let (address, available) = span(&mut c, ptr);
            hypercall(READ, u64::from(index as u32), address, u64::from(cap as u32), available) as i32
        })
        .map_err(bad)?;
    linker
        .func_wrap("hyalos", "infer", |mut c: Caller<'_, Host>, schema: i32, ptr: i32, len: i32| -> i32 {
            let (address, available) = span(&mut c, ptr);
            hypercall(INFER, u64::from(schema as u32), address, u64::from(len as u32), fits(available, len)) as i32
        })
        .map_err(bad)?;
    linker
        .func_wrap("hyalos", "effect", |mut c: Caller<'_, Host>, kind: i32, ptr: i32, len: i32| -> i32 {
            let (address, available) = span(&mut c, ptr);
            hypercall(EFFECT, u64::from(kind as u32), address, u64::from(len as u32), fits(available, len)) as i32
        })
        .map_err(bad)?;
    linker
        .func_wrap("hyalos", "return", |mut c: Caller<'_, Host>, ptr: i32, len: i32| -> Result<(), Error> {
            c.data_mut().result = Some((ptr as u32, len as u32));
            Err(Error::i32_exit(0))
        })
        .map_err(bad)?;
    Ok(linker)
}

fn run(guest: &[u8]) -> u64 {
    let mut config = Config::default();
    config
        .consume_fuel(true)
        .floats(false)
        .allow_start_fn(false)
        .ignore_custom_sections(true)
        .wasm_memory64(false)
        .wasm_multi_memory(false)
        .wasm_tail_call(false)
        .wasm_wide_arithmetic(false)
        .wasm_custom_page_sizes(false);
    let engine = Engine::new(&config);
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
                && if func { matches!(e.ty(), ExternType::Func(_)) } else { matches!(e.ty(), ExternType::Memory(_)) }
        })
    };
    if !has("run", true) || !has("memory", false) {
        return BAD_EXPORT;
    }
    let limits = StoreLimitsBuilder::new()
        .memory_size(MAX_MEMORY)
        .memories(1)
        .tables(1)
        .table_elements(4096)
        .instances(1)
        .trap_on_grow_failure(true)
        .build();
    let mut store = Store::new(&engine, Host { limits, result: None });
    store.limiter(|h| &mut h.limits);
    if store.set_fuel(FUEL).is_err() {
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
    let (Some((ptr, len)), Some(memory)) = (store.data().result, instance.get_memory(&store, "memory")) else {
        return NO_RESULT;
    };
    let data = memory.data(&store);
    let inside = (ptr as usize).checked_add(len as usize).is_some_and(|end| end <= data.len());
    let address = if inside { data.as_ptr() as u64 + u64::from(ptr) } else { 0 };
    hypercall(RETURN, address, u64::from(len), u64::from(inside), FUEL - left);
    NO_RESULT
}

/// The monitor enters here in 64-bit mode with the guest's bytes in the first two arguments.
#[unsafe(no_mangle)]
pub extern "C" fn _start(guest: *const u8, len: usize) -> ! {
    let guest = unsafe { core::slice::from_raw_parts(guest, len) };
    #[cfg(feature = "selftest")]
    probe(guest);
    halt(run(guest))
}

/// Built only for the monitor's tests: tries one thing the address space must stop, chosen by the
/// first input byte, then sends a request the monitor calls unknown. Dying first is the pass.
#[cfg(feature = "selftest")]
fn probe(guest: &[u8]) {
    unsafe {
        match guest.first().copied().unwrap_or(0) {
            1 => ptr::write_volatile(_start as usize as *mut u8, 0x90), // change its own code
            2 => {
                ptr::write_volatile(HEAP_START as *mut u8, 0xc3); // run bytes it wrote
                core::mem::transmute::<usize, extern "C" fn()>(HEAP_START)();
            }
            3 => ptr::write_volatile(0x1000 as *mut u64, 0), // change its page tables
            4 => ptr::write_volatile(0x1c0_0000 as *mut u8, 1), // run off the stack
            5 => drop(ptr::read_volatile(8 as *const u8)), // follow a null pointer
            6 => ptr::write_volatile(guest.as_ptr() as *mut u8, 0), // change the signed guest
            7 => drop(ptr::read_volatile(0x200_0000 as *const u8)), // read past its memory
            _ => {}
        }
    }
    hypercall(9, 0, 0, 0, 0);
}
