//! The binary keeps itself out of crash dumps (S1.12, ADR-0030), checked on a real process.
#![cfg(target_os = "linux")]

use std::error::Error;
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

type Res<T = ()> = Result<T, Box<dyn Error>>;

fn dir(name: &str) -> Res<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("hardening-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Kills a child it still holds when the test ends, however it ends.
struct Guard(Child);

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn core_files(dir: &Path) -> Res<Vec<String>> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if name.starts_with("core") {
            found.push(name);
        }
    }
    Ok(found)
}

fn signal(pid: u32, name: &str) -> Res<()> {
    let sent = Command::new("kill")
        .args([&format!("-{name}"), &pid.to_string()])
        .status()?;
    assert!(sent.success(), "kill -{name} {pid}");
    Ok(())
}

/// The core file size limit of a process, as `/proc/<pid>/limits` shows it: (soft, hard).
fn core_limit(pid: u32) -> Res<(String, String)> {
    let limits = std::fs::read_to_string(format!("/proc/{pid}/limits"))?;
    let line = limits
        .lines()
        .find(|l| l.starts_with("Max core file size"))
        .ok_or("no core limit line")?;
    let mut words = line.split_whitespace().skip(4);
    let soft = words.next().ok_or("no soft limit")?.to_string();
    let hard = words.next().ok_or("no hard limit")?.to_string();
    Ok((soft, hard))
}

/// Runs `hyalos` with the core limit raised to unlimited, as an operator's shell might leave it, on a
/// command that waits: it opens a named pipe nobody writes to, so the process stays alive, idle, after
/// it has started up. Returns once the process has finished starting.
fn running_hyalos(work: &Path) -> Res<Guard> {
    let fifo = work.join("wait");
    assert!(Command::new("mkfifo").arg(&fifo).status()?.success());
    let child = Command::new("sh")
        .current_dir(work)
        .arg("-c")
        .arg("ulimit -c unlimited && exec \"$0\" \"$@\"")
        .arg(env!("CARGO_BIN_EXE_hyalos"))
        .args(["log", "verify", "--db"])
        .arg(&fifo)
        .args(["--public-key", &"00".repeat(32)])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(Guard(child))
}

/// Waits until the process shows the limit it will keep, or fails with what it showed.
fn wait_for_core_limit_zero(pid: u32) -> Res<(String, String)> {
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        let now = core_limit(pid)?;
        if now == ("0".to_string(), "0".to_string()) || Instant::now() > until {
            return Ok(now);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_binary_turns_dumps_off_for_itself_whatever_limit_it_was_started_under() -> Res {
    let work = dir("limits")?;
    let guard = running_hyalos(&work)?;
    let pid = guard.0.id();
    // It started with an unlimited core limit and has lowered it, soft and hard, so nothing in it can
    // raise it again.
    assert_eq!(
        wait_for_core_limit_zero(pid)?,
        ("0".to_string(), "0".to_string())
    );
    // And it is marked not dumpable: the kernel then makes the files of /proc/<pid> root's, which a
    // dumpable process of ours would own. (Not checkable as root, who owns them either way.)
    if rustix::process::geteuid().is_root() {
        eprintln!("not checked: running as root");
    } else {
        let owner = std::fs::metadata(format!("/proc/{pid}/environ"))?.uid();
        assert_eq!(
            owner, 0,
            "a dumpable process's /proc files belong to its user"
        );
    }
    Ok(())
}

/// A process started the same way but not hardened: the control that says whether this machine writes
/// a core file into the working directory at all. Where `core_pattern` pipes to a crash handler, it
/// does not, and finding no file proves nothing.
fn control_writes_a_core_file(work: &Path) -> Res<bool> {
    Command::new("sh")
        .current_dir(work)
        .arg("-c")
        // Not waited for: where a crash handler is piped to, finishing its dump can take minutes.
        .arg("ulimit -c unlimited && { sleep 30 & p=$!; sleep 0.2; kill -ABRT $p; }")
        // Nothing inherited: the crashed process outlives the shell, and holding the test's output open
        // would make whoever reads it wait for the dump to finish.
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    std::thread::sleep(Duration::from_secs(2));
    Ok(!core_files(work)?.is_empty())
}

#[test]
fn a_crash_leaves_no_core_file_where_an_unhardened_process_would_leave_one() -> Res {
    let control = dir("control")?;
    let can_dump = control_writes_a_core_file(&control)?;

    let work = dir("crash")?;
    let mut guard = running_hyalos(&work)?;
    let pid = guard.0.id();
    assert_eq!(
        wait_for_core_limit_zero(pid)?,
        ("0".to_string(), "0".to_string()),
        "it must have started up before it is crashed"
    );
    // Abort, as a crash in the process would. A SIGSEGV sent from outside is swallowed by the Rust
    // runtime's stack overflow handler, which only acts on a real fault, and the process lives on.
    signal(pid, "ABRT")?;
    let status = guard.0.wait()?;
    assert_eq!(
        status.signal(),
        Some(6),
        "it was killed by the crash: {status:?}"
    );
    // The kernel says whether it dumped core, to a file or to a crash handler alike.
    assert!(!status.core_dumped(), "the kernel dumped core: {status:?}");
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(core_files(&work)?, Vec::<String>::new());

    if can_dump {
        eprintln!("control: an unhardened process dumps core here, so the absence above is real");
    } else {
        eprintln!(
            "control: no core file appears here even for an unhardened process (core_pattern is {:?}), \
             so the absence of a file proves nothing on this machine; the kernel's own report that it dumped no core, and the limit and dumpable checks, do",
            std::fs::read_to_string("/proc/sys/kernel/core_pattern")?.trim()
        );
    }
    Ok(())
}
