//! The framed inputs file and the version 4 input rule (S-02.1, S-02.2, S-02.6, 2.1 and 2.3 of
//! `docs/plans/hyalos-kernel-capsules`).
//!
//! The reader is shown at unit level. The refusals are shown through the `task-kernel` binary beside
//! this harness, on the process backend and, where KVM is usable, with the kernel image named:
//! `HYALOS_REQUIRE_KVM` turns a host without KVM into a failure.

use super::*;
use crate::inputs::{ReadError, read_inputs};
use crate::profile_tests::kvm_backend;
use crate::{inputs, runner};
use std::{
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

/// The records of a text file's non-empty lines, as a test fixture in the line form gives them.
pub(crate) fn fixture_lines(path: &Path) -> Vec<Vec<u8>> {
    inputs::lines(&fs::read(path).unwrap())
}

/// A text fixture's non-empty lines framed into `<dir>/<name>.inputs`, for a test that runs a
/// command on a file.
pub(crate) fn framed_fixture(fixture: &Path, dir: &Path) -> PathBuf {
    let out = dir.join(format!(
        "{}.inputs",
        fixture.file_stem().unwrap().to_str().unwrap()
    ));
    fs::write(&out, inputs::frame(&fixture_lines(fixture))).unwrap();
    out
}

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}

fn program() -> PathBuf {
    let path = env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("task-kernel");
    assert!(path.exists(), "the monitor is not built");
    path
}

/// A directory of one test's own, emptied first.
fn scratch(name: &str) -> PathBuf {
    let dir = env::temp_dir().join(format!("hyalos-inputs-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes `bytes` as an inputs file and reads it back through `read_inputs`.
fn read_back(name: &str, bytes: &[u8]) -> Result<Vec<Vec<u8>>, ReadError> {
    let dir = scratch(name);
    let path = dir.join("inputs");
    fs::write(&path, bytes).unwrap();
    let read = read_inputs(path.to_str().unwrap());
    let _ = fs::remove_dir_all(&dir);
    read
}

/// The framed form, written by hand from the layout in `inputs.rs`, so the reader is not checked
/// against its own writer.
fn by_hand(records: &[&[u8]]) -> Vec<u8> {
    let mut out = b"hyalos.inputs/2\n".to_vec();
    out.extend_from_slice(&(records.len() as u32).to_be_bytes());
    for r in records {
        out.extend_from_slice(&(r.len() as u32).to_be_bytes());
        out.extend_from_slice(r);
    }
    out
}

#[test]
fn capsules_s_02_1_e1_a_script_of_several_lines_keeps_every_newline_reader() {
    let script: &[u8] = b"from hyalos import task\n\nfor i in range(3):\n    print(i)\n";
    let got = read_back("s-02-1-e1", &by_hand(&[script, b"one\n"])).unwrap();
    assert_eq!(got, [script.to_vec(), b"one\n".to_vec()]);
    assert_eq!(got[0].iter().filter(|b| **b == b'\n').count(), 4);
}

#[test]
fn capsules_s_02_1_e2_an_empty_record_is_kept_reader() {
    let got = read_back("s-02-1-e2", &by_hand(&[b"a", b"", b"c"])).unwrap();
    assert_eq!(got, [b"a".to_vec(), vec![], b"c".to_vec()]);
    assert_eq!(
        read_back("s-02-1-e2-only", &by_hand(&[b""])).unwrap(),
        [Vec::<u8>::new()]
    );
}

#[test]
fn capsules_s_02_1_e3_bytes_that_are_not_utf8_come_back_the_same_reader() {
    // Not UTF-8 from the first byte, with a newline and a zero byte in it.
    let bytes: &[u8] = &[0xff, 0xfe, 0x00, b'\n', 0xc3, 0x28];
    let got = read_back("s-02-1-e3", &by_hand(&[bytes])).unwrap();
    assert_eq!(got, [bytes.to_vec()]);
}

/// The kernel image the KVM leg names, written where `HYALOS_KERNEL` can point, or `None` when
/// this host reports the KVM leg as skipped.
fn kernel_file(test: &str, dir: &Path) -> Option<PathBuf> {
    let Backend::Kvm { kernel, .. } = kvm_backend(test)? else {
        unreachable!("kvm_backend gives the KVM backend")
    };
    let path = dir.join("kernel");
    fs::write(&path, kernel).unwrap();
    Some(path)
}

/// `task-kernel <args>` with the kernel image named or not.
fn monitor(args: &[&Path], kernel: Option<&Path>, dir: &Path) -> Output {
    let mut command = Command::new(program());
    command
        .args(args)
        .current_dir(dir)
        .env_clear()
        .stdin(Stdio::null());
    if let Some(kernel) = kernel {
        command.env("HYALOS_KERNEL", kernel);
    }
    command.output().unwrap()
}

/// `run` on a malformed inputs file ends `refused: BadInputs`, exit 1, on each backend, and writes
/// no transcript. Its guest is not a module, so a guest that had loaded would end `InvalidModule`.
fn refused_before_the_guest_loads(test: &str, inputs: &[u8]) {
    let dir = scratch(test);
    fs::write(dir.join("inputs"), inputs).unwrap();
    fs::write(dir.join("guest"), b"not a module").unwrap();
    let kernel = kernel_file(test, &dir);
    for kernel in [None, kernel.as_deref()] {
        let output = monitor(
            &[
                Path::new("run"),
                &dir.join("guest"),
                &dir.join("inputs"),
                &dir.join("out.json"),
            ],
            kernel,
            &dir,
        );
        assert_eq!(output.status.code(), Some(1), "{test}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "refused: BadInputs\n"
        );
        assert!(
            !dir.join("out.json").exists(),
            "{test}: a transcript was written"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn capsules_s_02_2_e1_a_length_past_the_end_is_refused_before_the_guest_loads() {
    let mut past = by_hand(&[b"457\tfix a crash"]);
    past.truncate(past.len() - 1);
    assert_eq!(
        read_back("s-02-2-e1-unit", &past),
        Err(ReadError::BadInputs)
    );
    // The other ways a file is not the layout: its magic, its count and a length over the bound.
    let mut magic = by_hand(&[b"a"]);
    magic[14] = b'1';
    let mut count = by_hand(&[]);
    count[16..20].copy_from_slice(&(inputs::MAX_RECORDS as u32 + 1).to_be_bytes());
    let mut long = by_hand(&[]);
    long[19] = 1;
    long.extend_from_slice(&(kvm::RECORD_MAX as u32 + 1).to_be_bytes());
    for bad in [magic, count, long, b"457\tfix a crash\n".to_vec(), vec![]] {
        assert_eq!(
            read_back("s-02-2-e1-forms", &bad),
            Err(ReadError::BadInputs)
        );
    }
    // A file that cannot be read stays a usage error.
    assert_eq!(
        read_inputs("/nonexistent/inputs"),
        Err(ReadError::Usage("cannot read inputs".into()))
    );
    refused_before_the_guest_loads("capsules_s_02_2_e1", &past);
}

#[test]
fn capsules_s_02_2_e2_stray_bytes_after_the_last_record_are_refused_before_the_guest_loads() {
    let mut stray = by_hand(&[b"457\tfix a crash"]);
    stray.push(b'\n');
    assert_eq!(
        read_back("s-02-2-e2-unit", &stray),
        Err(ReadError::BadInputs)
    );
    refused_before_the_guest_loads("capsules_s_02_2_e2", &stray);
}

/// S-02.6.e1: each default command that starts a version 4 run refuses a framed record holding a
/// newline, an empty record and one that is not UTF-8, with `BadInputs`, before the guest loads.
#[test]
fn capsules_s_02_6_e1_a_version_4_run_refuses_a_record_holding_a_newline() {
    let dir = scratch("s-02-6-e1");
    let guest = root().join("guest/digest.wasm");
    let kernel = kernel_file("capsules_s_02_6_e1", &dir);
    let breaking: [&[u8]; 3] = [b"457\tfix a\ncrash", b"", &[b'4', 0xff]];
    for (at, record) in breaking.iter().enumerate() {
        let inputs = dir.join(format!("inputs-{at}"));
        fs::write(&inputs, by_hand(&[b"458\tadd a feature", record])).unwrap();
        let out = dir.join("out.json");
        let (count, issue, stub) = (Path::new("3"), Path::new("1"), Path::new("stub"));
        let mut commands: Vec<Vec<&Path>> = vec![
            vec![Path::new("run"), &guest, &inputs, &out],
            vec![Path::new("live"), &guest, &inputs, &out, stub, issue],
            vec![Path::new("fuzz"), &inputs, count],
            vec![Path::new("task"), &guest, &inputs],
            vec![Path::new("bench"), &guest, &inputs, count],
        ];
        if cfg!(feature = "test-switches") {
            commands.push(vec![Path::new("attacks"), &guest, &inputs]);
        }
        for args in &commands {
            let output = monitor(args, None, &dir);
            assert_eq!(
                (
                    output.status.code(),
                    String::from_utf8_lossy(&output.stderr)
                ),
                (Some(1), "refused: BadInputs\n".into()),
                "{:?} on record {at}",
                args[0]
            );
            assert!(output.stdout.is_empty(), "{:?} printed", args[0]);
        }
        // `run` on the kernel image too.
        if let Some(kernel) = &kernel {
            let output = monitor(&commands[0], Some(kernel), &dir);
            assert_eq!(
                String::from_utf8_lossy(&output.stderr),
                "refused: BadInputs\n"
            );
        }
        assert!(!out.exists(), "a transcript was written");
        assert!(!dir.join("ledger").exists(), "a ledger was opened");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// S-02.6.e2: a framed file of single-line records runs as the line form did: `run` writes the
/// transcript the same records give in memory, on each backend.
#[test]
fn capsules_s_02_6_e2_a_version_4_run_of_single_line_records_runs_as_today() {
    let dir = scratch("s-02-6-e2");
    let fixture = root().join("evidence/fixtures/issues.tsv");
    let framed = framed_fixture(&fixture, &dir);
    let guest = root().join("guest/digest.wasm");
    let kvm = kvm_backend("capsules_s_02_6_e2");
    let kernel = kernel_file("capsules_s_02_6_e2", &dir);
    let mut legs = vec![(Backend::Process, None)];
    if let (Some(kvm), Some(kernel)) = (kvm, kernel) {
        legs.push((kvm, Some(kernel)));
    }
    for (backend, kernel) in legs {
        let out = dir.join("out.json");
        let _ = fs::remove_file(&out);
        let output = monitor(
            &[Path::new("run"), &guest, &framed, &out],
            kernel.as_deref(),
            &dir,
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let want = execute(
            &backend,
            &fs::read(&guest).unwrap(),
            fixture_lines(&fixture),
            Mode::Record { seed: 0 },
        )
        .unwrap();
        assert_eq!(
            fs::read(&out).unwrap(),
            serde_json::to_vec_pretty(&want).unwrap()
        );
        assert_eq!(want.inputs_hex.len(), 40);
    }
    let _ = fs::remove_dir_all(&dir);
}

/// 2.1: `frame` then `read_inputs` gives back exactly the records given, positional files first,
/// and `--lines` gives one record per non-empty line. A missing file or too many records is a usage
/// error, exit 1, and nothing is written.
#[test]
fn capsules_2_1_frame_then_read_gives_back_the_records() {
    let dir = scratch("2-1");
    let records: [&[u8]; 3] = [b"line one\nline two\n", b"", &[0xff, b'\n', 0x00]];
    let mut args: Vec<PathBuf> = vec!["frame".into(), dir.join("out")];
    for (at, record) in records.iter().enumerate() {
        let path = dir.join(format!("record-{at}"));
        fs::write(&path, record).unwrap();
        args.push(path);
    }
    fs::write(dir.join("lines"), b"a\n\nb\tc\n\n").unwrap();
    args.extend(["--lines".into(), dir.join("lines")]);
    let refs: Vec<&Path> = args.iter().map(PathBuf::as_path).collect();
    let output = monitor(&refs, None, &dir);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "framed: 5 records\n"
    );
    let mut want: Vec<Vec<u8>> = records.iter().map(|r| r.to_vec()).collect();
    want.extend([b"a".to_vec(), b"b\tc".to_vec()]);
    assert_eq!(
        read_inputs(dir.join("out").to_str().unwrap()),
        Ok(want.clone())
    );
    assert_eq!(
        fs::read(dir.join("out")).unwrap(),
        by_hand(&want.iter().map(Vec::as_slice).collect::<Vec<_>>())
    );
    // A missing file, and one record too many, write nothing.
    fs::write(dir.join("many"), "x\n".repeat(inputs::MAX_RECORDS + 1)).unwrap();
    for (what, args) in [
        (
            "missing",
            vec![
                Path::new("frame"),
                &dir.join("none.out"),
                &dir.join("no-such-file"),
            ],
        ),
        (
            "too many",
            vec![
                Path::new("frame"),
                &dir.join("none.out"),
                Path::new("--lines"),
                &dir.join("many"),
            ],
        ),
    ] {
        let output = monitor(&args, None, &dir);
        assert_eq!(output.status.code(), Some(1), "{what}");
        assert!(
            String::from_utf8_lossy(&output.stderr).starts_with("refused: "),
            "{what}"
        );
        assert!(!dir.join("none.out").exists(), "{what}: a file was written");
    }
    // Exactly the cap frames.
    fs::write(dir.join("many"), "x\n".repeat(inputs::MAX_RECORDS)).unwrap();
    let output = monitor(
        &[
            Path::new("frame"),
            &dir.join("cap"),
            Path::new("--lines"),
            &dir.join("many"),
        ],
        None,
        &dir,
    );
    assert!(output.status.success());
    assert_eq!(
        read_inputs(dir.join("cap").to_str().unwrap())
            .unwrap()
            .len(),
        inputs::MAX_RECORDS
    );
    let _ = fs::remove_dir_all(&dir);
}

/// 2.3: `BadContract` has no feature gate and takes byte 29, `BadInputs` byte 30, in every build,
/// and the holder decides both, so neither is a runner's reason.
#[test]
fn capsules_2_3_bad_contract_takes_byte_29_and_bad_inputs_byte_30() {
    assert_eq!(runner::REASONS.len(), 31);
    assert_eq!(runner::REASONS[29], Reason::BadContract);
    assert_eq!(runner::REASONS[30], Reason::BadInputs);
    for reason in [Reason::BadContract, Reason::BadInputs] {
        assert!(!runner::RUNNER_REASONS.contains(&reason));
    }
    assert_eq!(ReadError::BadInputs.to_string(), "BadInputs");
}
