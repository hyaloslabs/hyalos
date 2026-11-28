//! A release build's test switches (S-05.4 of `docs/plans/hyalos-kernel-core`). No environment
//! variable and no raw kernel image changes how a build without the `test-switches` feature
//! behaves: its `attacks` command is refused before it reads a kernel file, its runs print no
//! timings, and a raw, non-ELF kernel image is refused with `KernelFault`.
//!
//! CI runs this file without the feature, against the default binary. Built with the feature, each
//! test checks the other column of the slice's contract instead, so the same test shows that the
//! feature is the only difference. The KVM tests need KVM and Landlock; `HYALOS_REQUIRE_KVM` or
//! `HYALOS_REQUIRE_LANDLOCK` turns a host without them into a failure.

use std::{
    env, fs,
    io::{Read as _, Write as _},
    os::unix::fs::OpenOptionsExt as _,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

/// Whether this test binary, and so the `task-kernel` binary beside it, has the feature.
const SWITCHES: bool = cfg!(feature = "test-switches");

/// The refusal a release build gives the `attacks` command.
const ATTACKS_REFUSED: &str =
    "refused: the attacks command needs a build with the test-switches feature\n";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn program() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_task-kernel"));
    command.env_clear().stdin(Stdio::null());
    command
}

/// The issue fixture framed by the binary's own `frame --lines`, once per test binary, in the
/// `hyalos.inputs/2` form every run reads (ADR-0138).
fn issues() -> PathBuf {
    static FRAMED: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FRAMED
        .get_or_init(|| {
            let out = env::temp_dir().join(format!(
                "hyalos-release-switches-issues-{}.inputs",
                std::process::id()
            ));
            let framed = program()
                .arg("frame")
                .arg(&out)
                .arg("--lines")
                .arg(root().join("evidence/fixtures/issues.tsv"))
                .output()
                .unwrap();
            assert!(framed.status.success(), "frame --lines failed");
            out
        })
        .clone()
}

/// A directory of this test's own, removed when it is dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let dir = env::temp_dir().join(format!(
            "hyalos-release-switches-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Whether this host can run a confined KVM run, asked of the binary itself
/// (`machine-runner --check`: KVM, Landlock and the seccomp filter). `false` when this host reports
/// the test as skipped; with either variable set, a host that cannot fails instead.
fn machine_gate(test: &str) -> bool {
    let status = program()
        .args(["machine-runner", "--check"])
        .status()
        .unwrap();
    if status.success() {
        return true;
    }
    let required = env::var_os("HYALOS_REQUIRE_KVM").is_some()
        || env::var_os("HYALOS_REQUIRE_LANDLOCK").is_some();
    assert!(
        !required,
        "{test}: a KVM test is required here and this host cannot run a confined machine"
    );
    eprintln!("skipped: {test} needs KVM and Landlock");
    false
}

/// The kernel image, built from this tree as the monitor's own tests build it.
fn built_kernel(test: &str) -> Vec<u8> {
    built_kernel_with(test, &[], "target")
}

/// The kernel image built with `features` into its own target directory, as `built_kernel`
/// builds the default one (`spikes/438/kernel.sh` uses the same directories).
fn built_kernel_with(test: &str, features: &[&str], target: &str) -> Vec<u8> {
    let dir = root().join("kernel-image");
    let mut build = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    build
        .current_dir(&dir)
        .args(["build", "--release", "--locked", "--target-dir", target]);
    for feature in features {
        build.args(["--features", feature]);
    }
    for (name, _) in env::vars_os() {
        if name.to_str().is_some_and(|n| {
            n.starts_with("CARGO") && n != "CARGO_HOME" || n == "RUSTFLAGS" || n == "RUSTC"
        }) {
            build.env_remove(name);
        }
    }
    assert!(
        build.output().unwrap().status.success(),
        "{test}: the kernel image did not build"
    );
    fs::read(
        dir.join(target)
            .join("x86_64-unknown-none/release/hyalos-kernel"),
    )
    .unwrap()
}

/// `task-kernel run` of the committed guest on the fixtures, on the kernel image at `kernel`.
fn run(kernel: &Path, out: &Path, switches: &[(&str, &str)]) -> Output {
    program()
        .arg("run")
        .arg(root().join("guest/digest.wasm"))
        .arg(issues())
        .arg(out)
        .env("HYALOS_KERNEL", kernel)
        .envs(switches.iter().copied())
        .output()
        .unwrap()
}

/// A named pipe. Opening one to read blocks until something opens it to write, so a program that
/// tries to read it never ends on its own.
fn fifo(at: &Path) {
    assert!(Command::new("mkfifo").arg(at).status().unwrap().success());
}

/// Opens `fifo` to write on a thread of its own, which reports once a reader has opened it.
fn watch(fifo: &Path) -> mpsc::Receiver<()> {
    let (opened, said) = mpsc::channel();
    let fifo = fifo.to_path_buf();
    thread::spawn(move || {
        if fs::OpenOptions::new().write(true).open(fifo).is_ok() {
            let _ = opened.send(());
        }
    });
    said
}

/// Waits up to `limit` for `child` to end.
fn ended(child: &mut Child, limit: Duration) -> Option<Output> {
    let start = Instant::now();
    while start.elapsed() < limit {
        if child.try_wait().unwrap().is_some() {
            return Some(child_output(child));
        }
        thread::sleep(Duration::from_millis(20));
    }
    None
}

fn child_output(child: &mut Child) -> Output {
    let status = child.wait().unwrap();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(mut out) = child.stdout.take() {
        out.read_to_end(&mut stdout).unwrap();
    }
    if let Some(mut err) = child.stderr.take() {
        err.read_to_end(&mut stderr).unwrap();
    }
    Output {
        status,
        stdout,
        stderr,
    }
}

/// S-05.4.e1: with `HYALOS_SELFTEST_KERNEL` set, or `HYALOS_KERNEL`, a release build's `attacks`
/// command exits non-zero, naming the feature, and never opens the file. Each is a named pipe the
/// test watches: a program that opens one to read is seen, and could not end without a writer.
#[test]
fn core_s_05_4_e1_attacks_reads_no_kernel_file() {
    let scratch = Scratch::new("attacks");
    for variable in ["HYALOS_SELFTEST_KERNEL", "HYALOS_KERNEL"] {
        let pipe = scratch.0.join(variable);
        fifo(&pipe);
        let opened = watch(&pipe);
        let mut child = program()
            .arg("attacks")
            .arg(root().join("guest/digest.wasm"))
            .arg(issues())
            .env(variable, &pipe)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if SWITCHES {
            // With the feature, the command reads the kernel file it names.
            let read = opened.recv_timeout(Duration::from_secs(120));
            let _ = child.kill();
            let _ = child.wait();
            assert!(
                read.is_ok(),
                "{variable}: a test build's attacks command did not read the file"
            );
        } else {
            let output = ended(&mut child, Duration::from_secs(30));
            if output.is_none() {
                let _ = child.kill();
            }
            let output = output.expect("a release build's attacks command did not end");
            assert_eq!(output.status.code(), Some(1), "{variable}");
            assert_eq!(output.stderr, ATTACKS_REFUSED.as_bytes(), "{variable}");
            assert!(output.stdout.is_empty(), "{variable}");
            assert!(
                opened.try_recv().is_err(),
                "{variable}: the file was opened"
            );
            // Release the watching thread.
            let _ = fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&pipe);
        }
    }
}

/// S-05.4.e2: a raw, non-ELF kernel image is refused with `KernelFault` when a run starts. The
/// image writes to a port that is not the kernel's, which a build with the feature loads and ends
/// in `BadHypercall`. The real kernel image, as a control, runs.
#[test]
fn core_s_05_4_e2_a_raw_kernel_image_is_refused() {
    if !machine_gate("core_s_05_4_e2") {
        return;
    }
    let scratch = Scratch::new("raw");
    let kernel = scratch.0.join("kernel");
    fs::write(&kernel, built_kernel("core_s_05_4_e2")).unwrap();
    let control = run(&kernel, &scratch.0.join("control.json"), &[]);
    assert!(
        control.status.success(),
        "the real kernel image did not run"
    );

    let raw = scratch.0.join("raw");
    // `mov dx, 0x80; out dx, eax`: no ELF header, so raw machine code.
    fs::File::create(&raw)
        .unwrap()
        .write_all(&[0x66, 0xba, 0x80, 0x00, 0xef])
        .unwrap();
    let output = run(&raw, &scratch.0.join("raw.json"), &[]);
    assert_eq!(output.status.code(), Some(1));
    let want: &[u8] = if SWITCHES {
        b"refused: BadHypercall\n"
    } else {
        b"refused: KernelFault\n"
    };
    assert_eq!(output.stderr, want);
    assert!(output.stdout.is_empty());
    assert!(!scratch.0.join("raw.json").exists());
}

/// S-05.4: `HYALOS_TRACE` and `HYALOS_HUGE` change nothing a release build does: a run with both
/// set prints and writes exactly what a run without them does, and no timing line. With the
/// feature, the trace switch prints its timings.
#[test]
fn core_s_05_4_trace_and_huge_change_nothing() {
    if !machine_gate("core_s_05_4 trace") {
        return;
    }
    let scratch = Scratch::new("trace");
    let kernel = scratch.0.join("kernel");
    fs::write(&kernel, built_kernel("core_s_05_4 trace")).unwrap();
    let plain = run(&kernel, &scratch.0.join("plain.json"), &[]);
    let switched = run(
        &kernel,
        &scratch.0.join("switched.json"),
        &[("HYALOS_TRACE", "1"), ("HYALOS_HUGE", "1")],
    );
    assert!(plain.status.success() && switched.status.success());
    assert_eq!(plain.stdout, switched.stdout);
    assert_eq!(
        fs::read(scratch.0.join("plain.json")).unwrap(),
        fs::read(scratch.0.join("switched.json")).unwrap()
    );
    assert!(plain.stderr.is_empty());
    if SWITCHES {
        assert!(
            switched.stderr.starts_with(b"trace ms: memory+load "),
            "a test build printed no timings"
        );
    } else {
        assert!(
            switched.stderr.is_empty(),
            "a release build printed something under the switches"
        );
    }
}

/// S-05.4: the monitor names its test-only environment variables in one file, `switches.rs`, which
/// is compiled only with the feature. Its tests name them only in code compiled with it too.
#[test]
fn core_s_05_4_every_switch_is_named_only_in_code_built_with_the_feature() {
    let names = ["HYALOS_SELFTEST_KERNEL", "HYALOS_TRACE", "HYALOS_HUGE"];
    let src = root().join("monitor/src");
    let main = fs::read_to_string(src.join("main.rs")).unwrap();
    assert!(main.contains("\n#[cfg(feature = \"test-switches\")]\nmod switches;\n"));
    let switches = fs::read_to_string(src.join("switches.rs")).unwrap();
    for name in names {
        assert!(
            switches.contains(&format!("(\"{name}\")")),
            "switches.rs does not read {name}"
        );
    }
    let mut files: Vec<PathBuf> = fs::read_dir(&src)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    for file in files {
        let label = file.file_name().unwrap().to_string_lossy().into_owned();
        if label == "switches.rs" {
            continue;
        }
        let text = fs::read_to_string(&file).unwrap();
        for name in names {
            for (at, _) in text.match_indices(name) {
                // A test may name one only inside a test compiled with the feature.
                let before = &text[..at];
                let item = before
                    .rfind("\n#[test]\n")
                    .unwrap_or_else(|| panic!("{label} names {name} outside a test"));
                assert!(label.ends_with("_tests.rs"), "{label} names {name}");
                assert!(
                    before[..item].ends_with("\n#[cfg(feature = \"test-switches\")]"),
                    "{label} names {name} in a test built without the feature"
                );
            }
        }
    }
}

/// CI's `hyalos-os` job runs this file without the feature, against the default binary, with KVM
/// and Landlock required, after the workspace's tests with the feature.
#[test]
fn core_s_05_4_ci_runs_this_file_without_the_feature() {
    let ci = fs::read_to_string(root().join("../.github/workflows/ci.yml")).unwrap();
    let job = ci
        .split("\n  hyalos-os:\n")
        .nth(1)
        .unwrap()
        .split("\n\n  #")
        .next()
        .unwrap();
    let with = job
        .find("--features task-kernel/test-trust,hyalos-audit/test-trust,task-kernel/test-switches")
        .expect("the workspace's tests run with the feature");
    let step = "run: cargo test --release --locked --manifest-path hyalos-os/Cargo.toml -p task-kernel --test release_switches\n";
    let without = job.find(step).expect("the release_switches step");
    assert!(with < without);
    let env = &job[without + step.len()..];
    let env = &env[..env.find("\n      - ").unwrap_or(env.len())];
    assert!(
        env.contains("HYALOS_REQUIRE_KVM: \"1\"") && env.contains("HYALOS_REQUIRE_LANDLOCK: \"1\"")
    );
}

/// Whether this test binary, and so the `task-kernel` binary beside it, is a spike build
/// (ADR-0136, `docs/plans/hyalos-kernel-python`).
const SPIKE: bool = cfg!(feature = "spike-contract");

/// The five files a spike command reads: the committed guest, the draft contract, an SDK, a
/// script and the issue fixture.
fn spike_files(scratch: &Scratch) -> Vec<PathBuf> {
    let file = |name: &str, bytes: &[u8]| {
        let path = scratch.0.join(name);
        fs::write(&path, bytes).unwrap();
        path
    };
    vec![
        root().join("guest/digest.wasm"),
        file("contract", br#"{"asks":[0],"may_touch":[0]}"#),
        file("sdk.py", b"def _nothing():\n    pass\n"),
        file("script.py", b"from hyalos import task\n"),
        root().join("evidence/fixtures/issues.tsv"),
    ]
}

/// S-01.4: a default build has no `spike-run` or `spike-task` command, and `HYALOS_SPIKE_PROFILE`
/// changes nothing `run` does. Built with the feature, both commands run.
#[test]
fn python_s_01_4_a_default_build_has_no_spike_command_and_ignores_its_variable() {
    let scratch = Scratch::new("spike-commands");
    let files = spike_files(&scratch);
    let usage = "refused: usage: run|replay|task|attacks|bench|probe-host ...\n";
    let record = scratch.0.join("spike.json");
    for (command, out) in [("spike-task", None), ("spike-run", Some(&record))] {
        let output = program()
            .arg(command)
            .args(&files)
            .args(out)
            .output()
            .unwrap();
        if SPIKE {
            assert!(output.status.success(), "{command}");
        } else {
            assert!(!output.status.success(), "{command}");
            assert_eq!(String::from_utf8_lossy(&output.stderr), usage);
            assert!(output.stdout.is_empty());
        }
    }
    assert_eq!(record.exists(), SPIKE);
    let run = |out: &str, variable: Option<&str>| {
        let mut command = program();
        command
            .arg("run")
            .arg(root().join("guest/digest.wasm"))
            .arg(issues())
            .arg(scratch.0.join(out));
        if let Some(name) = variable {
            command.env("HYALOS_SPIKE_PROFILE", name);
        }
        let output = command.output().unwrap();
        assert!(output.status.success());
        (output.stdout, fs::read(scratch.0.join(out)).unwrap())
    };
    let plain = run("plain.json", None);
    assert_eq!(plain, run("spike.json", Some("hyalos.spike/1")));
    assert!(String::from_utf8_lossy(&plain.1).contains("\"profile\": \"hyalos.profile/2\""));
}

/// 1.5: `spike-task` prints the task `spike-run` records for the same five files, and a spike run
/// prints its timing after its `ok:` line. A profile variable that names no spike profile is
/// refused before anything runs, with no record.
#[cfg(feature = "spike-contract")]
#[test]
fn python_1_5_spike_task_equals_record_task() {
    let scratch = Scratch::new("spike-task");
    let files = spike_files(&scratch);
    let task = program().arg("spike-task").args(&files).output().unwrap();
    assert!(task.status.success());
    let record = scratch.0.join("record.json");
    let run = program()
        .arg("spike-run")
        .args(&files)
        .arg(&record)
        .output()
        .unwrap();
    assert!(run.status.success());
    let written: serde_json::Value = serde_json::from_slice(&fs::read(&record).unwrap()).unwrap();
    assert_eq!(written["profile"], "hyalos.spike/1");
    assert_eq!(
        String::from_utf8_lossy(&task.stdout).trim_end(),
        written["task"].as_str().unwrap()
    );
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    let mut lines = stdout.lines();
    assert!(lines.next().unwrap().starts_with("ok: 43 inputs, "));
    assert!(lines.next().unwrap().starts_with("timing: cold_start "));
    for name in ["hyalos.profile/2", "hyalos.spike/9", "nonsense"] {
        let refused = scratch.0.join("refused.json");
        let output = program()
            .arg("spike-run")
            .args(&files)
            .arg(&refused)
            .env("HYALOS_SPIKE_PROFILE", name)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{name}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "refused: HYALOS_SPIKE_PROFILE does not name a known spike profile\n"
        );
        assert!(!refused.exists());
    }
    // A contract that is not one is refused, BadContract, by both commands, with no record.
    let bad = scratch.0.join("contract");
    fs::write(&bad, b"{\"asks\":[0], \"may_touch\":[0]}").unwrap();
    for (command, out) in [("spike-task", None), ("spike-run", Some(&record))] {
        let _ = fs::remove_file(&record);
        let output = program()
            .arg(command)
            .args(&files)
            .args(out)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{command}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "refused: BadContract\n"
        );
        assert!(!record.exists());
    }
}

/// 4.2 (`design.md` §2 of docs/plans/hyalos-kernel-python, KVM under a spike profile, ADR-0137): a
/// spike build runs a profile on KVM only on an image built for that profile's settings. The
/// default image, which carries no marker, still runs profile 2 and `hyalos.spike/1`; the
/// `spike-profile-2` image runs `hyalos.spike/2` and neither of those; a run on the wrong image
/// ends `ReplayDiverged` with no record; and an image whose marker names an unknown profile is
/// refused before boot, with a message and no record.
#[cfg(feature = "spike-contract")]
#[test]
fn python_4_2_a_spike_profile_runs_only_on_its_own_image() {
    if !machine_gate("python_4_2 images") {
        return;
    }
    let scratch = Scratch::new("spike-images");
    let files = spike_files(&scratch);
    let default = scratch.0.join("default-kernel");
    fs::write(&default, built_kernel("python_4_2 images")).unwrap();
    let spike_2 = built_kernel_with("python_4_2 images", &["spike-profile-2"], "target-spike-2");
    let unknown = scratch.0.join("unknown-kernel");
    // The marker and the name together: the kernel also holds the name alone, as `SPIKE_2.name`.
    let marked = b"\0hyalos-kernel-profile=hyalos.spike/2";
    let at = spike_2
        .windows(marked.len())
        .position(|w| w == marked)
        .unwrap();
    let mut renamed = spike_2.clone();
    renamed[at + marked.len() - 1] = b'9';
    fs::write(&unknown, renamed).unwrap();
    let spike_2_path = scratch.0.join("spike-2-kernel");
    fs::write(&spike_2_path, spike_2).unwrap();
    // One run on `kernel`: `spike-run` under `profile`, or `run` under profile 2 when it is `None`.
    let run = |kernel: &Path, profile: Option<&str>| -> (String, bool) {
        let record = scratch.0.join("record.json");
        let _ = fs::remove_file(&record);
        let mut command = program();
        match profile {
            Some(name) => command
                .arg("spike-run")
                .args(&files)
                .arg(&record)
                .env("HYALOS_SPIKE_PROFILE", name),
            None => command
                .arg("run")
                .arg(root().join("guest/digest.wasm"))
                .arg(issues())
                .arg(&record),
        };
        let output = command.env("HYALOS_KERNEL", kernel).output().unwrap();
        let said = [output.stdout, output.stderr].concat();
        let first = String::from_utf8_lossy(&said)
            .lines()
            .next()
            .unwrap_or("")
            .to_string();
        (first, record.exists())
    };
    let ok = |(line, recorded): (String, bool)| line.starts_with("ok: ") && recorded;
    let diverged = (String::from("refused: ReplayDiverged"), false);
    assert!(ok(run(&default, None)));
    assert!(ok(run(&default, Some("hyalos.spike/1"))));
    assert_eq!(run(&default, Some("hyalos.spike/2")), diverged);
    assert!(ok(run(&spike_2_path, Some("hyalos.spike/2"))));
    assert_eq!(run(&spike_2_path, Some("hyalos.spike/1")), diverged);
    assert_eq!(run(&spike_2_path, None), diverged);
    let refused = (
        String::from("refused: the kernel image names no profile this build knows"),
        false,
    );
    for profile in [None, Some("hyalos.spike/1"), Some("hyalos.spike/2")] {
        assert_eq!(run(&unknown, profile), refused, "{profile:?}");
    }
}
