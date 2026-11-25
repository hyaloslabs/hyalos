//! The Python guest spike's committed evidence (Slices 3 to 5 of `docs/plans/hyalos-kernel-python`,
//! ADR-0136): every case in `spikes/438/evidence/expected.tsv` is run again from its inputs on each
//! backend its line lists, and every committed record replays. The MicroPython guest's own checks
//! (3.3, S-04.4) are here too. Each test names the criterion it proves. `HYALOS_REQUIRE_KVM` turns a
//! host without KVM into a failure.

use super::*;
use crate::profile_tests::kvm_backend;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const SPIKE: &str = "hyalos.spike/1";
const MICROPYTHON: &str = "spikes/438/guests/micropython.wasm";
const FIXTURE: &str = "hyalos-os/evidence/fixtures/issues.tsv";

/// The repository's root, which `expected.tsv`'s paths are relative to.
fn repo() -> PathBuf {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).to_path_buf()
}

fn path(relative: &str) -> String {
    repo().join(relative).to_str().unwrap().to_string()
}

fn written(t: &Transcript) -> Vec<u8> {
    serde_json::to_vec_pretty(t).unwrap()
}

/// One line of `expected.tsv`.
struct Case {
    id: String,
    guest: String,
    contract: String,
    sdk: String,
    script: String,
    data: String,
    seed: u64,
    both: bool,
    outcome: Outcome,
    record: Option<String>,
}

#[derive(Debug, PartialEq)]
enum Outcome {
    Result { bytes: Vec<u8>, effects: usize },
    Refused(String),
}

fn outcome(text: &str, id: &str) -> Outcome {
    if let Some(reason) = text.strip_prefix("refused ") {
        return Outcome::Refused(reason.to_string());
    }
    let (bytes, effects) = text
        .strip_prefix("result ")
        .and_then(|rest| rest.rsplit_once(" effects "))
        .unwrap_or_else(|| panic!("{id}: outcome {text:?}"));
    Outcome::Result {
        bytes: bytes.as_bytes().to_vec(),
        effects: effects.parse().unwrap_or_else(|_| panic!("{id}: effects")),
    }
}

fn cases() -> Vec<Case> {
    let text = fs::read_to_string(repo().join("spikes/438/evidence/expected.tsv")).unwrap();
    let cases: Vec<Case> = text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            let [
                id,
                guest,
                contract,
                sdk,
                script,
                data,
                seed,
                backends,
                out,
                record,
            ] = f[..]
            else {
                panic!("expected.tsv: a line without ten fields: {line:?}");
            };
            Case {
                id: id.into(),
                guest: guest.into(),
                contract: contract.into(),
                sdk: sdk.into(),
                script: script.into(),
                data: data.into(),
                seed: seed.parse().unwrap_or_else(|_| panic!("{id}: seed")),
                both: match backends {
                    "both" => true,
                    "process" => false,
                    _ => panic!("{id}: backends {backends:?}"),
                },
                outcome: outcome(out, id),
                record: (record != "-").then(|| record.into()),
            }
        })
        .collect();
    assert!(!cases.is_empty(), "expected.tsv lists no case");
    cases
}

/// The runtimes and case ids that `spikes/438/README.md` names on a `process-only:` line.
fn process_only() -> BTreeSet<String> {
    let text = fs::read_to_string(repo().join("spikes/438/README.md")).unwrap();
    text.lines()
        .filter_map(|l| l.strip_prefix("process-only: "))
        .filter_map(|l| l.split_once(" — "))
        .map(|(name, _)| name.to_string())
        .collect()
}

/// The profile `spikes/438/README.md` names for a runtime on its `<runtime> profile: <name>` line,
/// the guest's file stem, as `run.sh` runs it (`HYALOS_SPIKE_PROFILE`).
fn runtime_profile(guest: &str) -> String {
    let runtime = Path::new(guest).file_stem().unwrap().to_str().unwrap();
    let text = fs::read_to_string(repo().join("spikes/438/README.md")).unwrap();
    let prefix = format!("{runtime} profile: ");
    text.lines()
        .find_map(|l| l.strip_prefix(prefix.as_str()))
        .and_then(|l| l.split_whitespace().next())
        .unwrap_or_else(|| panic!("{runtime}: no `{prefix}` line in spikes/438/README.md"))
        .to_string()
}

/// A case's run from its inputs, under its runtime's profile (`runtime_profile`), as `spike-run`
/// makes it. A case's record must name that profile.
fn run_case(case: &Case, backend: &Backend) -> Result<Transcript, Reason> {
    let guest = fs::read(path(&case.guest)).unwrap_or_else(|_| panic!("{}: no guest", case.id));
    for input in [&case.contract, &case.sdk, &case.script, &case.data] {
        assert!(repo().join(input).is_file(), "{}: no {input}", case.id);
    }
    let inputs = spike::inputs(
        &path(&case.contract),
        &path(&case.sdk),
        &path(&case.script),
        &path(&case.data),
    )
    .unwrap();
    let profile = runtime_profile(&case.guest);
    if let Some(record) = &case.record {
        let named = read_transcript(&fs::read(path(record)).unwrap())
            .ok()
            .and_then(|t| t.profile);
        assert_eq!(
            named.as_ref(),
            Some(&profile),
            "{}: the record's profile",
            case.id
        );
    }
    hyalos_audit::execute_with(
        CURRENT_VERSION,
        Some(profile.as_str()),
        &guest,
        inputs,
        Mode::Record { seed: case.seed },
        |guest, host, profile| on_backend(backend, guest, host, profile),
    )
}

/// The Shared contract's test: S-04.1.e1, S-04.1.e2, S-04.2.e1 and S-04.3.e1, and the cases later
/// slices add.
#[test]
fn python_committed_evidence_replays_on_both_backends() {
    let cases = cases();
    let process_only = process_only();
    let kvm = if cases.iter().any(|c| c.both) {
        kvm_backend("python_committed_evidence_replays_on_both_backends")
    } else {
        None
    };
    let mut records = BTreeSet::new();
    for case in &cases {
        let runtime = Path::new(&case.guest)
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            case.both || process_only.contains(runtime) || process_only.contains(&case.id),
            "{}: listed process with no process-only line in spikes/438/README.md",
            case.id
        );
        let mut backends = vec![&Backend::Process];
        if case.both {
            backends.extend(kvm.as_ref());
        }
        let guest = fs::read(path(&case.guest)).unwrap();
        for backend in backends {
            let ran = run_case(case, backend);
            match (&case.outcome, case.record.as_ref()) {
                (Outcome::Refused(reason), None) => {
                    assert_eq!(
                        ran.map(|_| ()).map_err(|r| format!("{r:?}")),
                        Err(reason.clone()),
                        "{}",
                        case.id
                    );
                }
                (Outcome::Result { bytes, effects }, Some(record)) => {
                    let t = ran.unwrap_or_else(|r| panic!("{}: {r:?}", case.id));
                    assert_eq!(unhex(&t.result_hex).unwrap(), *bytes, "{}: result", case.id);
                    assert_eq!(t.effects.len(), *effects, "{}: effects", case.id);
                    let committed = fs::read(path(record))
                        .unwrap_or_else(|_| panic!("{}: no record {record}", case.id));
                    assert!(
                        written(&t) == committed,
                        "{}: the transcript differs from {record}",
                        case.id
                    );
                    let recorded = read_transcript(&committed).unwrap();
                    assert_eq!(
                        replay(backend, &guest, &recorded),
                        Ok(()),
                        "{}: replay",
                        case.id
                    );
                }
                _ => panic!(
                    "{}: a result needs a record and a refusal has none",
                    case.id
                ),
            }
        }
        if let Some(record) = &case.record {
            records.insert(record.clone());
        }
    }
    for entry in fs::read_dir(repo().join("spikes/438/evidence")).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if name.ends_with(".json") {
            let record = format!("spikes/438/evidence/{name}");
            assert!(records.contains(&record), "{record} is no case's record");
        }
    }
}

/// 3.3: the MicroPython triage run's notification and result are the digest guest's, under
/// `hyalos.profile/2` on the fixture's lines alone, for seeds 0 and 7, and the two seeds differ.
#[test]
fn python_3_3_micropython_triage_matches_the_digest_guest() {
    let guest = fs::read(path(MICROPYTHON)).unwrap();
    let digest = fs::read(path("hyalos-os/guest/digest.wasm")).unwrap();
    let mut notifications = Vec::new();
    for seed in [0, 7] {
        let inputs = spike::inputs(
            &path("spikes/438/contracts/draft"),
            &path("spikes/438/sdk/draft.py"),
            &path("spikes/438/scripts/triage.py"),
            &path(FIXTURE),
        )
        .unwrap();
        let python = hyalos_audit::execute_with(
            CURRENT_VERSION,
            Some(SPIKE),
            &guest,
            inputs,
            Mode::Record { seed },
            |guest, host, profile| on_backend(&Backend::Process, guest, host, profile),
        )
        .unwrap();
        let lines = read_inputs(&path(FIXTURE)).unwrap();
        let rust = execute(&Backend::Process, &digest, lines, Mode::Record { seed }).unwrap();
        assert_eq!(rust.profile.as_deref(), Some("hyalos.profile/2"));
        let bodies = |t: &Transcript| -> Vec<String> {
            t.effects.iter().map(|e| e.body_hex.clone()).collect()
        };
        assert_eq!(bodies(&python), bodies(&rust), "seed {seed}: notification");
        assert_eq!(python.result_hex, rust.result_hex, "seed {seed}: result");
        assert_eq!(bodies(&python).len(), 1, "seed {seed}");
        notifications.push(bodies(&python));
    }
    assert_ne!(
        notifications[0], notifications[1],
        "seeds 0 and 7 give one notification"
    );
}

/// S-04.4.e1: the MicroPython guest's imports, as `wasmi`'s module reader lists them, are the four
/// `hyalos` calls and nothing else, and it exports `run` and `memory`.
#[test]
fn python_s_04_4_e1_the_micropython_guest_imports_only_the_four_calls() {
    let guest = fs::read(path(MICROPYTHON)).unwrap();
    let module = wasmi::Module::new(&wasmi::Engine::default(), &guest).unwrap();
    let imports: BTreeSet<(String, String, bool)> = module
        .imports()
        .map(|i| {
            let func = matches!(i.ty(), wasmi::ExternType::Func(_));
            (i.module().to_string(), i.name().to_string(), func)
        })
        .collect();
    let four: BTreeSet<(String, String, bool)> = ["read", "infer", "effect", "return"]
        .iter()
        .map(|name| ("hyalos".to_string(), name.to_string(), true))
        .collect();
    assert_eq!(imports, four);
    let exports: BTreeSet<String> = module.exports().map(|e| e.name().to_string()).collect();
    assert!(
        exports.contains("run") && exports.contains("memory"),
        "{exports:?}"
    );
}

/// `value` as unsigned LEB128.
fn leb(mut value: usize, out: &mut Vec<u8>) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn read_leb(bytes: &[u8], at: &mut usize) -> usize {
    let (mut value, mut shift) = (0, 0);
    loop {
        let byte = bytes[*at];
        *at += 1;
        value |= usize::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return value;
        }
        shift += 7;
    }
}

/// A copy of `guest` with one more import, `env.leak`, a mutable `i32` global. The guest's own
/// globals are all mutable `i32`s, so the copy stays valid with every global index moved by one,
/// and the loader can refuse it only for the import.
fn with_leaked_import(guest: &[u8]) -> Vec<u8> {
    let mut out = guest[..8].to_vec();
    let mut at = 8;
    let mut added = false;
    while at < guest.len() {
        let id = guest[at];
        at += 1;
        let size = read_leb(guest, &mut at);
        let payload = &guest[at..at + size];
        at += size;
        if id != 2 {
            out.push(id);
            leb(size, &mut out);
            out.extend_from_slice(payload);
            continue;
        }
        let mut p = 0;
        let count = read_leb(payload, &mut p);
        let mut section = Vec::new();
        leb(count + 1, &mut section);
        section.extend_from_slice(&payload[p..]);
        section.extend_from_slice(&[
            3, b'e', b'n', b'v', 4, b'l', b'e', b'a', b'k', 0x03, 0x7f, 1,
        ]);
        out.push(2);
        leb(section.len(), &mut out);
        out.extend_from_slice(&section);
        added = true;
    }
    assert!(added, "the guest has no import section");
    out
}

/// S-04.4.e2: a build that leaks one other import is refused `BadImport` when it is loaded, on
/// both backends: the import check comes before the guest runs, so the default kernel image's
/// limit (`process-only: micropython`) does not reach it.
#[test]
fn python_s_04_4_e2_a_micropython_guest_with_one_more_import_is_refused() {
    let Some(kvm) = kvm_backend("python_s_04_4_e2") else {
        return;
    };
    let guest = with_leaked_import(&fs::read(path(MICROPYTHON)).unwrap());
    let module = wasmi::Module::new(&wasmi::Engine::default(), &guest).unwrap();
    assert_eq!(module.imports().len(), 5);
    for backend in [&Backend::Process, &kvm] {
        let inputs = spike::inputs(
            &path("spikes/438/contracts/draft"),
            &path("spikes/438/sdk/draft.py"),
            &path("spikes/438/scripts/triage.py"),
            &path(FIXTURE),
        )
        .unwrap();
        let ran = hyalos_audit::execute_with(
            CURRENT_VERSION,
            Some(SPIKE),
            &guest,
            inputs,
            Mode::Record { seed: 0 },
            |guest, host, profile| on_backend(backend, guest, host, profile),
        );
        assert_eq!(ran.map(|_| ()), Err(Reason::BadImport));
    }
}
