//! `hyalos-vm-init`: PID 1 in a microVM sandbox (ADR-0058).
//!
//! The kernel starts it as `init=/.hyalos/init` on the job's root filesystem, mounted read-only. It:
//!
//! 1. mounts `/proc`, `/sys`, `/dev`, a cgroup2 hierarchy, and small private `/tmp` and `/run`, and
//!    remounts `/` read-only, `nosuid` and `nodev`;
//! 2. brings up loopback, the only interface the VM has;
//! 3. opens its output streams to the host (ports [`STDOUT`] and [`STDERR`]) and asks for the job
//!    (port [`CONTROL`]);
//! 4. drops the capability bounding set and sets no-new-privileges, for itself and so for every
//!    process after it;
//! 5. listens on `/run/broker.sock` and relays each connection to the host's vsock port [`BROKER`],
//!    which Firecracker connects to the broker's socket;
//! 6. runs the program as user 65534 in a cgroup with the job's memory and process limits, with
//!    those streams as its stdout and stderr;
//! 7. reaps every orphan, and when the program ends kills what is left, reports the exit code, waits
//!    for the host to say it has the output, and reboots, which ends the VM.
//!
//! The job's description is a 4-byte big-endian length and that many bytes of NUL-terminated
//! fields: the protocol version, memory bytes, process limit, the seconds after which the VM ends
//! whatever the host does (the host's own deadline is sooner), working directory, the number of
//! arguments, the arguments, then `NAME=value` for each variable. No arguments at all is the
//! readiness probe: set up, run nothing, exit 0.

use std::ffi::CString;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Command, Stdio};
use std::time::Duration;

/// The vsock ports, guest to host. They must match `core/substrate/src/microvm.rs`.
const CONTROL: u32 = 1000;
const STDOUT: u32 = 1001;
const STDERR: u32 = 1002;
const BROKER: u32 = 1024;
const VERSION: &str = "1";
/// The most a job's description may be.
const MAX_JOB: usize = 1 << 20;
/// The sandbox's user and group: `nobody`, as in a container sandbox.
const SANDBOX_ID: u32 = 65534;
const CGROUP: &str = "/sys/fs/cgroup/job";
const BROKER_SOCKET: &str = "/run/broker.sock";
const DEFAULT_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

struct Job {
    memory_bytes: u64,
    max_processes: u32,
    backstop_secs: u64,
    workdir: String,
    argv: Vec<String>,
    env: Vec<(String, String)>,
}

fn main() {
    let mut control = None;
    let result = run(&mut control);
    if let Some(c) = control.as_mut() {
        let line = match &result {
            Ok(code) => format!("exit {code}\n"),
            Err(e) => format!("error {}\n", e.to_string().replace('\n', " ")),
        };
        let _ = c.write_all(line.as_bytes());
        // The host answers once it has read the program's output to its end.
        let _ = c.set_read_timeout(Some(Duration::from_secs(10)));
        let mut done = [0u8; 5];
        let _ = c.read_exact(&mut done);
    } else if let Err(e) = &result {
        eprintln!("hyalos-vm-init: {e}");
    }
    // SAFETY: sync takes no arguments and cannot fail.
    unsafe { libc::sync() };
    // SAFETY: restarting is how a Firecracker guest ends its VM; nothing runs after it.
    unsafe { libc::reboot(libc::LINUX_REBOOT_CMD_RESTART) };
}

fn run(control: &mut Option<UnixStream>) -> io::Result<i32> {
    mounts()?;
    loopback_up()?;
    let mut c = vsock_connect(CONTROL)?;
    // The output streams are opened at once, whatever follows, so the host always has them to read
    // to their end; each is closed before the report, by the program's end or by returning.
    let stdout = vsock_connect(STDOUT)?;
    let stderr = vsock_connect(STDERR)?;
    let job = read_job(&mut c)?;
    *control = Some(c);
    drop_privileges_for_children()?;
    if job.argv.is_empty() {
        return Ok(0);
    }
    limits(&job)?;
    // If the engine is gone and nothing stops the VM, it ends itself.
    let backstop = Duration::from_secs(job.backstop_secs);
    std::thread::spawn(move || {
        std::thread::sleep(backstop);
        // SAFETY: as at the end of main; the VM ends here, without a report.
        unsafe { libc::reboot(libc::LINUX_REBOOT_CMD_RESTART) };
    });
    relay_broker()?;
    let code = run_program(&job, stdout, stderr);
    // Whatever the program left behind goes with it, so its output streams close.
    // SAFETY: -1 signals every process but this one; this process ignores nothing it needs.
    unsafe { libc::kill(-1, libc::SIGKILL) };
    reap_all();
    Ok(code)
}

fn check(ret: libc::c_int, what: &str) -> io::Result<()> {
    if ret < 0 {
        let e = io::Error::last_os_error();
        Err(io::Error::new(e.kind(), format!("{what}: {e}")))
    } else {
        Ok(())
    }
}

fn c(s: &str) -> io::Result<CString> {
    CString::new(s).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a NUL in a string"))
}

fn mount(
    source: &str,
    target: &str,
    fstype: &str,
    flags: libc::c_ulong,
    data: &str,
) -> io::Result<()> {
    let (s, t, f, d) = (c(source)?, c(target)?, c(fstype)?, c(data)?);
    // SAFETY: every pointer is a NUL-terminated string that outlives the call.
    let ret = unsafe { libc::mount(s.as_ptr(), t.as_ptr(), f.as_ptr(), flags, d.as_ptr().cast()) };
    check(ret, &format!("mount {target}"))
}

fn mounts() -> io::Result<()> {
    let safe = libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC;
    mount("proc", "/proc", "proc", safe, "")?;
    mount("sysfs", "/sys", "sysfs", safe, "")?;
    // A kernel built to mount devtmpfs itself has done so already.
    match mount(
        "devtmpfs",
        "/dev",
        "devtmpfs",
        libc::MS_NOSUID | libc::MS_NOEXEC,
        "mode=0755",
    ) {
        Err(e) if e.kind() == io::ErrorKind::ResourceBusy => {}
        other => other?,
    }
    mount("cgroup2", "/sys/fs/cgroup", "cgroup2", safe, "")?;
    mount("tmpfs", "/tmp", "tmpfs", safe, "mode=1777,size=64m")?;
    mount("tmpfs", "/run", "tmpfs", safe, "mode=0755,size=1m")?;
    mount(
        "",
        "/",
        "",
        libc::MS_REMOUNT | libc::MS_RDONLY | libc::MS_NOSUID | libc::MS_NODEV,
        "",
    )
}

fn loopback_up() -> io::Result<()> {
    // SAFETY: a plain socket call; the descriptor is owned below.
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) };
    check(fd, "socket for loopback")?;
    // SAFETY: fd is a socket this function just opened and nothing else owns.
    let sock = unsafe { OwnedFd::from_raw_fd(fd) };
    // SAFETY: ifreq is plain data; zero is a valid value for all of it.
    let mut req: libc::ifreq = unsafe { std::mem::zeroed() };
    for (dst, src) in req.ifr_name.iter_mut().zip(b"lo") {
        *dst = *src as libc::c_char;
    }
    req.ifr_ifru.ifru_flags =
        (libc::IFF_UP | libc::IFF_LOOPBACK | libc::IFF_RUNNING) as libc::c_short;
    // SAFETY: SIOCSIFFLAGS reads an ifreq, which req is, for the life of the call.
    let ret = unsafe { libc::ioctl(sock.as_raw_fd(), libc::SIOCSIFFLAGS as _, &req) };
    check(ret, "loopback up")
}

/// A vsock connection to the host. It is held as a `UnixStream` only for the socket calls that type
/// makes, which are the same on any stream socket: read, write, shutdown and timeouts.
fn vsock_connect(port: u32) -> io::Result<UnixStream> {
    // SAFETY: a plain socket call; the descriptor is owned below.
    let fd = unsafe { libc::socket(libc::AF_VSOCK, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
    check(fd, "vsock socket")?;
    // SAFETY: fd is a socket this function just opened and nothing else owns.
    let sock = unsafe { OwnedFd::from_raw_fd(fd) };
    let addr = libc::sockaddr_vm {
        svm_family: libc::AF_VSOCK as libc::sa_family_t,
        svm_reserved1: 0,
        svm_port: port,
        svm_cid: libc::VMADDR_CID_HOST,
        svm_zero: [0; 4],
    };
    // SAFETY: addr is a sockaddr_vm and the length given is its size.
    let ret = unsafe {
        libc::connect(
            sock.as_raw_fd(),
            (&addr as *const libc::sockaddr_vm).cast(),
            std::mem::size_of::<libc::sockaddr_vm>() as libc::socklen_t,
        )
    };
    check(ret, &format!("vsock port {port}"))?;
    Ok(UnixStream::from(sock))
}

fn read_job(control: &mut UnixStream) -> io::Result<Job> {
    let bad = |why: &str| io::Error::new(io::ErrorKind::InvalidData, format!("job: {why}"));
    let mut len = [0u8; 4];
    control.read_exact(&mut len)?;
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_JOB {
        return Err(bad("too long"));
    }
    let mut body = vec![0u8; len];
    control.read_exact(&mut body)?;
    let text = String::from_utf8(body).map_err(|_| bad("not UTF-8"))?;
    let mut fields = text.split_terminator('\0');
    let mut next = || fields.next().ok_or_else(|| bad("too few fields"));
    if next()? != VERSION {
        return Err(bad("another protocol version"));
    }
    let memory_bytes = next()?.parse().map_err(|_| bad("memory"))?;
    let max_processes = next()?.parse().map_err(|_| bad("processes"))?;
    let backstop_secs = next()?.parse().map_err(|_| bad("backstop"))?;
    let workdir = next()?.to_string();
    let argc: usize = next()?.parse().map_err(|_| bad("argument count"))?;
    let mut argv = Vec::with_capacity(argc.min(4096));
    for _ in 0..argc {
        argv.push(next()?.to_string());
    }
    let env = fields
        .map(|kv| {
            kv.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .ok_or_else(|| bad("a variable without '='"))
        })
        .collect::<io::Result<_>>()?;
    Ok(Job {
        memory_bytes,
        max_processes,
        backstop_secs,
        workdir,
        argv,
        env,
    })
}

/// Empties the capability bounding set and sets no-new-privileges on this process. Both are
/// inherited, so no process the job starts can gain a capability, by setuid or file capabilities.
/// This process keeps the capabilities it has, which it needs to reboot and to kill.
fn drop_privileges_for_children() -> io::Result<()> {
    for cap in 0..=63 {
        // SAFETY: PR_CAPBSET_DROP takes a capability number; EINVAL past the last one is expected.
        let ret = unsafe { libc::prctl(libc::PR_CAPBSET_DROP, cap, 0, 0, 0) };
        if ret < 0 && io::Error::last_os_error().raw_os_error() != Some(libc::EINVAL) {
            return check(ret, "drop the bounding set");
        }
    }
    // SAFETY: PR_SET_NO_NEW_PRIVS takes 1 and three zeros.
    let ret = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
    check(ret, "no new privileges")
}

fn limits(job: &Job) -> io::Result<()> {
    std::fs::write("/sys/fs/cgroup/cgroup.subtree_control", "+memory +pids")?;
    std::fs::create_dir(CGROUP)?;
    std::fs::write(format!("{CGROUP}/memory.max"), job.memory_bytes.to_string())?;
    std::fs::write(format!("{CGROUP}/memory.swap.max"), "0")?;
    std::fs::write(format!("{CGROUP}/pids.max"), job.max_processes.to_string())
}

fn relay_broker() -> io::Result<()> {
    let listener = UnixListener::bind(BROKER_SOCKET)?;
    std::fs::set_permissions(BROKER_SOCKET, std::fs::Permissions::from_mode(0o666))?;
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            std::thread::spawn(move || {
                if let Ok(host) = vsock_connect(BROKER) {
                    relay(conn, host);
                }
            });
        }
    });
    Ok(())
}

/// Copies both ways until both sides are done, passing each end-of-stream on.
fn relay(guest: UnixStream, host: UnixStream) {
    let (Ok(guest_r), Ok(host_r)) = (guest.try_clone(), host.try_clone()) else {
        return;
    };
    let up = std::thread::spawn(move || {
        let mut from = guest_r;
        let mut to = host;
        let _ = io::copy(&mut from, &mut to);
        let _ = to.shutdown(std::net::Shutdown::Write);
    });
    let mut from = host_r;
    let mut to = guest;
    let _ = io::copy(&mut from, &mut to);
    let _ = to.shutdown(std::net::Shutdown::Write);
    let _ = up.join();
}

/// Runs the program in the job's cgroup as the sandbox user and returns its exit code, or 128 plus
/// the signal that ended it.
fn run_program(job: &Job, stdout: UnixStream, stderr: UnixStream) -> i32 {
    let (program, args) = match job.argv.split_first() {
        Some(split) => split,
        None => return 0,
    };
    let mut env: Vec<(String, String)> = job.env.clone();
    for (name, default) in [("PATH", DEFAULT_PATH), ("HOME", "/")] {
        if !env.iter().any(|(k, _)| k == name) {
            env.push((name.into(), default.into()));
        }
    }
    let workdir = if std::path::Path::new(&job.workdir).is_dir() {
        job.workdir.as_str()
    } else {
        "/"
    };
    let mut stderr_note = stderr.try_clone().ok();
    // The child is created in the job's cgroup by moving this process there for the moment it
    // forks, so nothing the program does happens outside the limits.
    if let Err(e) = std::fs::write(format!("{CGROUP}/cgroup.procs"), "0") {
        note(
            &mut stderr_note,
            &format!("cannot enter the job's cgroup: {e}"),
        );
        return 125;
    }
    let spawned = Command::new(program)
        .args(args)
        .env_clear()
        .envs(env)
        .current_dir(workdir)
        .uid(SANDBOX_ID)
        .gid(SANDBOX_ID)
        .stdin(Stdio::null())
        .stdout(Stdio::from(OwnedFd::from(stdout)))
        .stderr(Stdio::from(OwnedFd::from(stderr)))
        .spawn();
    let _ = std::fs::write("/sys/fs/cgroup/cgroup.procs", "0");
    let child = match spawned {
        Ok(child) => child,
        Err(e) => {
            note(&mut stderr_note, &format!("{program}: {e}"));
            return if e.kind() == io::ErrorKind::NotFound {
                127
            } else {
                126
            };
        }
    };
    drop(stderr_note);
    let main = child.id() as libc::pid_t;
    // As PID 1 this reaps every orphan, and returns when the program itself ends.
    loop {
        let mut status = 0;
        // SAFETY: waitpid with a valid pointer to an int.
        let pid = unsafe { libc::waitpid(-1, &mut status, 0) };
        if pid == main {
            let status = std::process::ExitStatus::from_raw(status);
            return status
                .code()
                .unwrap_or_else(|| 128 + status.signal().unwrap_or(0));
        }
        if pid < 0 && io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            return 125;
        }
    }
}

fn note(stream: &mut Option<UnixStream>, message: &str) {
    if let Some(s) = stream {
        let _ = writeln!(s, "hyalos-vm-init: {message}");
    }
}

fn reap_all() {
    loop {
        let mut status = 0;
        // SAFETY: waitpid with a valid pointer to an int.
        let pid = unsafe { libc::waitpid(-1, &mut status, 0) };
        if pid < 0 && io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            return;
        }
    }
}
