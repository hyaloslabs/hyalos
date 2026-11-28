//! The machine runner's confinement (S-04.2, S-04.3 and 7.1 of `docs/plans/hyalos-kernel-core`,
//! ADR-0132).
//!
//! The hostile runner and the injected confinement failures exist only with the `test-switches`
//! feature, so the tests that use them do too. They start the real `machine-runner` from the
//! `task-kernel` binary beside this harness and need KVM and Landlock: `HYALOS_REQUIRE_KVM` and
//! `HYALOS_REQUIRE_LANDLOCK` turn a host without either into a failure.

use super::*;
use crate::runner;
use std::path::Path;

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}

/// Whether this kernel has Landlock turned on. Asking restricts nothing.
pub(crate) fn landlock_available() -> bool {
    use landlock::{ABI, Access, AccessFs, CompatLevel, Compatible, Ruleset, RulesetAttr};
    Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(ABI::V1))
        .and_then(|r| r.create())
        .is_ok()
}

/// `false` when this host reports the test as skipped. With `HYALOS_REQUIRE_LANDLOCK` set, a host
/// without Landlock fails instead.
#[cfg_attr(not(feature = "test-switches"), allow(dead_code))]
fn landlock_gate(test: &str) -> bool {
    if landlock_available() {
        return true;
    }
    assert!(
        env::var_os("HYALOS_REQUIRE_LANDLOCK").is_none(),
        "{test}: HYALOS_REQUIRE_LANDLOCK is set and this kernel has no Landlock"
    );
    eprintln!("skipped: {test} needs Landlock");
    false
}

/// The `task-kernel` binary built beside this harness.
#[cfg_attr(not(feature = "test-switches"), allow(dead_code))]
fn program() -> std::path::PathBuf {
    env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("task-kernel")
}

/// S-04.2 and P2 of the design's pre-mortem: the seccomp allow-list is exactly the measured one,
/// so no system call joins it unreviewed, and none of them opens a file, uses a network, starts or
/// signals a process or changes memory's protection.
#[test]
fn core_s_04_2_the_allow_list_is_pinned_and_reaches_nothing() {
    let names: Vec<&str> = runner::ALLOWED.iter().map(|(name, _)| *name).collect();
    assert_eq!(
        names,
        [
            "ioctl",
            "recvfrom",
            "sendto",
            "write",
            "mmap",
            "munmap",
            "brk",
            "clock_gettime",
            "close",
            "exit_group"
        ]
    );
    let numbers: Vec<i64> = runner::ALLOWED.iter().map(|(_, number)| *number).collect();
    assert_eq!(
        numbers,
        [16, 45, 44, 1, 9, 11, 12, 228, 3, 231],
        "x86-64 numbers"
    );
    for forbidden in [
        libc::SYS_open,
        libc::SYS_openat,
        libc::SYS_openat2,
        libc::SYS_creat,
        libc::SYS_socket,
        libc::SYS_connect,
        libc::SYS_execve,
        libc::SYS_execveat,
        libc::SYS_fork,
        libc::SYS_vfork,
        libc::SYS_clone,
        libc::SYS_clone3,
        libc::SYS_kill,
        libc::SYS_tkill,
        libc::SYS_tgkill,
        libc::SYS_mprotect,
        libc::SYS_mremap,
        libc::SYS_ptrace,
        libc::SYS_process_vm_writev,
        libc::SYS_read,
    ] {
        assert!(!numbers.contains(&forbidden), "system call {forbidden}");
    }
    // Each restriction is in the filter's source, beside the list.
    let source = include_str!("runner.rs");
    for restriction in [
        "fd(0, vcpu),",
        "when(1, SeccompCmpArgLen::Dword, SeccompCmpOp::Eq, KVM_RUN),",
        "\"recvfrom\" => vec![rule(vec![fd(0, channel)])?],",
        "when(4, SeccompCmpArgLen::Qword, SeccompCmpOp::Eq, 0),",
        "\"write\" if trace => vec![rule(vec![fd(0, 2)])?],",
        "\"write\" => continue,",
        "when(2, SeccompCmpArgLen::Qword, SeccompCmpOp::MaskedEq(exec), 0),",
        "libc::CLOCK_MONOTONIC as u64,",
        "SeccompAction::KillProcess,",
    ] {
        assert!(source.contains(restriction), "{restriction}");
    }
}

/// S-04.3: no guest instruction runs before both confinements hold. Inside the runner, Landlock
/// comes before the `Boot` frame is read, and inside the machine's run, the seccomp filter comes
/// after the machine is built and before its virtual CPU first runs.
#[test]
fn core_s_04_3_both_confinements_hold_before_any_guest_instruction() {
    let runner_source = include_str!("runner.rs");
    let command = &runner_source[runner_source.find("pub(crate) fn command(").unwrap()..];
    let at = |text: &str| command.find(text).unwrap();
    assert!(at("close_inherited()") < at("kvm_ioctls::Kvm::new()"));
    assert!(at("kvm_ioctls::Kvm::new()") < at("if !landlock("));
    assert!(at("if !landlock(") < at("read_frame(&mut channel.0, BOOT_LIMIT"));
    assert!(at("read_frame(&mut channel.0, BOOT_LIMIT") < at("kvm::machine_run("));
    let kvm = include_str!("kvm.rs");
    let run = &kvm[kvm.find("fn boot_and_run(").unwrap()..];
    let run = &run[..run.find("\n}\n").unwrap()];
    let at = |text: &str| run.find(text).unwrap();
    assert!(at("machine(&kvm, &ram") < at("if !seal(&vcpu)"));
    assert!(at("if !seal(&vcpu)") < at("vcpu.run()"));
    assert_eq!(run.matches("vcpu.run()").count(), 1);
    assert!(run.contains("if !seal(&vcpu) {\n        return Err(Reason::KernelFault);"));
}

/// 7.1: a repository ADR records the confinement, and the index lists it.
#[test]
fn core_7_1_a_repository_adr_records_the_confinement() {
    let adr = fs::read_to_string(
        root().join("../docs/adr/0132-the-hyalos-machine-runner-is-confined.md"),
    )
    .unwrap();
    assert!(adr.starts_with("# ADR-0132: The hyalOS machine runner is confined"));
    for needed in [
        "Landlock",
        "seccomp",
        "seccompiler",
        "--check",
        "KernelFault",
    ] {
        assert!(adr.contains(needed), "{needed}");
    }
    let index = fs::read_to_string(root().join("../docs/adr/README.md")).unwrap();
    assert!(
        index.contains("| [0132](0132-the-hyalos-machine-runner-is-confined.md) | The hyalOS machine runner is confined |")
    );
}

/// The CI job requires Landlock, so these tests cannot skip there.
#[test]
fn core_s_04_3_the_ci_job_requires_landlock_and_runs_the_test_switches() {
    let ci = fs::read_to_string(root().join("../.github/workflows/ci.yml")).unwrap();
    let job = ci
        .split("\n  hyalos-os:\n")
        .nth(1)
        .unwrap()
        .split("\n\n  #")
        .next()
        .unwrap();
    assert!(job.contains("HYALOS_REQUIRE_LANDLOCK: \"1\""));
    assert!(job.contains("task-kernel/test-switches"));
}

#[cfg(feature = "test-switches")]
mod hostile {
    use super::*;
    use crate::inputs_tests::{fixture_lines, framed_fixture};
    use crate::profile_tests::kvm_backend;
    use crate::runner::{Timing, hostile::Act};
    use hyalos_audit::{Asked, Source};
    use std::{
        os::unix::{net::UnixListener, process::ExitStatusExt},
        process::{Command, ExitStatus, Stdio},
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };

    /// A source that counts what it is asked.
    #[derive(Clone, Default)]
    struct Counting(Arc<AtomicUsize>);

    impl Source for Counting {
        fn infer(&mut self, _: usize, _: i32, q: &[u8], _: &str) -> Result<Asked, Reason> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(Asked {
                label: stub_model(q, 0),
                model: "stub".into(),
                response_id: String::new(),
                attestation: String::new(),
                provider_proofs: Vec::new(),
            })
        }
        fn effect(&mut self, _: usize, _: i32, _: &[u8], _: &str) -> Result<String, Reason> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(String::new())
        }
    }

    /// How a run on a runner started with `args` ended: the holder's outcome, how many frames the
    /// runner sent, how many questions and effects the source was asked for, and how the runner
    /// process ended. `None` when the host reports the test as skipped.
    struct Ended {
        outcome: Result<(), Reason>,
        frames: u32,
        asked: usize,
        status: ExitStatus,
    }

    fn run_on(test: &str, args: &[&str], guest: &[u8]) -> Option<Ended> {
        let Some(Backend::Kvm { kernel, .. }) = kvm_backend(test) else {
            return None;
        };
        if !landlock_gate(test) {
            return None;
        }
        let runner::Runner {
            mut child,
            mut stream,
        } = runner::start(args).unwrap();
        let counting = Counting::default();
        let inputs = fixture_lines(&root().join("evidence/fixtures/issues.tsv"));
        let mut timing = Timing::default();
        let outcome = runner::serve(
            &mut stream,
            &runner::boot(&kernel, guest).unwrap(),
            hyalos_audit::Host::new(counting.clone(), inputs),
            Instant::now() + Duration::from_secs(30),
            &mut timing,
        )
        .map(|_| ());
        drop(stream);
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the runner did not end"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        Some(Ended {
            outcome,
            frames: timing.requests,
            asked: counting.0.load(Ordering::SeqCst),
            status,
        })
    }

    /// A hostile runner's attempt at `target`, which must end with the runner killed by `SIGSYS`
    /// and the holder ending the run with `KernelFault`, having asked its source nothing.
    fn killed(test: &str, act: &str, target: &[u8]) -> bool {
        assert!(Act::named(act).is_some());
        let Some(ended) = run_on(test, &["--hostile", act], target) else {
            return false;
        };
        assert_eq!(
            ended.status.signal(),
            Some(libc::SIGSYS),
            "{test}: the runner was not killed by its filter (exit {:?})",
            ended.status.code()
        );
        assert_eq!(ended.outcome, Err(Reason::KernelFault), "{test}");
        assert_eq!(
            (ended.frames, ended.asked),
            (0, 0),
            "{test}: no further effect"
        );
        true
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = env::temp_dir().join(format!("hyalos-hostile-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// S-04.2.e1: a sealed runner that opens the operator's home directory, or creates a file, is
    /// killed, and no file appears.
    #[test]
    fn core_s_04_2_e1_a_sealed_runner_that_opens_the_home_directory_is_killed() {
        let home = env::var_os("HOME").unwrap_or_else(|| "/".into());
        if !killed("core_s_04_2_e1 open", "open", home.as_encoded_bytes()) {
            return;
        }
        let dir = scratch("create");
        let file = dir.join("written");
        assert!(killed(
            "core_s_04_2_e1 create",
            "create",
            file.as_os_str().as_encoded_bytes()
        ));
        assert!(!file.exists(), "the file was created");
        let _ = fs::remove_dir_all(&dir);
    }

    /// S-04.2.e1, Landlock alone: before the seccomp filter, a runner that creates a file is refused
    /// by Landlock, and no file appears.
    #[test]
    fn core_s_04_2_e1_landlock_alone_refuses_a_file() {
        let dir = scratch("landlock");
        let file = dir.join("written");
        let Some(ended) = run_on(
            "core_s_04_2_e1 landlock",
            &["--hostile", "create-under-landlock"],
            file.as_os_str().as_encoded_bytes(),
        ) else {
            return;
        };
        assert_eq!(ended.status.code(), Some(runner::hostile::DENIED));
        assert!(!file.exists(), "the file was created");
        assert_eq!(
            (ended.outcome, ended.frames, ended.asked),
            (Err(Reason::KernelFault), 0, 0)
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// S-04.2.e2: a sealed runner that connects a socket is killed, and the listener is never
    /// reached.
    #[test]
    fn core_s_04_2_e2_a_sealed_runner_that_connects_a_socket_is_killed() {
        let dir = scratch("connect");
        let path = dir.join("socket");
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        if !killed(
            "core_s_04_2_e2",
            "connect",
            path.as_os_str().as_encoded_bytes(),
        ) {
            return;
        }
        let reached = listener.accept();
        assert!(
            matches!(&reached, Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
            "the listener was reached"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// S-04.2.e3: a sealed runner that starts a program, or replaces itself with one, is killed,
    /// and the program never runs.
    #[test]
    fn core_s_04_2_e3_a_sealed_runner_that_starts_a_program_is_killed() {
        for act in ["spawn", "exec"] {
            let dir = scratch(act);
            let marker = dir.join("ran");
            if !killed("core_s_04_2_e3", act, marker.as_os_str().as_encoded_bytes()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
            assert!(!marker.exists(), "{act}: the program ran");
            let _ = fs::remove_dir_all(&dir);
        }
    }

    /// S-04.2: a sealed runner that signals another process is killed, and the other process
    /// lives on.
    #[test]
    fn core_s_04_2_a_sealed_runner_that_signals_another_process_is_killed() {
        let mut victim = Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let pid = victim.id().to_string();
        let ran = killed("core_s_04_2 signal", "signal", pid.as_bytes());
        let alive = victim.try_wait().unwrap().is_none();
        let _ = victim.kill();
        let _ = victim.wait();
        if ran {
            assert!(alive, "the other process was signalled");
        }
    }

    /// S-04.2: a sealed runner that maps memory it could execute is killed: the filter allows
    /// `mmap` only without execute permission, and `mprotect` not at all.
    #[test]
    fn core_s_04_2_a_sealed_runner_that_maps_executable_memory_is_killed() {
        killed("core_s_04_2 map", "map-executable", b"");
    }

    /// S-04.2: a sealed runner may read the monotonic clock with the system call itself, as the
    /// standard library's `Instant` does on a host whose clock source has no vDSO path. Without
    /// it, every run on such a host would die by `SIGSYS` just after sealing.
    #[test]
    fn core_s_04_2_a_sealed_runner_may_read_the_monotonic_clock() {
        let Some(ended) = run_on("core_s_04_2 clock", &["--hostile", "read-clock"], b"") else {
            return;
        };
        assert_eq!(
            (ended.status.signal(), ended.status.code()),
            (None, Some(runner::hostile::SURVIVED)),
            "the runner did not survive reading the clock"
        );
    }

    /// S-04.2: a runner whose guest waits on its first question is sealed: its filter is
    /// installed, and it can gain no privilege.
    #[test]
    fn core_s_04_2_a_running_runner_is_sealed() {
        let Some(Backend::Kvm { kernel, .. }) = kvm_backend("core_s_04_2 sealed") else {
            return;
        };
        if !landlock_gate("core_s_04_2 sealed") {
            return;
        }
        let guest = fs::read(root().join("guest/digest.wasm")).unwrap();
        let runner::Runner { child, mut stream } = runner::start(&[]).unwrap();
        let pid = child.id();
        let seen: Arc<std::sync::Mutex<Option<String>>> = Arc::default();
        struct Peek(Arc<std::sync::Mutex<Option<String>>>, u32);
        impl Source for Peek {
            fn infer(&mut self, _: usize, _: i32, q: &[u8], _: &str) -> Result<Asked, Reason> {
                let mut seen = self.0.lock().unwrap();
                if seen.is_none() {
                    *seen = fs::read_to_string(format!("/proc/{}/status", self.1)).ok();
                }
                Ok(Asked {
                    label: stub_model(q, 0),
                    model: "stub".into(),
                    response_id: String::new(),
                    attestation: String::new(),
                    provider_proofs: Vec::new(),
                })
            }
            fn effect(&mut self, _: usize, _: i32, _: &[u8], _: &str) -> Result<String, Reason> {
                Ok(String::new())
            }
        }
        let inputs = fixture_lines(&root().join("evidence/fixtures/issues.tsv"));
        let outcome = runner::serve(
            &mut stream,
            &runner::boot(&kernel, &guest).unwrap(),
            hyalos_audit::Host::new(Peek(seen.clone(), pid), inputs),
            Instant::now() + Duration::from_secs(30),
            &mut Timing::default(),
        );
        runner::Runner { child, stream }.end();
        assert!(outcome.is_ok());
        let status = seen.lock().unwrap().take().unwrap();
        let field = |name: &str| {
            status
                .lines()
                .find_map(|l| l.strip_prefix(name))
                .map(str::trim)
                .map(str::to_string)
        };
        assert_eq!(field("Seccomp:").as_deref(), Some("2"), "a seccomp filter");
        assert_eq!(field("NoNewPrivs:").as_deref(), Some("1"));
    }

    /// The run of the committed digest guest, with the holder's runners failing `confinement`.
    fn refused_run(test: &str, pretend: &str) -> Option<Ended> {
        let guest = fs::read(root().join("guest/digest.wasm")).unwrap();
        run_on(test, &[pretend], &guest)
    }

    /// The `live` and `run` commands on a host whose runner fails `confinement`, injected through
    /// the holder: what each printed, and whether the chain or the transcript appeared.
    fn refused_commands(confinement: &str, kernel: &[u8]) -> (String, bool, String, bool) {
        let dir = scratch(&format!("refused-{confinement}"));
        fs::write(dir.join("kernel"), kernel).unwrap();
        let live = Command::new(program())
            .arg("live")
            .arg(root().join("guest/digest.wasm"))
            .arg(framed_fixture(
                &root().join("evidence/fixtures/issues.tsv"),
                &dir,
            ))
            .arg(dir.join("record.json"))
            .arg("octo/real")
            .arg("1")
            .current_dir(&dir)
            .env_clear()
            .env("HYALOS_KERNEL", dir.join("kernel"))
            .env("HYALOS_TEST_NO_CONFINEMENT", confinement)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let run = Command::new(program())
            .arg("run")
            .arg(root().join("guest/digest.wasm"))
            .arg(framed_fixture(
                &root().join("evidence/fixtures/issues.tsv"),
                &dir,
            ))
            .arg(dir.join("transcript.json"))
            .current_dir(&dir)
            .env_clear()
            .env("HYALOS_KERNEL", dir.join("kernel"))
            .env("HYALOS_TEST_NO_CONFINEMENT", confinement)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let said = |o: &std::process::Output| String::from_utf8_lossy(&o.stderr).trim().to_string();
        let found = (
            said(&live),
            dir.join("ledger").exists() || dir.join("record.json").exists(),
            said(&run),
            dir.join("transcript.json").exists(),
        );
        let _ = fs::remove_dir_all(&dir);
        found
    }

    fn check_exit(pretend: &str) -> Option<i32> {
        Command::new(program())
            .args(["machine-runner", "--check", pretend])
            .env_clear()
            .current_dir("/")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .code()
    }

    /// S-04.3.e1: on a host without Landlock, a KVM run is refused before the guest runs. The
    /// holder's check refuses a `live` run naming Landlock, before any chain link; a runner that
    /// cannot confine itself sends only its refusal, so no guest instruction ran, and the run ends
    /// in `KernelFault`.
    #[test]
    fn core_s_04_3_e1_a_host_without_landlock_refuses_the_run_naming_landlock() {
        let Some(ended) = refused_run("core_s_04_3_e1", "--pretend-no-landlock") else {
            return;
        };
        assert_eq!(
            (ended.outcome, ended.frames, ended.asked),
            (Err(Reason::KernelFault), 1, 0),
            "one frame, the runner's refusal, and nothing asked"
        );
        assert_eq!(ended.status.code(), Some(runner::NO_LANDLOCK));
        assert_eq!(
            check_exit("--pretend-no-landlock"),
            Some(runner::NO_LANDLOCK)
        );
        assert_eq!(check_exit("--pretend-no-seccomp"), Some(runner::NO_SECCOMP));
        let Some(Backend::Kvm { kernel, .. }) = kvm_backend("core_s_04_3_e1") else {
            return;
        };
        let (live, chained, run, transcribed) = refused_commands("landlock", &kernel);
        assert_eq!(live, format!("refused: {}", runner::NO_LANDLOCK_SAID));
        assert!(live.contains("Landlock"));
        assert!(!chained, "a chain link or record was written");
        assert_eq!(run, "refused: KernelFault");
        assert!(!transcribed, "a transcript was written");
        // Without the injected failure, this host's runner confines itself and the same check passes.
        assert_eq!(runner::check(), Ok(()));
    }

    /// S-04.3.e2: a seccomp filter that fails to install refuses the run before the guest runs.
    /// The runner installs a program the kernel rejects, sends only its refusal and never runs the
    /// machine; the holder's check refuses a `live` run before its chain.
    #[test]
    fn core_s_04_3_e2_a_seccomp_filter_that_fails_to_install_refuses_before_the_guest_runs() {
        let Some(ended) = refused_run("core_s_04_3_e2", "--pretend-no-seccomp") else {
            return;
        };
        assert_eq!(
            (ended.outcome, ended.frames, ended.asked),
            (Err(Reason::KernelFault), 1, 0),
            "one frame, the runner's refusal, and nothing asked"
        );
        let Some(Backend::Kvm { kernel, .. }) = kvm_backend("core_s_04_3_e2") else {
            return;
        };
        let (live, chained, run, transcribed) = refused_commands("seccomp", &kernel);
        assert_eq!(live, format!("refused: {}", runner::NO_SECCOMP_SAID));
        assert!(!chained, "a chain link or record was written");
        assert_eq!(run, "refused: KernelFault");
        assert!(!transcribed, "a transcript was written");
    }

    /// S-04.3: the test-only arguments are taken only after the command's own, once each, and a
    /// `--check` runner takes no hostile act.
    #[test]
    fn core_s_04_3_the_runner_takes_its_test_arguments_strictly() {
        for args in [
            &["--check", "--hostile", "open"][..],
            &["--hostile", "nothing"],
            &["--hostile"],
            &["--pretend-no-landlock", "--pretend-no-seccomp"],
            &["--pretend-no-landlock", "--check"],
        ] {
            let status = Command::new(program())
                .arg("machine-runner")
                .args(args)
                .env_clear()
                .stdin(Stdio::null())
                .status()
                .unwrap();
            assert_eq!(status.code(), Some(runner::BROKEN), "{args:?}");
        }
    }
}
