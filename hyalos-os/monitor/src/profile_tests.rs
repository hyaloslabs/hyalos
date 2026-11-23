//! Record version 4 and the one execution profile both backends build from (S-02).

use super::*;
use std::{path::{Path, PathBuf}, process::Command};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn committed(path: &str) -> Vec<u8> {
    fs::read(root().join(path)).unwrap()
}

fn fixture_inputs() -> Vec<Vec<u8>> {
    read_inputs(root().join("evidence/fixtures/issues.tsv").to_str().unwrap()).unwrap()
}

fn guest() -> Vec<u8> {
    committed("guest/digest.wasm")
}

fn v4_run() -> Transcript {
    execute(&Backend::Process, &guest(), fixture_inputs(), Mode::Record { seed: 0 }).unwrap()
}

fn with_field(t: &Transcript, field: &str, value: serde_json::Value) -> Vec<u8> {
    let mut record = serde_json::to_value(t).unwrap();
    record[field] = value;
    serde_json::to_vec(&record).unwrap()
}

#[derive(Debug, PartialEq, Eq)]
enum KvmGate {
    Run,
    Skip,
    Fail,
}

/// With the variable set, a host that cannot run the comparison fails instead of skipping.
fn kvm_gate(usable: bool, required: bool) -> KvmGate {
    match (usable, required) {
        (true, _) => KvmGate::Run,
        (false, true) => KvmGate::Fail,
        (false, false) => KvmGate::Skip,
    }
}

/// Builds the kernel image from this tree, so the comparison never runs a stale one.
fn built_kernel() -> Option<Vec<u8>> {
    let dir = root().join("kernel-image");
    let mut build = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    build.current_dir(&dir).args(["build", "--release", "--locked"]);
    for (name, _) in env::vars_os() {
        if name.to_str().is_some_and(|n| n.starts_with("CARGO") && n != "CARGO_HOME" || n == "RUSTFLAGS" || n == "RUSTC") {
            build.env_remove(name);
        }
    }
    if !build.output().ok()?.status.success() {
        return None;
    }
    fs::read(dir.join("target/x86_64-unknown-none/release/hyalos-kernel")).ok()
}

/// The KVM backend, or `None` when this host reports the comparison as skipped.
fn kvm_backend(test: &str) -> Option<Backend> {
    let usable = fs::OpenOptions::new().read(true).write(true).open("/dev/kvm").is_ok();
    let required = env::var_os("HYALOS_REQUIRE_KVM").is_some();
    match kvm_gate(usable, required) {
        KvmGate::Skip => {
            eprintln!("skipped: {test} needs KVM");
            return None;
        }
        KvmGate::Fail => panic!("{test}: HYALOS_REQUIRE_KVM is set and /dev/kvm cannot be opened"),
        KvmGate::Run => {}
    }
    match built_kernel() {
        Some(kernel) => Some(Backend::Kvm { kernel, limit: Duration::from_secs(30) }),
        None if required => panic!("{test}: HYALOS_REQUIRE_KVM is set and the kernel image did not build"),
        None => {
            eprintln!("skipped: {test} needs the x86_64-unknown-none target");
            None
        }
    }
}

/// The lines of a backend's mapping from a profile to its `wasmi` settings.
fn mapping(source: &str) -> Vec<&str> {
    let lines = source.lines().map(str::trim).filter(|l| l.starts_with('.') && !l.starts_with(".set_fuel") && l.contains("(profile.")).collect::<Vec<_>>();
    assert_eq!(lines.len(), 15, "{lines:?}");
    lines
}

#[test]
fn s_02_1_e1_both_backends_map_one_profile_the_same_way() {
    let process = include_str!("../../audit/src/replay.rs");
    let kernel = include_str!("../../kernel-image/src/main.rs");
    assert_eq!(mapping(process), mapping(kernel));
    assert!(kernel.contains("const PROFILE: Profile = PROFILE_1;"));
    for source in [process, include_str!("main.rs"), kernel] {
        for literal in [concat!("200_", "000_000"), concat!("4 * 1024", " * 1024"), concat!(".floats(", "false)")] {
            assert!(!source.contains(literal), "a setting is defined outside the profile: {literal}");
        }
    }
}

#[test]
fn s_02_1_e2_a_float_guest_is_refused_on_either_backend() {
    let wasm = wat::parse_str(with_run("(drop (f64.div (f64.const 0) (f64.const 0)))")).unwrap();
    assert_eq!(execute(&Backend::Process, &wasm, vec![], Mode::Record { seed: 0 }).err(), Some(Reason::InvalidModule));
    if let Some(kvm) = kvm_backend("s_02_1_e2") {
        assert_eq!(execute(&kvm, &wasm, vec![], Mode::Record { seed: 0 }).err(), Some(Reason::InvalidModule));
    }
}

#[test]
fn s_02_2_e1_a_new_run_is_version_4_and_names_profile_1() {
    let t = v4_run();
    assert_eq!((t.version, t.profile.as_deref()), (4, Some("hyalos.profile/1")));
    let written: serde_json::Value = serde_json::from_slice(&serde_json::to_vec(&t).unwrap()).unwrap();
    assert_eq!((&written["version"], &written["profile"]), (&serde_json::json!(4), &serde_json::json!("hyalos.profile/1")));
    assert_eq!(replay(&Backend::Process, &guest(), &read_transcript(&serde_json::to_vec(&t).unwrap()).unwrap()), Ok(()));
}

#[test]
fn s_02_2_e2_two_profile_names_give_two_task_identities() {
    let (hash, inputs) = (sha(&guest()), fixture_inputs());
    assert_ne!(task_v4("hyalos.profile/1", &hash, &inputs), task_v4("hyalos.profile/2", &hash, &inputs));
    assert_ne!(task_v4("hyalos.profile/1", &hash, &inputs), task_v2(&hash, &inputs));
}

#[test]
fn s_02_2_the_version_4_identity_is_the_documented_frame() {
    let hash = "ab".repeat(32);
    let inputs = vec![b"one".to_vec(), Vec::new()];
    let name = b"hyalos.profile/1";
    let mut frame = b"hyalos.task/4".to_vec();
    frame.extend((name.len() as u64).to_be_bytes());
    frame.extend(name);
    frame.extend([0xab; 32]);
    frame.extend(2u64.to_be_bytes());
    frame.extend(3u64.to_be_bytes());
    frame.extend(b"one");
    frame.extend(0u64.to_be_bytes());
    assert_eq!(task_v4("hyalos.profile/1", &hash, &inputs), Some(sha(&frame)));
}

#[test]
fn s_02_3_e1_every_committed_older_record_replays_byte_identically() {
    for (path, version) in [("evidence/out/live3.json", 1), ("evidence/out/two-machines-record/record.json", 2), ("evidence/out/two-machines-canary/record.json", 3)] {
        let bytes = committed(path);
        let t = read_transcript(&bytes).unwrap();
        assert_eq!((t.version, t.profile.as_deref()), (version, None), "{path}");
        assert_eq!(serde_json::to_vec_pretty(&t).unwrap(), bytes.trim_ascii_end(), "{path}");
        assert_eq!(replay(&Backend::Process, &guest(), &t), Ok(()), "{path}");
    }
}

#[test]
fn s_02_3_e2_a_version_3_record_holding_a_profile_is_refused() {
    let t = read_transcript(&committed("evidence/out/two-machines-canary/record.json")).unwrap();
    let bytes = with_field(&t, "profile", "hyalos.profile/1".into());
    assert_eq!(read_transcript(&bytes).unwrap_err(), "unknown field `profile` in a version 3 record");
}

#[test]
fn s_02_3_e2_a_profile_field_holding_null_is_refused_on_any_version() {
    let older = read_transcript(&committed("evidence/out/two-machines-canary/record.json")).unwrap();
    for t in [&older, &v4_run()] {
        assert_eq!(read_transcript(&with_field(t, "profile", serde_json::Value::Null)).unwrap_err(), "cannot parse", "version {}", t.version);
    }
}

#[test]
fn s_02_4_e1_an_unknown_profile_is_refused_and_named() {
    let bytes = with_field(&v4_run(), "profile", "hyalos.profile/9".into());
    assert_eq!(read_transcript(&bytes).unwrap_err(), "unknown profile hyalos.profile/9");
}

#[test]
fn s_02_4_a_profile_name_that_is_not_plain_is_not_echoed() {
    let bytes = with_field(&v4_run(), "profile", "secret\nvalue".into());
    assert_eq!(read_transcript(&bytes).unwrap_err(), "unknown profile <not a profile name>");
}

#[test]
fn s_02_4_e2_a_version_4_record_with_no_profile_is_refused() {
    let mut record = serde_json::to_value(v4_run()).unwrap();
    record.as_object_mut().unwrap().remove("profile");
    assert_eq!(read_transcript(&serde_json::to_vec(&record).unwrap()).unwrap_err(), "version 4 record names no profile");
}

#[test]
fn s_02_5_e1_both_backends_write_byte_identical_records() {
    let Some(kvm) = kvm_backend("s_02_5_e1") else { return };
    let (guest, inputs) = (guest(), fixture_inputs());
    let process = execute(&Backend::Process, &guest, inputs.clone(), Mode::Record { seed: 0 }).unwrap();
    let kernel = execute(&kvm, &guest, inputs, Mode::Record { seed: 0 }).unwrap();
    assert_eq!(serde_json::to_vec_pretty(&process).unwrap(), serde_json::to_vec_pretty(&kernel).unwrap());
    assert_eq!(replay(&kvm, &guest, &process), Ok(()));
}

#[test]
fn s_02_5_e2_no_kvm_reports_skipped_unless_it_is_required() {
    assert_eq!(kvm_gate(false, false), KvmGate::Skip);
    assert_eq!(kvm_gate(false, true), KvmGate::Fail);
    assert_eq!(kvm_gate(true, false), KvmGate::Run);
    assert_eq!(kvm_gate(true, true), KvmGate::Run);
    let ci = fs::read_to_string(root().join("../.github/workflows/ci.yml")).unwrap();
    let job = ci.split("\n  hyalos-os:\n").nth(1).unwrap().split("\n\n  #").next().unwrap();
    assert!(job.contains("HYALOS_REQUIRE_KVM: \"1\""), "the CI job must not skip the comparison");
}

/// The one `wasmi` version a lockfile holds.
fn locked_wasmi(lock: &str) -> &str {
    let mut versions = lock.split("[[package]]").filter(|p| p.contains("name = \"wasmi\"\n")).filter_map(|p| p.split("version = \"").nth(1)?.split('"').next());
    let version = versions.next().expect("no wasmi in the lockfile");
    assert_eq!(versions.next(), None, "two wasmi versions");
    version
}

#[test]
fn local_2_2_the_kernel_images_wasmi_is_the_workspaces_and_the_profiles() {
    let workspace = fs::read_to_string(root().join("Cargo.lock")).unwrap();
    let kernel = fs::read_to_string(root().join("kernel-image/Cargo.lock")).unwrap();
    assert_eq!(locked_wasmi(&kernel), locked_wasmi(&workspace));
    assert_eq!(locked_wasmi(&workspace), hyalos_profile::PROFILE_1.wasmi);
}
