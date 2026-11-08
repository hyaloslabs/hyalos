//! The microVM driver: each sandbox is a Firecracker virtual machine with its own kernel (ADR-0058).
//!
//! The job's image comes from a container runtime, as for the other drivers, and is turned once into
//! a read-only ext4 disk with the init (`crates/vm-init`) at `/.hyalos/init`, cached by the image's id
//! and the init. The VM has no network device at all, only vsock: the guest connects to the host,
//! and Firecracker turns a connection to port P into one to the Unix socket `v.sock_P` beside the
//! VM. The host listens on the control, stdout and stderr ports; port [`BROKER`] is a link to the
//! broker's socket, so the init's `/run/broker.sock` reaches the broker and nothing else.
//!
//! Firecracker runs as the operator, with its own seccomp filters and without the jailer, which
//! needs root. A spec is validated before anything is run, and commands are run directly, never
//! through a shell.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use hyalos_types::{Attestation, SubstrateName};

use crate::spec::OUTPUT_LIMIT;
use crate::{
    Ending, Isolation, Outcome, Sandbox, SandboxSpec, StartError, StopHandle, Substrate,
    Unavailable,
};

/// The vsock ports, guest to host. They must match `crates/vm-init`.
const CONTROL: u32 = 1000;
const STDOUT: u32 = 1001;
const STDERR: u32 = 1002;
const BROKER: u32 = 1024;
const PROTOCOL: &str = "1";

/// Where the init is on the job's disk, and what the kernel is told to run.
const INIT_IN_GUEST: &str = ".hyalos/init";
const BOOT_ARGS: &str = "console=ttyS0 quiet loglevel=1 reboot=k panic=1 pci=off nomodule ro root=/dev/vda rootfstype=ext4 init=/.hyalos/init";
/// Memory the guest's kernel and init need beyond what the job may use.
const GUEST_OVERHEAD_MIB: u64 = 128;
const VCPUS: u32 = 2;

const POLL: Duration = Duration::from_millis(10);
/// How long after the host's deadline the guest ends the VM itself, should the engine be gone.
const BACKSTOP: Duration = Duration::from_secs(30);
/// How long the readiness probe may take to boot and report.
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// Where the driver finds what it runs, from the operator's `microvm.conf` (ADR-0058).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicroVmConfig {
    /// The `firecracker` binary.
    pub firecracker: PathBuf,
    /// The guest kernel, an uncompressed `vmlinux` with vsock, virtio block, ext4 and cgroup v2.
    pub kernel: PathBuf,
    /// The static `hyalos-vm-init` binary.
    pub init: PathBuf,
    /// Where converted images are kept.
    pub cache_dir: PathBuf,
    /// Where each VM's sockets and configuration are, while it runs. Unix socket paths are short,
    /// so this must be too.
    pub run_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct MicroVmSubstrate {
    config: MicroVmConfig,
    /// The container runtime the job's image is read from.
    runtime: String,
}

impl MicroVmSubstrate {
    pub fn new(config: MicroVmConfig, runtime: &str) -> Self {
        Self {
            config,
            runtime: runtime.to_string(),
        }
    }

    /// A driver for each container runtime the engine looks for, in order of preference.
    pub fn candidates(config: &MicroVmConfig) -> Vec<Self> {
        ["podman", "docker"]
            .into_iter()
            .map(|r| Self::new(config.clone(), r))
            .collect()
    }

    pub fn runtime(&self) -> &str {
        &self.runtime
    }

    fn run(&self, program: &Path, args: &[&str]) -> Result<std::process::Output, Unavailable> {
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| Unavailable(format!("cannot run {}: {e}", program.display())))
    }

    fn runtime_out(&self, args: &[&str]) -> Result<std::process::Output, Unavailable> {
        self.run(Path::new(&self.runtime), args)
    }

    /// The image's id, or why it cannot be used.
    fn image_id(&self, image: &str) -> Result<String, String> {
        let out = self
            .runtime_out(&["image", "inspect", "--format", "{{.Id}}", image])
            .map_err(|e| e.0)?;
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !out.status.success() || id.is_empty() {
            return Err(format!(
                "{} has no image {image}; build or pull it first (hyalos setup says how)",
                self.runtime
            ));
        }
        Ok(id)
    }

    /// The image's own environment and working directory, which a container runtime would apply.
    fn image_settings(&self, image: &str) -> Result<(Vec<(String, String)>, String), String> {
        let out = self
            .runtime_out(&[
                "image",
                "inspect",
                "--format",
                "{{range .Config.Env}}{{.}}{{\"\\x00\"}}{{end}}{{\"\\x01\"}}{{.Config.WorkingDir}}",
                image,
            ])
            .map_err(|e| e.0)?;
        if !out.status.success() {
            return Err(first_line(&out.stderr));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let (env, workdir) = text.split_once('\u{1}').unwrap_or((&text, ""));
        let env = env
            .split('\0')
            .filter_map(|kv| kv.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let workdir = workdir.trim();
        Ok((
            env,
            if workdir.is_empty() { "/" } else { workdir }.to_string(),
        ))
    }

    /// A key for what the init binary is, so a new init makes new disks.
    fn init_key(&self) -> Result<String, String> {
        let bytes = std::fs::read(&self.config.init)
            .map_err(|e| format!("cannot read the init {}: {e}", self.config.init.display()))?;
        let mut h = DefaultHasher::new();
        bytes.hash(&mut h);
        Ok(format!("{:016x}", h.finish()))
    }

    /// The job's disk for `image`, made once and kept.
    fn disk_for(&self, image: &str) -> Result<PathBuf, String> {
        let id = self.image_id(image)?;
        let id = id.trim_start_matches("sha256:");
        let name = format!("{}-{}.ext4", &id[..id.len().min(32)], self.init_key()?);
        let disk = self.config.cache_dir.join(name);
        if disk.is_file() {
            return Ok(disk);
        }
        let build = self.config.cache_dir.join(format!(".build-{}", unique()));
        let made = self.make_disk(image, &build, &disk);
        let _ = make_removable(&build);
        let _ = std::fs::remove_dir_all(&build);
        made.map(|()| disk)
    }

    /// Exports the image's filesystem, adds the init and `/etc/hosts`, and writes it as ext4.
    fn make_disk(&self, image: &str, build: &Path, disk: &Path) -> Result<(), String> {
        let root = build.join("root");
        std::fs::create_dir_all(&root).map_err(|e| format!("{}: {e}", root.display()))?;
        let created = self
            .runtime_out(&[
                "create",
                "--pull",
                "never",
                "--network",
                "none",
                "--entrypoint",
                "true",
                image,
            ])
            .map_err(|e| e.0)?;
        let container = String::from_utf8_lossy(&created.stdout).trim().to_string();
        if !created.status.success() || container.is_empty() {
            return Err(first_line(&created.stderr));
        }
        let exported = self.export(&container, &root);
        let _ = self.runtime_out(&["rm", "--force", &container]);
        exported?;
        // The program's mount points, and what the init and the sandbox expect to find.
        for dir in ["proc", "sys", "dev", "tmp", "run", "etc", ".hyalos"] {
            std::fs::create_dir_all(root.join(dir)).map_err(|e| format!("{dir}: {e}"))?;
        }
        let init = root.join(INIT_IN_GUEST);
        std::fs::copy(&self.config.init, &init).map_err(|e| format!("copy the init: {e}"))?;
        std::fs::set_permissions(&init, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("the init: {e}"))?;
        // A container runtime writes this at run time; a VM has only its disk.
        let _ = std::fs::remove_file(root.join("etc/hosts"));
        std::fs::write(
            root.join("etc/hosts"),
            "127.0.0.1\tlocalhost\n::1\tlocalhost\n",
        )
        .map_err(|e| format!("/etc/hosts: {e}"))?;
        // Everything readable by the operator, so mke2fs can copy it.
        make_removable(&root)?;
        let bytes = tree_size(&root);
        let mib = bytes / (1024 * 1024) * 13 / 10 + 64;
        let partial = build.join("disk.ext4");
        let mke2fs = find_mke2fs().ok_or("mke2fs is not installed (e2fsprogs)")?;
        let root_arg = root.to_string_lossy().into_owned();
        let partial_arg = partial.to_string_lossy().into_owned();
        let size = format!("{mib}M");
        let out = self
            .run(
                &mke2fs,
                &[
                    "-q",
                    "-F",
                    "-t",
                    "ext4",
                    "-L",
                    "hyalos",
                    "-E",
                    "root_owner=0:0",
                    "-d",
                    &root_arg,
                    &partial_arg,
                    &size,
                ],
            )
            .map_err(|e| e.0)?;
        if !out.status.success() {
            return Err(format!("mke2fs: {}", first_line(&out.stderr)));
        }
        std::fs::rename(&partial, disk).map_err(|e| format!("{}: {e}", disk.display()))
    }

    /// Streams the container's filesystem into `root`, without the device nodes.
    fn export(&self, container: &str, root: &Path) -> Result<(), String> {
        let mut export = Command::new(&self.runtime)
            .args(["export", container])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("export: {e}"))?;
        let stream = export.stdout.take().ok_or("export: no output")?;
        let tar = Command::new("tar")
            .arg("-x")
            .arg("-C")
            .arg(root)
            .args(["--no-same-owner", "--exclude=dev/*", "--exclude=./dev/*"])
            .stdin(Stdio::from(stream))
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("tar: {e}"))?;
        let exported = export.wait().map_err(|e| format!("export: {e}"))?;
        if !exported.success() {
            return Err(format!("{} could not export the image", self.runtime));
        }
        if !tar.status.success() {
            return Err(format!("tar: {}", first_line(&tar.stderr)));
        }
        Ok(())
    }

    /// Boots a VM and runs `argv` in it; an empty `argv` is the readiness probe.
    fn boot(
        &self,
        disk: &Path,
        job: &JobText,
        broker_socket: &Path,
        deadline: Duration,
        memory_bytes: u64,
    ) -> Result<MicroVmSandbox, StartError> {
        let id = unique();
        let dir = self.config.run_dir.join(&id);
        let refused = |what: &str, e: std::io::Error| StartError::Refused(format!("{what}: {e}"));
        std::fs::create_dir_all(&dir).map_err(|e| refused("the VM's directory", e))?;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| refused("the VM's directory", e))?;
        let fail = |e: StartError| {
            let _ = std::fs::remove_dir_all(&dir);
            e
        };
        let uds = dir.join("v.sock");
        let listen = |port: u32| {
            let path = dir.join(format!("v.sock_{port}"));
            let l = UnixListener::bind(&path).map_err(|e| refused("a vsock listener", e))?;
            l.set_nonblocking(true)
                .map_err(|e| refused("a vsock listener", e))?;
            Ok::<_, StartError>(l)
        };
        let control = listen(CONTROL).map_err(fail)?;
        let stdout = listen(STDOUT).map_err(fail)?;
        let stderr = listen(STDERR).map_err(fail)?;
        std::os::unix::fs::symlink(broker_socket, dir.join(format!("v.sock_{BROKER}")))
            .map_err(|e| fail(refused("the broker's link", e)))?;
        let mem_mib = memory_bytes.div_ceil(1024 * 1024) + GUEST_OVERHEAD_MIB;
        let config = serde_json::json!({
            "boot-source": {
                "kernel_image_path": self.config.kernel,
                "boot_args": BOOT_ARGS,
            },
            "drives": [{
                "drive_id": "rootfs",
                "path_on_host": disk,
                "is_root_device": true,
                "is_read_only": true,
            }],
            "machine-config": { "vcpu_count": VCPUS, "mem_size_mib": mem_mib },
            "vsock": { "guest_cid": 3, "uds_path": uds },
        });
        let config_path = dir.join("vm.json");
        std::fs::write(&config_path, config.to_string())
            .map_err(|e| fail(refused("the VM's configuration", e)))?;
        let child = Command::new(&self.config.firecracker)
            .arg("--no-api")
            .arg("--config-file")
            .arg(&config_path)
            .args(["--id", &format!("hyalos-{id}"), "--level", "Error"])
            // Its own process group, so the operator's Ctrl-C reaches the engine and not the VM;
            // the engine stops it itself (ADR-0053).
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| fail(refused("cannot start firecracker", e)))?;
        let _ = std::fs::write(dir.join("pid"), child.id().to_string());
        let done = Arc::new(AtomicBool::new(false));
        let out = accept_and_drain(stdout, done.clone());
        let err = accept_and_drain(stderr, done.clone());
        let message = job.encode();
        let host = {
            let done = done.clone();
            std::thread::spawn(move || serve_control(control, &message, out, err, &done))
        };
        Ok(MicroVmSandbox {
            id,
            dir,
            child,
            done,
            host: Some(host),
            deadline_at: Instant::now() + deadline,
            stopped: false,
            stop_asked: StopHandle::default(),
            outcome: None,
        })
    }

    /// Boots the probe: a disk with only the init, which sets up and reports exit 0.
    fn probe(&self) -> Result<(), Unavailable> {
        let disk = self.probe_disk().map_err(Unavailable)?;
        let socket_dir = self.config.run_dir.join(format!(".probe-{}", unique()));
        std::fs::create_dir_all(&socket_dir).map_err(|e| Unavailable(e.to_string()))?;
        let socket = socket_dir.join("none.sock");
        let listener = UnixListener::bind(&socket);
        let job = JobText {
            memory_bytes: 64 * 1024 * 1024,
            max_processes: 16,
            backstop: PROBE_TIMEOUT + BACKSTOP,
            workdir: "/".into(),
            argv: Vec::new(),
            env: Vec::new(),
        };
        let result = self
            .boot(&disk, &job, &socket, PROBE_TIMEOUT, 64 * 1024 * 1024)
            .map_err(|e| Unavailable(e.to_string()))
            .map(|mut vm| vm.wait());
        drop(listener);
        let _ = std::fs::remove_dir_all(&socket_dir);
        match result? {
            Outcome {
                ending: Ending::Exited(0),
                ..
            } => Ok(()),
            other => Err(Unavailable(format!(
                "a microVM did not boot and report: {:?} {}",
                other.ending,
                String::from_utf8_lossy(&other.stderr).trim()
            ))),
        }
    }

    fn probe_disk(&self) -> Result<PathBuf, String> {
        let disk = self
            .config
            .cache_dir
            .join(format!("probe-{}.ext4", self.init_key()?));
        if disk.is_file() {
            return Ok(disk);
        }
        let build = self.config.cache_dir.join(format!(".build-{}", unique()));
        let root = build.join("root");
        let made = (|| {
            for dir in ["proc", "sys", "dev", "tmp", "run", ".hyalos"] {
                std::fs::create_dir_all(root.join(dir)).map_err(|e| e.to_string())?;
            }
            std::fs::copy(&self.config.init, root.join(INIT_IN_GUEST))
                .map_err(|e| format!("copy the init: {e}"))?;
            let mke2fs = find_mke2fs().ok_or("mke2fs is not installed (e2fsprogs)")?;
            let partial = build.join("disk.ext4");
            let (r, p) = (
                root.to_string_lossy().into_owned(),
                partial.to_string_lossy().into_owned(),
            );
            let out = self
                .run(
                    &mke2fs,
                    &[
                        "-q",
                        "-F",
                        "-t",
                        "ext4",
                        "-E",
                        "root_owner=0:0",
                        "-d",
                        &r,
                        &p,
                        "16M",
                    ],
                )
                .map_err(|e| e.0)?;
            if !out.status.success() {
                return Err(format!("mke2fs: {}", first_line(&out.stderr)));
            }
            std::fs::rename(&partial, &disk).map_err(|e| e.to_string())
        })();
        let _ = std::fs::remove_dir_all(&build);
        made.map(|()| disk)
    }
}

/// The job as the init reads it (`crates/vm-init`).
struct JobText {
    memory_bytes: u64,
    max_processes: u32,
    backstop: Duration,
    workdir: String,
    argv: Vec<String>,
    env: Vec<(String, String)>,
}

impl JobText {
    fn encode(&self) -> Vec<u8> {
        let mut fields = vec![
            PROTOCOL.to_string(),
            self.memory_bytes.to_string(),
            self.max_processes.to_string(),
            self.backstop.as_secs().to_string(),
            self.workdir.clone(),
            self.argv.len().to_string(),
        ];
        fields.extend(self.argv.iter().cloned());
        fields.extend(self.env.iter().map(|(k, v)| format!("{k}={v}")));
        let mut body = Vec::new();
        for f in fields {
            body.extend_from_slice(f.as_bytes());
            body.push(0);
        }
        let mut message = (body.len() as u32).to_be_bytes().to_vec();
        message.extend(body);
        message
    }
}

/// What the guest said about how the program ended.
enum Report {
    Exited(i32),
    /// The init could not run the job, and why.
    Failed(String),
    /// It said nothing: it never booted, or it was killed.
    Silent,
}

struct HostSide {
    report: Report,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// Waits for a guest connection on `listener` until `done`.
fn accept(listener: &UnixListener, done: &AtomicBool) -> Option<UnixStream> {
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                return Some(stream);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if done.load(Ordering::SeqCst) {
                    return None;
                }
                std::thread::sleep(POLL);
            }
            Err(_) => return None,
        }
    }
}

/// Reads one output stream from the guest to its end, keeping the first [`OUTPUT_LIMIT`] bytes.
fn accept_and_drain(listener: UnixListener, done: Arc<AtomicBool>) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let Some(mut stream) = accept(&listener, &done) else {
            return Vec::new();
        };
        let mut kept = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let room = OUTPUT_LIMIT.saturating_sub(kept.len());
                    kept.extend_from_slice(&buffer[..n.min(room)]);
                }
            }
        }
        kept
    })
}

/// Gives the guest its job, takes its report, and once the output is read tells it to end.
fn serve_control(
    listener: UnixListener,
    message: &[u8],
    stdout: JoinHandle<Vec<u8>>,
    stderr: JoinHandle<Vec<u8>>,
    done: &AtomicBool,
) -> HostSide {
    let mut report = Report::Silent;
    let mut stream = accept(&listener, done);
    if let Some(s) = stream.as_mut()
        && s.write_all(message).is_ok()
    {
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        while line.len() < 4096 {
            match s.read(&mut byte) {
                Ok(1) if byte[0] == b'\n' => break,
                Ok(1) => line.push(byte[0]),
                _ => break,
            }
        }
        let line = String::from_utf8_lossy(&line).into_owned();
        report = match line.split_once(' ') {
            Some(("exit", code)) => code.parse().map_or(Report::Silent, Report::Exited),
            Some(("error", why)) => Report::Failed(why.to_string()),
            _ => Report::Silent,
        };
    }
    // The guest has stopped everything that held the streams, so they end.
    let stdout = stdout.join().unwrap_or_default();
    let mut stderr = stderr.join().unwrap_or_default();
    if let Report::Failed(why) = &report {
        stderr.extend_from_slice(format!("hyalos-vm-init: {why}\n").as_bytes());
    }
    if let Some(mut s) = stream {
        let _ = s.write_all(b"done\n");
    }
    HostSide {
        report,
        stdout,
        stderr,
    }
}

fn unique() -> String {
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!(
        "{:x}{:x}{:x}",
        std::process::id(),
        nanos,
        COUNT.fetch_add(1, Ordering::SeqCst)
    )
}

fn first_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("no message")
        .to_string()
}

fn find_mke2fs() -> Option<PathBuf> {
    ["/usr/sbin/mke2fs", "/sbin/mke2fs", "/usr/bin/mke2fs"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
}

/// Gives the operator read and write on everything under `root`, and search on directories.
fn make_removable(root: &Path) -> Result<(), String> {
    let out = Command::new("chmod")
        .args(["-R", "u+rwX"])
        .arg(root)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("chmod: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("chmod: {}", first_line(&out.stderr)))
    }
}

/// The bytes the files under `root` hold, without following links.
fn tree_size(root: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.path().symlink_metadata() else {
                continue;
            };
            if meta.is_dir() {
                stack.push(entry.path());
            }
            total += meta.len().max(4096);
        }
    }
    total
}

impl Substrate for MicroVmSubstrate {
    fn name(&self) -> &'static str {
        "microvm"
    }

    fn recorded_as(&self) -> SubstrateName {
        SubstrateName::new("microvm.firecracker").unwrap_or_else(SubstrateName::unrecorded)
    }

    fn isolation(&self) -> Isolation {
        Isolation::MicroVm
    }

    /// None: Firecracker has no confidential mode, so nothing beneath the host signs the guest.
    fn attestation(&self) -> Attestation {
        Attestation::no_hardware_root()
    }

    fn available(&self) -> Result<(), Unavailable> {
        let c = &self.config;
        for (what, path) in [("kernel", &c.kernel), ("init", &c.init)] {
            if !path.is_file() {
                return Err(Unavailable(format!(
                    "the {what} {} is not there",
                    path.display()
                )));
            }
        }
        let out = self.run(&c.firecracker, &["--version"])?;
        if !out.status.success() {
            return Err(Unavailable(format!(
                "{} did not answer",
                c.firecracker.display()
            )));
        }
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/kvm")
            .map_err(|e| {
                Unavailable(format!(
                    "cannot open /dev/kvm ({e}); add yourself to the kvm group, or the host has no KVM"
                ))
            })?;
        for dir in [&c.cache_dir, &c.run_dir] {
            std::fs::create_dir_all(dir)
                .map_err(|e| Unavailable(format!("{}: {e}", dir.display())))?;
        }
        let out = self.runtime_out(&["version"])?;
        if !out.status.success() {
            return Err(Unavailable(format!(
                "{} did not answer: {}",
                self.runtime,
                first_line(&out.stderr)
            )));
        }
        Ok(())
    }

    fn ready(&self, image: &str) -> Result<(), Unavailable> {
        if !crate::spec::image_ok(image) {
            return Err(Unavailable(format!(
                "{image:?} is not a plain image reference"
            )));
        }
        self.available()?;
        self.image_id(image).map_err(Unavailable)?;
        self.probe()
    }

    fn start(&self, spec: &SandboxSpec) -> Result<Box<dyn Sandbox>, StartError> {
        spec.validate().map_err(StartError::Spec)?;
        self.available().map_err(StartError::Unavailable)?;
        let disk = self.disk_for(&spec.image).map_err(StartError::Refused)?;
        let (mut env, workdir) = self
            .image_settings(&spec.image)
            .map_err(StartError::Refused)?;
        // The spec's environment wins over the image's, as `--env` does for a container.
        env.retain(|(k, _)| !spec.env.contains_key(k));
        env.extend(spec.env.iter().map(|(k, v)| (k.clone(), v.clone())));
        let job = JobText {
            memory_bytes: spec.memory_bytes,
            max_processes: spec.max_processes,
            backstop: spec.deadline + BACKSTOP,
            workdir,
            argv: spec.command.clone(),
            env,
        };
        let vm = self.boot(
            &disk,
            &job,
            &spec.broker_socket,
            spec.deadline,
            spec.memory_bytes,
        )?;
        Ok(Box::new(vm))
    }

    fn existing(&self) -> Result<Vec<String>, Unavailable> {
        let Ok(entries) = std::fs::read_dir(&self.config.run_dir) else {
            return Ok(Vec::new());
        };
        let mut ids: Vec<String> = entries
            .flatten()
            .filter(|e| e.path().join("vm.json").is_file())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        ids.sort();
        Ok(ids)
    }
}

struct MicroVmSandbox {
    id: String,
    dir: PathBuf,
    child: Child,
    /// Set once Firecracker has exited, so threads still waiting for the guest give up.
    done: Arc<AtomicBool>,
    host: Option<JoinHandle<HostSide>>,
    deadline_at: Instant,
    stopped: bool,
    stop_asked: StopHandle,
    outcome: Option<Outcome>,
}

impl MicroVmSandbox {
    fn finish(&mut self) -> HostSide {
        self.done.store(true, Ordering::SeqCst);
        let side = self
            .host
            .take()
            .and_then(|h| h.join().ok())
            .unwrap_or(HostSide {
                report: Report::Silent,
                stdout: Vec::new(),
                stderr: Vec::new(),
            });
        let _ = std::fs::remove_dir_all(&self.dir);
        side
    }
}

impl Sandbox for MicroVmSandbox {
    fn id(&self) -> &str {
        &self.id
    }

    fn wait(&mut self) -> Outcome {
        if let Some(done) = &self.outcome {
            return done.clone();
        }
        let mut deadline_hit = false;
        let mut lost = false;
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {}
                Err(_) => {
                    lost = true;
                    break;
                }
            }
            if self.stop_asked.is_stopped() {
                self.stopped = true;
            }
            if !deadline_hit && !self.stopped && Instant::now() >= self.deadline_at {
                deadline_hit = true;
            }
            if self.stopped || deadline_hit {
                let _ = self.child.kill();
            }
            std::thread::sleep(POLL);
        }
        let _ = self.child.wait();
        let side = self.finish();
        let ending = if self.stopped {
            Ending::Stopped
        } else if deadline_hit {
            Ending::DeadlineExceeded
        } else if lost {
            Ending::Lost
        } else {
            match side.report {
                Report::Exited(code) => Ending::Exited(code),
                Report::Failed(_) | Report::Silent => Ending::Lost,
            }
        };
        let outcome = Outcome {
            ending,
            stdout: side.stdout,
            stderr: side.stderr,
        };
        self.outcome = Some(outcome.clone());
        outcome
    }

    fn stop(&mut self) {
        if self.outcome.is_none() {
            self.stopped = true;
            let _ = self.child.kill();
        }
    }

    fn stop_handle(&self) -> StopHandle {
        self.stop_asked.clone()
    }
}

// A VM that is dropped without being waited for must not outlive the engine's interest in it.
impl Drop for MicroVmSandbox {
    fn drop(&mut self) {
        if self.outcome.is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = self.finish();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_job_is_a_length_and_nul_terminated_fields() {
        let job = JobText {
            memory_bytes: 5,
            max_processes: 6,
            backstop: Duration::from_secs(7),
            workdir: "/w".into(),
            argv: vec!["sh".into(), "-c".into(), "a b".into()],
            env: vec![("A".into(), "x=y".into())],
        };
        let message = job.encode();
        let body = b"1\x005\x006\x007\x00/w\x003\x00sh\x00-c\x00a b\x00A=x=y\x00";
        assert_eq!(&message[..4], (body.len() as u32).to_be_bytes());
        assert_eq!(&message[4..], body);
    }

    #[test]
    fn a_microvm_driver_names_itself() {
        let config = MicroVmConfig {
            firecracker: "/nonexistent/firecracker".into(),
            kernel: "/nonexistent/vmlinux".into(),
            init: "/nonexistent/init".into(),
            cache_dir: "/tmp".into(),
            run_dir: "/tmp".into(),
        };
        let d = MicroVmSubstrate::new(config, "docker");
        assert_eq!(d.name(), "microvm");
        assert_eq!(d.isolation(), Isolation::MicroVm);
        assert_eq!(d.recorded_as().as_str(), "microvm.firecracker");
        assert!(d.available().is_err());
    }
}
