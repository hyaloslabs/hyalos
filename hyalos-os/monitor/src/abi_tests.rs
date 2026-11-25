//! The guest interface `hyalos.abi/1`, held on both backends (S-01, `ABI.md`).
//!
//! Every case runs on the process backend and, where KVM is usable, on the kernel image. The two
//! must give byte-identical transcripts, or the same refusal, and that outcome must be the one
//! `ABI.md` states. `HYALOS_REQUIRE_KVM` turns a host without KVM into a failure.

use super::*;
use crate::profile_tests::kvm_backend;
use hyalos_audit::{ABI, MAX_BODY, MAX_EFFECTS, MAX_QUESTION};
use std::path::Path;

/// The digest of each interface section of `ABI.md`. A section is never edited once a
/// backend implements it: a change to a call's name, signature, limit or error value is a new
/// interface name, a new section, a new row here and an ADR.
const PINNED: &[(&str, &str)] = &[(
    "hyalos.abi/1",
    "c252b9e8fcd87805df364b8f0b8e7386e4e70c559aff511b96573648f741fa49",
)];

const GUIDE: &str = "a change to the guest interface needs a new name, a new section in ABI.md, a new pinned digest and an ADR";

fn abi_md() -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../ABI.md")).unwrap()
}

/// The text of `ABI.md`'s section for one interface name, up to the next `##` heading.
fn section(doc: &str, name: &str) -> String {
    let heading = format!("\n## `{name}`\n");
    let start = doc
        .find(&heading)
        .unwrap_or_else(|| panic!("ABI.md has no section for {name}"))
        + heading.len();
    let rest = &doc[start..];
    rest[..rest.find("\n## ").unwrap_or(rest.len())].to_string()
}

/// The rows of every table in a piece of `ABI.md`, without header separators, as trimmed cells.
fn rows(text: &str) -> Vec<Vec<String>> {
    text.lines()
        .filter(|l| l.starts_with('|') && !l.starts_with("|---"))
        .map(|l| {
            l.trim()
                .trim_matches('|')
                .split('|')
                .map(|c| c.trim().trim_matches('`').to_string())
                .collect()
        })
        .collect()
}

/// The digest of a whole section, prose and tables: what a backend is held to.
fn digest(section: &str) -> String {
    sha(section.as_bytes())
}

/// The current interface's call table: name, signature and limit, in the document's order.
fn calls() -> Vec<(String, String, String)> {
    rows(&section(&abi_md(), ABI))
        .into_iter()
        .filter(|r| r.len() == 4 && r[0] != "Call")
        .map(|r| (r[0].clone(), r[1].clone(), r[2].clone()))
        .collect()
}

/// The current interface's refusals: where, and the reason's name.
fn refusals() -> Vec<(String, String)> {
    rows(&section(&abi_md(), ABI))
        .into_iter()
        .filter(|r| r.len() == 3 && r[0] != "Where")
        .map(|r| (r[0].clone(), r[1].clone()))
        .collect()
}

/// The number of bytes a limit cell states, such as `4,096 bytes`.
fn limit_of(cell: &str) -> usize {
    cell.split(' ')
        .next()
        .unwrap()
        .replace(',', "")
        .parse()
        .unwrap_or_else(|_| panic!("not a limit: {cell}"))
}

/// The WebAssembly import of one call with the signature `ABI.md` gives it, such as
/// `(index, ptr, cap) -> len`.
fn import(name: &str, signature: &str, extra_param: bool, flip_result: bool) -> String {
    let (params, result) = signature
        .split_once(" -> ")
        .map_or((signature, false), |(p, _)| (p, true));
    let count = params.trim_matches(['(', ')']).split(',').count() + usize::from(extra_param);
    let result = if result != flip_result {
        " (result i32)"
    } else {
        ""
    };
    format!(
        r#"(import "hyalos" "{name}" (func (param{}){result}))"#,
        " i32".repeat(count)
    )
}

/// Every reason, so a new one fails to compile here until `ABI.md` is checked for it.
fn every_reason() -> Vec<Reason> {
    use Reason::*;
    let all = vec![
        InvalidModule,
        BadImport,
        BadExport,
        OutOfFuel,
        GuestTrap,
        BadPointer,
        TooLarge,
        UnknownSchema,
        UnknownEffect,
        EffectLimit,
        NoResult,
        ReplayDiverged,
        GuestMismatch,
        KernelFault,
        BadHypercall,
        Timeout,
        NoKvm,
        ModelUnavailable,
        ModelRefused,
        NotaryUnreachable,
        NotaryDeadline,
        WrongNotaryKey,
        ReleaseMismatch,
        ProviderRefused,
        NoAuthorisation,
        AttemptsSpent,
        CanaryHeld,
        EffectFailed,
        LedgerFailed,
    ];
    for reason in &all {
        match reason {
            InvalidModule | BadImport | BadExport | OutOfFuel | GuestTrap | BadPointer
            | TooLarge | UnknownSchema | UnknownEffect | EffectLimit | NoResult
            | ReplayDiverged | GuestMismatch | KernelFault | BadHypercall | Timeout | NoKvm
            | ModelUnavailable | ModelRefused | NotaryUnreachable | NotaryDeadline
            | WrongNotaryKey | ReleaseMismatch | ProviderRefused | NoAuthorisation
            | AttemptsSpent | CanaryHeld | EffectFailed | LedgerFailed => {}
            // A spike-only reason, outside `ABI.md` (ADR-0136), so it is not in `all`.
            #[cfg(feature = "spike-contract")]
            BadContract => {}
        }
    }
    all
}

/// The inputs every case runs with. Record 0 is 23 bytes.
fn inputs() -> Vec<Vec<u8>> {
    vec![
        b"457\tfix a crash in read".to_vec(),
        b"458\tadd a feature".to_vec(),
    ]
}

/// A run's transcript as written, or its refusal.
type Outcome = Result<Vec<u8>, Reason>;

/// Runs one guest on both backends, demands the same outcome from each, and returns it.
fn on_both(kvm: &Option<Backend>, wat: &str) -> Outcome {
    let wasm = wat::parse_str(wat).unwrap();
    let written =
        |run: Result<Transcript, Reason>| run.map(|t| serde_json::to_vec_pretty(&t).unwrap());
    let process = written(execute(
        &Backend::Process,
        &wasm,
        inputs(),
        Mode::Record { seed: 0 },
    ));
    if let Some(kvm) = kvm {
        let kernel = written(execute(kvm, &wasm, inputs(), Mode::Record { seed: 0 }));
        assert_eq!(process, kernel, "the backends differ on {wat}");
    }
    process
}

/// The result bytes of a run that ended with `return`.
fn result(outcome: &Outcome) -> Vec<u8> {
    let bytes = outcome
        .as_ref()
        .unwrap_or_else(|r| panic!("refused: {r:?}"));
    unhex(&read_transcript(bytes).unwrap().result_hex).unwrap()
}

/// The result as the guest's stored `i32` values.
fn words(outcome: &Outcome) -> Vec<i32> {
    result(outcome)
        .chunks(4)
        .map(|w| i32::from_le_bytes(w.try_into().unwrap()))
        .collect()
}

/// A guest with the four calls, `pages` of memory and this body as `run`.
fn guest(pages: u32, body: &str) -> String {
    format!(r#"(module {IMPORTS} (memory (export "memory") {pages}) (func (export "run") {body}))"#)
}

const RET: &str = "(call $ret (i32.const 0) (i32.const 4))";

/// One call made with these arguments, then a normal end.
fn calling(call: &str, args: &str) -> String {
    guest(2, &format!("(drop (call ${call} {args})) {RET}"))
}

#[test]
fn core_s_01_1_e1_a_guest_calling_all_four_gives_byte_identical_transcripts() {
    let kvm = kvm_backend("core_s_01_1_e1");
    let body = r#"(local $len i32)
        (local.set $len (call $read (i32.const 0) (i32.const 1024) (i32.const 256)))
        (i32.store (i32.const 0) (local.get $len))
        (i32.store (i32.const 4) (call $read (i32.const 5) (i32.const 1024) (i32.const 256)))
        (i32.store (i32.const 8) (call $read (i32.const -1) (i32.const 1024) (i32.const 256)))
        (i32.store (i32.const 12) (call $infer (i32.const 0) (i32.const 1024) (local.get $len)))
        (i32.store (i32.const 16) (call $effect (i32.const 0) (i32.const 1024) (local.get $len)))
        (call $ret (i32.const 0) (i32.const 20))"#;
    let outcome = on_both(&kvm, &guest(1, body));
    // The record's length, no record twice, the stand-in model's `bug` label, and the effect's 0.
    assert_eq!(words(&outcome), [23, -1, -1, 0, 0]);
    let t = read_transcript(&outcome.unwrap()).unwrap();
    assert_eq!((t.answers.len(), t.effects.len()), (1, 1));
    assert_eq!(unhex(&t.effects[0].body_hex).unwrap(), inputs()[0]);
}

#[test]
fn core_s_01_1_e2_read_into_too_small_a_buffer_returns_minus_2_on_both_backends() {
    let kvm = kvm_backend("core_s_01_1_e2");
    let body = r#"
        (i32.store (i32.const 0) (call $read (i32.const 0) (i32.const 1024) (i32.const 0)))
        (i32.store (i32.const 4) (call $read (i32.const 0) (i32.const 1024) (i32.const 22)))
        (i32.store (i32.const 8) (call $read (i32.const 0) (i32.const 1024) (i32.const 23)))
        (call $ret (i32.const 0) (i32.const 12))"#;
    assert_eq!(words(&on_both(&kvm, &guest(1, body))), [-2, -2, 23]);
}

#[test]
fn core_s_01_1_every_call_refuses_as_abi_md_states_on_both_backends() {
    let kvm = kvm_backend("core_s_01_1");
    let documented = refusals();
    let cases: Vec<(&str, String, Reason)> = vec![
        (
            "read",
            calling("read", "(i32.const 0) (i32.const 131066) (i32.const 256)"),
            Reason::BadPointer,
        ),
        (
            "read",
            calling("read", "(i32.const 0) (i32.const 131066) (i32.const -1)"),
            Reason::BadPointer,
        ),
        (
            "infer",
            calling("infer", "(i32.const 1) (i32.const 0) (i32.const 1)"),
            Reason::UnknownSchema,
        ),
        (
            "infer",
            calling("infer", "(i32.const 9) (i32.const 200000) (i32.const 5000)"),
            Reason::UnknownSchema,
        ),
        (
            "infer",
            calling("infer", "(i32.const 0) (i32.const 0) (i32.const 4097)"),
            Reason::TooLarge,
        ),
        (
            "infer",
            calling("infer", "(i32.const 0) (i32.const 200000) (i32.const 5000)"),
            Reason::TooLarge,
        ),
        (
            "infer",
            calling("infer", "(i32.const 0) (i32.const 130000) (i32.const 4000)"),
            Reason::BadPointer,
        ),
        (
            "infer",
            calling("infer", "(i32.const 0) (i32.const 200000) (i32.const 0)"),
            Reason::BadPointer,
        ),
        (
            "effect",
            calling("effect", "(i32.const 1) (i32.const 0) (i32.const 1)"),
            Reason::UnknownEffect,
        ),
        (
            "effect",
            calling(
                "effect",
                "(i32.const 7) (i32.const 200000) (i32.const 70000)",
            ),
            Reason::UnknownEffect,
        ),
        (
            "effect",
            guest(
                1,
                &format!(
                    "(drop (call $effect (i32.const 0) (i32.const 0) (i32.const 1))) (drop (call $effect (i32.const 0) (i32.const 0) (i32.const 1))) {RET}"
                ),
            ),
            Reason::EffectLimit,
        ),
        (
            "effect",
            calling("effect", "(i32.const 0) (i32.const 0) (i32.const 65537)"),
            Reason::TooLarge,
        ),
        (
            "effect",
            calling(
                "effect",
                "(i32.const 0) (i32.const 130000) (i32.const 4000)",
            ),
            Reason::BadPointer,
        ),
        (
            "return",
            guest(2, "(call $ret (i32.const 0) (i32.const 4097))"),
            Reason::TooLarge,
        ),
        (
            "return",
            guest(2, "(call $ret (i32.const 130000) (i32.const 4000))"),
            Reason::BadPointer,
        ),
        (
            "return",
            guest(2, "(call $ret (i32.const 200000) (i32.const 0))"),
            Reason::BadPointer,
        ),
        ("running", guest(1, "(loop $l (br $l))"), Reason::OutOfFuel),
        (
            "running",
            guest(1, &format!("unreachable {RET}")),
            Reason::GuestTrap,
        ),
        (
            "running",
            guest(1, &format!("(drop (memory.grow (i32.const 1000))) {RET}")),
            Reason::GuestTrap,
        ),
        ("running", guest(1, ""), Reason::NoResult),
    ];
    for (call, wat, want) in cases {
        assert_eq!(on_both(&kvm, &wat), Err(want), "{call}: {wat}");
        assert!(
            documented.contains(&(call.to_string(), format!("{want:?}"))),
            "ABI.md does not list {want:?} for {call}"
        );
    }
    // And each call at its limit is answered, not refused.
    assert_eq!(
        words(&on_both(
            &kvm,
            &guest(
                2,
                "(i32.store (i32.const 0) (call $infer (i32.const 0) (i32.const 0) (i32.const 4096))) (call $ret (i32.const 0) (i32.const 4))"
            )
        )),
        [3]
    );
    assert_eq!(
        words(&on_both(
            &kvm,
            &guest(
                2,
                "(i32.store (i32.const 0) (call $effect (i32.const 0) (i32.const 0) (i32.const 65536))) (call $ret (i32.const 0) (i32.const 4))"
            )
        )),
        [0]
    );
    assert_eq!(
        result(&on_both(
            &kvm,
            &guest(2, "(call $ret (i32.const 0) (i32.const 4096))")
        )),
        vec![0; 4096]
    );
    assert_eq!(
        result(&on_both(
            &kvm,
            &guest(2, "(call $ret (i32.const 131072) (i32.const 0))")
        )),
        Vec::<u8>::new()
    );
}

#[test]
fn core_s_01_2_e1_the_unchanged_interface_passes() {
    let doc = abi_md();
    let pinned = PINNED
        .iter()
        .find(|(name, _)| *name == ABI)
        .map(|(_, pin)| *pin);
    assert_eq!(
        pinned,
        Some(digest(&section(&doc, ABI)).as_str()),
        "ABI.md's {ABI} section changed: {GUIDE}"
    );
    // The kernel image is its own workspace, so its copy of the name is read from its source.
    let kernel = include_str!("../../kernel-image/src/main.rs");
    assert!(
        kernel.contains(&format!("const ABI: &str = \"{ABI}\";")),
        "the kernel image names another interface: {GUIDE}"
    );
    let names: Vec<String> = calls().into_iter().map(|(name, ..)| name).collect();
    assert_eq!(names, ["read", "infer", "effect", "return"], "{GUIDE}");
    // Each call loads with the signature ABI.md gives it, on both backends, and with no other.
    let kvm = kvm_backend("core_s_01_2_e1");
    for (name, signature, _) in calls() {
        let ret = if name == "return" {
            String::new()
        } else {
            import("return", "(ptr, len)", false, false)
        };
        let module = |declared: String| {
            format!(
                r#"(module {declared} {ret} (memory (export "memory") 1) (func (export "run") unreachable))"#
            )
        };
        assert_eq!(
            on_both(&kvm, &module(import(&name, &signature, false, false))),
            Err(Reason::GuestTrap),
            "{name} {signature}: {GUIDE}"
        );
        assert_eq!(
            on_both(&kvm, &module(import(&name, &signature, true, false))),
            Err(Reason::BadImport),
            "{name} with another parameter"
        );
        assert_eq!(
            on_both(&kvm, &module(import(&name, &signature, false, true))),
            Err(Reason::BadImport),
            "{name} with another result"
        );
    }
}

#[test]
fn core_s_01_2_e2_a_limit_raised_without_a_version_change_fails() {
    let kvm = kvm_backend("core_s_01_2_e2");
    let limits: Vec<(String, usize)> = calls()
        .into_iter()
        .filter(|(name, ..)| name != "read")
        .map(|(name, _, limit)| (name, limit_of(&limit)))
        .collect();
    assert_eq!(
        limits,
        [
            ("infer".to_string(), MAX_QUESTION),
            ("effect".to_string(), MAX_BODY),
            ("return".to_string(), MAX_RESULT)
        ],
        "{GUIDE}"
    );
    assert!(
        calls()
            .iter()
            .any(|(name, _, limit)| name == "effect" && limit.ends_with("one per run"))
            && MAX_EFFECTS == 1,
        "{GUIDE}"
    );
    // The documented limit is the one each backend enforces: one byte more is refused.
    for (name, limit) in limits {
        let over = |len: usize| match name.as_str() {
            "return" => guest(2, &format!("(call $ret (i32.const 0) (i32.const {len}))")),
            call => calling(
                call,
                &format!("(i32.const 0) (i32.const 0) (i32.const {len})"),
            ),
        };
        assert!(
            on_both(&kvm, &over(limit)).is_ok(),
            "{name} at {limit} bytes: {GUIDE}"
        );
        assert_eq!(
            on_both(&kvm, &over(limit + 1)),
            Err(Reason::TooLarge),
            "{name} past {limit} bytes: {GUIDE}"
        );
    }
}

#[test]
fn core_s_01_3_e1_a_guest_importing_hyalos_clock_is_refused_by_both_backends() {
    let kvm = kvm_backend("core_s_01_3_e1");
    let wat = r#"(module (import "hyalos" "clock" (func (result i64))) (memory (export "memory") 1) (func (export "run")))"#;
    assert_eq!(on_both(&kvm, wat), Err(Reason::BadImport));
    assert!(refusals().contains(&("loading".into(), "BadImport".into())));
}

#[test]
fn core_s_01_3_e2_a_guest_with_no_run_export_is_refused_by_both_backends() {
    let kvm = kvm_backend("core_s_01_3_e2");
    let wat = format!(
        r#"(module {IMPORTS} (memory (export "memory") 1) (func (export "start") (call $ret (i32.const 0) (i32.const 0))))"#
    );
    assert_eq!(on_both(&kvm, &wat), Err(Reason::BadExport));
    assert!(refusals().contains(&("loading".into(), "BadExport".into())));
}

#[test]
fn core_s_01_3_anything_outside_the_interface_is_refused_alike() {
    let kvm = kvm_backend("core_s_01_3");
    let run = r#"(func (export "run"))"#;
    let memory = r#"(memory (export "memory") 1)"#;
    let cases: Vec<(String, Reason)> = vec![
        (
            format!(
                r#"(module (import "wasi_snapshot_preview1" "clock_time_get" (func (param i32 i64 i32) (result i32))) {memory} {run})"#
            ),
            Reason::BadImport,
        ),
        (
            format!(
                r#"(module (import "env" "read" (func (param i32 i32 i32) (result i32))) {memory} {run})"#
            ),
            Reason::BadImport,
        ),
        (
            format!(
                r#"(module (import "hyalos" "memory" (memory 1)) (export "memory" (memory 0)) {run})"#
            ),
            Reason::BadImport,
        ),
        (
            format!(r#"(module (import "hyalos" "fuel" (global i32)) {memory} {run})"#),
            Reason::BadImport,
        ),
        (
            format!(r#"(module (import "hyalos" "table" (table 1 funcref)) {memory} {run})"#),
            Reason::BadImport,
        ),
        (format!(r#"(module {IMPORTS} {run})"#), Reason::BadExport),
        (
            format!(r#"(module {IMPORTS} {memory} (func (export "run") (param i32)))"#),
            Reason::BadExport,
        ),
        (
            format!(
                r#"(module {IMPORTS} {memory} (func (export "run") (result i32) (i32.const 0)))"#
            ),
            Reason::BadExport,
        ),
        (
            format!(r#"(module {IMPORTS} {memory} (global (export "run") i32 (i32.const 0)))"#),
            Reason::BadExport,
        ),
        (
            format!(r#"(module {IMPORTS} (memory 1) (func (export "memory")) {run})"#),
            Reason::BadExport,
        ),
        (
            with_run("(drop (f64.div (f64.const 0) (f64.const 0)))"),
            Reason::InvalidModule,
        ),
        (
            with_run("(drop (v128.const i32x4 0 0 0 0))"),
            Reason::InvalidModule,
        ),
        (
            with_run("(drop (i32.atomic.load (i32.const 0)))"),
            Reason::InvalidModule,
        ),
        (
            format!(r#"(module {IMPORTS} {memory} (func $s) (start $s) {run})"#),
            Reason::InvalidModule,
        ),
    ];
    let documented = refusals();
    for (wat, want) in cases {
        assert_eq!(on_both(&kvm, &wat), Err(want), "{wat}");
        assert!(
            documented.contains(&("loading".to_string(), format!("{want:?}"))),
            "ABI.md does not list {want:?} when loading"
        );
    }
    // Bytes that are not a module at all.
    let wasm = b"\0asm\x02\0\0\0".to_vec();
    assert_eq!(
        execute(&Backend::Process, &wasm, vec![], Mode::Record { seed: 0 }).err(),
        Some(Reason::InvalidModule)
    );
    if let Some(kvm) = &kvm {
        assert_eq!(
            execute(kvm, &wasm, vec![], Mode::Record { seed: 0 }).err(),
            Some(Reason::InvalidModule)
        );
    }
}

#[test]
fn core_1_1_abi_md_names_the_interface_and_states_every_call_limit_and_reason() {
    let doc = abi_md();
    assert_eq!(ABI, "hyalos.abi/1");
    assert!(doc.contains("## `hyalos.abi/1`"));
    let calls = calls();
    assert_eq!(calls.len(), 4);
    let (read, infer, effect, ret) = (&calls[0], &calls[1], &calls[2], &calls[3]);
    assert_eq!(
        (read.1.as_str(), read.2.as_str()),
        ("(index, ptr, cap) -> len", "Record length")
    );
    assert_eq!(
        (infer.1.as_str(), infer.2.as_str()),
        ("(schema, ptr, len) -> label", "4,096 bytes")
    );
    assert_eq!(
        (effect.1.as_str(), effect.2.as_str()),
        ("(kind, ptr, len) -> 0", "65,536 bytes, one per run")
    );
    assert_eq!(
        (ret.1.as_str(), ret.2.as_str()),
        ("(ptr, len)", "4,096 bytes")
    );
    let section = section(&doc, ABI);
    for value in [
        "`-1` no such record",
        "`-2` `cap` is smaller than the record",
        "`0` bug, `1` feature, `2` security or `3` other",
    ] {
        assert!(section.contains(value), "ABI.md does not state {value}");
    }
    // Every reason the documented tables name exists, and every reason is named somewhere.
    let names: Vec<String> = every_reason().iter().map(|r| format!("{r:?}")).collect();
    let tabled: Vec<String> = rows(&doc)
        .into_iter()
        .filter_map(|r| r.iter().find(|c| names.contains(c)).cloned())
        .collect();
    for (_, reason) in refusals() {
        assert!(
            names.contains(&reason),
            "ABI.md names {reason}, which is not a reason"
        );
    }
    for name in &names {
        assert!(
            tabled.contains(name),
            "ABI.md does not state when a run ends in {name}"
        );
    }
    // `read`'s one refusal is listed, as `design.md` asks.
    assert!(refusals().contains(&("read".into(), "BadPointer".into())));
}
