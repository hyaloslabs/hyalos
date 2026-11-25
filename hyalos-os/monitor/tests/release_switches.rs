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
    let dir = root().join("kernel-image");
    let mut build = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    build
        .current_dir(&dir)
        .args(["build", "--release", "--locked", "--target-dir", "target"]);
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
    fs::read(dir.join("target/x86_64-unknown-none/release/hyalos-kernel")).unwrap()
}

/// `task-kernel run` of the committed guest on the fixtures, on the kernel image at `kernel`.
fn run(kernel: &Path, out: &Path, switches: &[(&str, &str)]) -> Output {
    program()
        .arg("run")
        .arg(root().join("guest/digest.wasm"))
        .arg(root().join("evidence/fixtures/issues.tsv"))
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
            .arg(root().join("evidence/fixtures/issues.tsv"))
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
