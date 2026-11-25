//! The hyalOS spike's Monty guest (Slice 4 of `docs/plans/hyalos-kernel-python`): its `run`, the host
//! module `_hyalos`, and input record 1 as module `hyalos`.
//!
//! Under a spike profile input record 0 is the contract, 1 the generated SDK and 2 the script
//! (`design.md` §2). `run` compiles record 2 as `__main__` and drives Monty one suspension at a time:
//!
//! - `import hyalos` is answered with a module file whose source is [`PRELUDE`], which defines
//!   `bytearray` (Monty has none) and then executes record 1's bytes, unchanged, as the module's body;
//! - `import _hyalos` is answered with a host module of four functions, `read`, `infer`, `effect`
//!   and `finish`, each a call of the `hyalos` import of the same meaning;
//! - every other import, name or OS call is refused as CPython would refuse it in a sandbox with no
//!   files: `ModuleNotFoundError`, `NameError`, or the run ending `error OSError`.
//!
//! An uncaught exception ends the run with `finish` of `error ` and the exception's type name. Time
//! is fixed, randomness is a constant (`__getrandom_v03_custom`), and print goes nowhere.

use monty::{MontyRun, RunProgress};
use monty_types::{
    CallArgs, CompileOptions, DateTimeSource, ExcType, ExtFunctionResult, ModuleFile,
    MontyException, MontyObject, MontyUuid, NameLookupResult, OsFunctionCall, OsPolicy,
    PrintWriter, RandomSeed, RandomStart, ResourceTracker,
};

#[link(wasm_import_module = "hyalos")]
unsafe extern "C" {
    #[link_name = "read"]
    fn host_read(index: i32, ptr: i32, cap: i32) -> i32;
    #[link_name = "infer"]
    fn host_infer(schema: i32, ptr: i32, len: i32) -> i32;
    #[link_name = "effect"]
    fn host_effect(kind: i32, ptr: i32, len: i32) -> i32;
    #[link_name = "return"]
    fn host_return(ptr: i32, len: i32);
}

/// A record's buffer: the digest guest's (`hyalos-os/guest/src/lib.rs:28`) and the MicroPython
/// guest's. Records 1 and 2 are read through it too.
const RECORD_CAP: usize = 16384;

/// The source of module `hyalos`. Monty has no `bytearray`, which the generated SDK names in
/// `isinstance(value, (bytes, bytearray))`, and a file module's unknown name is a `NameError` the
/// host is never asked about. So the module first defines a `bytearray` class that no value is an
/// instance of and that cannot be built, which leaves `isinstance` giving what it gives in CPython
/// for every value Monty can hold, and then runs record 1's bytes, unchanged, in its own namespace.
const PRELUDE: &str = "class bytearray:\n    def __init__(self, *args):\n        raise TypeError('bytearray is not supported')\nimport _hyalos\nexec(_hyalos.read(1).decode())\n";

fn finish(bytes: &[u8]) -> ! {
    // SAFETY: the pointer and length name `bytes`, which lives across the call; the host does not
    // return from `return`.
    unsafe { host_return(bytes.as_ptr() as i32, bytes.len() as i32) };
    unreachable!("the host does not return from `return`")
}

fn finish_error(name: &str) -> ! {
    let mut out = b"error ".to_vec();
    out.extend_from_slice(name.as_bytes());
    finish(&out)
}

/// Record `index`: its bytes, `None` past the last record, or empty bytes when it is too large for
/// the buffer (`read`'s `-2`), which no real record is.
fn read(index: i64) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; RECORD_CAP];
    let Ok(index) = i32::try_from(index) else {
        return None;
    };
    // SAFETY: the pointer and capacity name `buf`, which the host writes at most `cap` bytes into.
    let n = unsafe { host_read(index, buf.as_mut_ptr() as i32, RECORD_CAP as i32) };
    match n {
        -1 => None,
        n if n < 0 => Some(Vec::new()),
        n => {
            buf.truncate(n as usize);
            Some(buf)
        }
    }
}

/// The bytes of a `bytes` value, from its repr, which Monty writes losslessly
/// (`monty_types::format::bytes_repr_fmt`): Monty has no other public way to read them.
fn bytes_of(repr: &str) -> Option<Vec<u8>> {
    let body = repr.strip_prefix('b')?;
    let quote = body.chars().next()?;
    let body = body.strip_prefix(quote)?.strip_suffix(quote)?.as_bytes();
    let mut out = Vec::with_capacity(body.len());
    let mut i = 0;
    while i < body.len() {
        let b = body[i];
        i += 1;
        if b != b'\\' {
            out.push(b);
            continue;
        }
        let e = *body.get(i)?;
        i += 1;
        out.push(match e {
            b'\\' => b'\\',
            b't' => b'\t',
            b'n' => b'\n',
            b'r' => b'\r',
            b'\'' => b'\'',
            b'"' => b'"',
            b'x' => {
                let hex = std::str::from_utf8(body.get(i..i + 2)?).ok()?;
                i += 2;
                u8::from_str_radix(hex, 16).ok()?
            }
            _ => return None,
        });
    }
    Some(out)
}

fn type_error() -> ExtFunctionResult {
    ExtFunctionResult::Error(MontyException::new(ExcType::TypeError, None))
}

/// One call of a `_hyalos` function: positional arguments only, an `int` and `bytes` as the native
/// module of the MicroPython guest takes them. Anything else is a `TypeError` in the guest.
fn call(name: &str, args: &CallArgs) -> ExtFunctionResult {
    if args.kwargs().len() != 0 {
        return type_error();
    }
    let args: Vec<_> = args.args().collect();
    let int = |i: usize| args.get(i).filter(|a| a.type_name() == "int").and_then(|a| a.as_int());
    let bytes = |i: usize| {
        args.get(i)
            .filter(|a| a.type_name() == "bytes")
            .and_then(|a| bytes_of(&a.py_repr()))
    };
    let small = |v: i64| i32::try_from(v).ok();
    match (name, args.len()) {
        ("read", 1) => match int(0) {
            Some(index) => ExtFunctionResult::Return(read(index).map_or_else(MontyObject::none, MontyObject::bytes)),
            None => type_error(),
        },
        ("infer", 2) => match (int(0).and_then(small), bytes(1)) {
            (Some(schema), Some(q)) => {
                // SAFETY: the pointer and length name `q`, which lives across the call.
                let label = unsafe { host_infer(schema, q.as_ptr() as i32, q.len() as i32) };
                ExtFunctionResult::Return(MontyObject::int(i64::from(label)))
            }
            _ => type_error(),
        },
        ("effect", 2) => match (int(0).and_then(small), bytes(1)) {
            (Some(kind), Some(body)) => {
                // SAFETY: as for `infer`. A refusal ends the run in the monitor: this call then
                // never returns.
                unsafe { host_effect(kind, body.as_ptr() as i32, body.len() as i32) };
                ExtFunctionResult::Return(MontyObject::none())
            }
            _ => type_error(),
        },
        ("finish", 1) => match bytes(0) {
            Some(result) => finish(&result),
            None => type_error(),
        },
        _ => type_error(),
    }
}

/// The host module `_hyalos`: its four functions, each called back by name.
fn hyalos_module() -> MontyObject {
    let attrs = ["read", "infer", "effect", "finish"]
        .map(|name| (MontyObject::string(name), MontyObject::function(name, None)));
    MontyObject::module("_hyalos", MontyUuid::from_u128(1), attrs)
}

/// Record 1 or 2 as source text. A missing record ends the run `error NoSource`, one too large
/// for the buffer `error BufferTooSmall` (`design.md` §2, Runtime guest `run`).
fn source(index: i64) -> String {
    match read(index) {
        None => finish_error("NoSource"),
        Some(r) if r.is_empty() => finish_error("BufferTooSmall"),
        Some(r) => String::from_utf8(r).unwrap_or_else(|_| finish_error("UnicodeDecodeError")),
    }
}

/// The guest's one export.
#[unsafe(no_mangle)]
pub extern "C" fn run() {
    let policy = OsPolicy {
        datetime: DateTimeSource::Fixed {
            unix_seconds: 0,
            microsecond: 0,
        },
        random_start: RandomStart::Seed(RandomSeed::Int(0.into())),
        ..OsPolicy::default()
    };
    // Record 1 is checked here, before anything compiles; `PRELUDE` then reads a record known to
    // fit its buffer.
    source(1);
    let runner = match MontyRun::new(source(2), "main.py", vec![], CompileOptions::default()) {
        Ok(runner) => runner.with_os_policy(policy),
        Err(e) => finish_error(&e.exc_type().to_string()),
    };
    let mut progress = runner.start(vec![], ResourceTracker::default(), PrintWriter::Disabled);
    loop {
        progress = match progress {
            Err(e) => finish_error(&e.exc_type().to_string()),
            // The script ended without `finish`: the run ends with no result.
            Ok(RunProgress::Complete(_)) => return,
            Ok(RunProgress::FunctionCall(fc)) => {
                // A method call on a host object is no `_hyalos` call.
                let result = if fc.object_id.is_some() {
                    type_error()
                } else {
                    call(&fc.function_name, &fc.args)
                };
                fc.resume(result, PrintWriter::Disabled)
            }
            Ok(RunProgress::NameLookup(lookup)) => {
                lookup.resume(NameLookupResult::Undefined, PrintWriter::Disabled)
            }
            Ok(RunProgress::OsCall(oc)) => {
                let result = match &oc.function_call {
                    OsFunctionCall::Import(args) if args.module == "hyalos" => {
                        ExtFunctionResult::ModuleFile(ModuleFile {
                            path: "/hyalos.py".into(),
                            source: PRELUDE.into(),
                        })
                    }
                    OsFunctionCall::Import(args) if args.module == "_hyalos" => {
                        ExtFunctionResult::Return(hyalos_module())
                    }
                    OsFunctionCall::Import(args) => ExtFunctionResult::NotFound(args.module.clone()),
                    _ => finish_error("OSError"),
                };
                oc.resume(result, PrintWriter::Disabled)
            }
            Ok(RunProgress::ResolveFutures(_)) => finish_error("ResolveFutures"),
        };
    }
}

/// `getrandom`'s backend on this target (`getrandom_backend="custom"`, set by `build.sh`): a
/// constant, so Monty's hashing and identities are the same on every run.
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __getrandom_v03_custom(
    dest: *mut u8,
    len: usize,
) -> Result<(), getrandom::Error> {
    // SAFETY: `getrandom` passes a buffer of `len` writable bytes.
    unsafe { core::ptr::write_bytes(dest, 0x5a, len) };
    Ok(())
}
