//! The container driver: sandboxes as OCI containers, run through the `docker` or `podman` command.
//!
//! Every sandbox gets no network at all (not a filtered one), a read-only root filesystem with a small
//! private `/tmp`, an unprivileged user, no capabilities, no way to gain privileges, limits on memory
//! and processes, and nothing inherited from the host. The one thing mounted in is the broker's
//! socket. Whether the host's kernel is shared with the sandbox, which is what makes this the weakest
//! boundary (ADR-0011), depends on the runtime: a rootless Podman is stronger than a root Docker
//! daemon.
//!
//! The same driver runs gVisor (ADR-0056): the same options under the OCI runtime the operator
//! registers as [`GVISOR_RUNTIME`], so the program talks to gVisor's kernel, not the host's. gVisor
//! keeps its processes inside its own kernel, where the host's process limit cannot count them, so
//! the driver also limits them there with `RLIMIT_NPROC`.
//!
//! Commands are run directly, never through a shell, and a spec is validated before anything is run.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hyalos_types::{Attestation, SubstrateName};

use crate::spec::{BROKER_SOCKET_IN_SANDBOX, OUTPUT_LIMIT, SKILLS_IN_SANDBOX};
use crate::{
    Ending, Isolation, Outcome, Sandbox, SandboxSpec, StartError, StopHandle, Substrate,
    Unavailable,
};

/// The user the sandbox runs as: `nobody`, which owns nothing on the host.
const SANDBOX_USER: &str = "65534:65534";

/// How often a waiting driver looks at the clock and the process.
const POLL: Duration = Duration::from_millis(10);
/// How long a sandbox that was killed may take to end before it is killed again.
const KILL_AGAIN: Duration = Duration::from_secs(1);

/// The label that marks a container as one of ours, for listing and for cleaning up after a crash.
const LABEL: &str = "hyalos.job";

/// The label on the readiness probe (ADR-0043). It is not a job's sandbox, so `existing` never
/// lists it.
const PROBE_LABEL: &str = "hyalos.probe";

/// The runtimes the engine looks for, in order of preference.
const RUNTIMES: [&str; 2] = ["podman", "docker"];

/// The name under which the operator registers gVisor's `runsc` with the container runtime, with the
/// options a job needs (ADR-0056): `--host-uds=open`, so the sandbox can connect to the broker's
/// socket. A name of its own, so an operator's other `runsc` settings are not a job's.
pub const GVISOR_RUNTIME: &str = "hyalos-runsc";

/// The fewest host tasks a gVisor sandbox may have. Its host cgroup counts gVisor's own threads, not
/// the program's processes, which `RLIMIT_NPROC` limits inside; too few and gVisor cannot start.
const GVISOR_HOST_TASKS: u32 = 256;

#[derive(Debug, Clone)]
pub struct ContainerSubstrate {
    runtime: String,
    /// Run every sandbox under [`GVISOR_RUNTIME`].
    gvisor: bool,
}

impl ContainerSubstrate {
    /// A driver for this command (`docker`, `podman` or the path to either).
    pub fn new(runtime: &str) -> Self {
        Self {
            runtime: runtime.to_string(),
            gvisor: false,
        }
    }

    /// A driver that runs every sandbox through this command under gVisor ([`GVISOR_RUNTIME`]).
    pub fn gvisor(runtime: &str) -> Self {
        Self {
            runtime: runtime.to_string(),
            gvisor: true,
        }
    }

    /// A driver for each runtime the engine looks for, in order of preference. Podman first: it runs
    /// without a root daemon.
    pub fn candidates() -> Vec<Self> {
        RUNTIMES.into_iter().map(Self::new).collect()
    }

    /// [`ContainerSubstrate::candidates`], each under gVisor.
    pub fn gvisor_candidates() -> Vec<Self> {
        RUNTIMES.into_iter().map(Self::gvisor).collect()
    }

    pub fn is_gvisor(&self) -> bool {
        self.gvisor
    }

    pub fn runtime(&self) -> &str {
        &self.runtime
    }

    /// Which of the runtimes it drives, by the command's file name, if it is one of them.
    fn known_runtime(&self) -> Option<&'static str> {
        let file = std::path::Path::new(&self.runtime).file_name()?;
        RUNTIMES.into_iter().find(|r| file == *r)
    }

    /// What to install or set when the runtime cannot start an init (ADR-0041).
    fn init_advice(&self) -> &'static str {
        match self.known_runtime() {
            Some("podman") => {
                "Podman runs the init named by `init_path` in containers.conf: install catatonit (the default), or point `init_path` at an init that is installed"
            }
            Some("docker") => {
                "Docker runs docker-init, which ships with Docker Engine: reinstall Docker Engine, or point `init-path` in daemon.json at an init that is installed"
            }
            _ => "install the init this runtime runs for `--init`",
        }
    }

    /// Whether the runtime has `image` locally. Jobs never pull (`--pull never`).
    fn has_image(&self, image: &str) -> Result<(), Unavailable> {
        let out = self.run(&["image", "inspect", "--format", "{{.Id}}", image])?;
        if out.status.success() {
            Ok(())
        } else {
            Err(Unavailable(format!(
                "{} has no image {image}; build or pull it first (hyalos setup says how)",
                self.runtime
            )))
        }
    }

    /// The OCI runtime option every container this driver starts is given, if any.
    fn runtime_option(&self) -> &'static [&'static str] {
        if self.gvisor {
            &["--runtime", GVISOR_RUNTIME]
        } else {
            &[]
        }
    }

    /// Starts and removes one short container from `image` under `--init`, the way a job's sandbox
    /// starts. A runtime with no init refuses it (ADR-0041). It runs `true`, or under gVisor
    /// `uname -r`, whose answer shows the kernel really is gVisor's (ADR-0056).
    fn starts_an_init(&self, image: &str) -> Result<(), Unavailable> {
        let (entrypoint, args): (&str, &[&str]) = if self.gvisor {
            ("uname", &["-r"])
        } else {
            ("true", &[])
        };
        let mut probe = vec!["run", "--rm"];
        probe.extend_from_slice(self.runtime_option());
        probe.extend_from_slice(&[
            "--init",
            "--network",
            "none",
            "--read-only",
            "--user",
            SANDBOX_USER,
            "--cap-drop",
            "ALL",
            "--security-opt",
            "no-new-privileges",
            "--pull",
            "never",
            "--log-driver",
            "none",
            "--label",
            PROBE_LABEL,
            "--entrypoint",
            entrypoint,
            image,
        ]);
        probe.extend_from_slice(args);
        let out = self.run(&probe)?;
        if !out.status.success() {
            let advice = if self.gvisor {
                GVISOR_ADVICE
            } else {
                self.init_advice()
            };
            return Err(Unavailable(format!(
                "{} cannot start a container with an init{} ({}). {advice}",
                self.runtime,
                if self.gvisor { " under gVisor" } else { "" },
                first_line(&out.stderr),
            )));
        }
        if self.gvisor {
            let kernel = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !is_gvisor_kernel(&kernel) {
                return Err(Unavailable(format!(
                    "{} runs {GVISOR_RUNTIME}, but its kernel is {kernel:?}, not gVisor's. {GVISOR_ADVICE}",
                    self.runtime
                )));
            }
        }
        Ok(())
    }

    fn run(&self, args: &[&str]) -> Result<std::process::Output, Unavailable> {
        Command::new(&self.runtime)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| Unavailable(format!("cannot run {}: {e}", self.runtime)))
    }
}

/// What to do when gVisor is not ready.
const GVISOR_ADVICE: &str = "Install gVisor and register it as the runtime hyalos-runsc with --host-uds=open (docs/operations/substrates.md). If runsc starts nothing on this kernel, as on WSL2, add --platform=kvm or --platform=ptrace";

/// Whether `release`, as `uname -r` prints it in a sandbox, is gVisor's kernel.
fn is_gvisor_kernel(release: &str) -> bool {
    release.ends_with("-gvisor")
}

fn first_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("no message")
        .to_string()
}

impl Substrate for ContainerSubstrate {
    fn name(&self) -> &'static str {
        if self.gvisor { "gvisor" } else { "container" }
    }

    /// `container.podman` or `container.docker` (`gvisor.podman`, `gvisor.docker` under gVisor), or
    /// the bare name for a command that is neither.
    fn recorded_as(&self) -> SubstrateName {
        self.known_runtime()
            .and_then(|r| SubstrateName::new(&format!("{}.{r}", self.name())))
            .or_else(|| SubstrateName::new(self.name()))
            .unwrap_or_else(SubstrateName::unrecorded)
    }

    fn isolation(&self) -> Isolation {
        if self.gvisor {
            Isolation::UserspaceKernel
        } else {
            Isolation::Container
        }
    }

    /// None: neither a container nor gVisor has a hardware root that signs what it started.
    fn attestation(&self) -> Attestation {
        Attestation::no_hardware_root()
    }

    fn ready(&self, image: &str) -> Result<(), Unavailable> {
        if !crate::spec::image_ok(image) {
            return Err(Unavailable(format!(
                "{image:?} is not a plain image reference"
            )));
        }
        self.available()?;
        self.has_image(image)?;
        self.starts_an_init(image)
    }

    fn available(&self) -> Result<(), Unavailable> {
        let out = self.run(&["version"])?;
        if out.status.success() {
            Ok(())
        } else {
            Err(Unavailable(format!(
                "{} did not answer: {}",
                self.runtime,
                first_line(&out.stderr)
            )))
        }
    }

    fn start(&self, spec: &SandboxSpec) -> Result<Box<dyn Sandbox>, StartError> {
        spec.validate().map_err(StartError::Spec)?;
        self.available().map_err(StartError::Unavailable)?;

        let (program, args) = spec
            .command
            .split_first()
            .ok_or(StartError::Spec(crate::SpecError::Command))?;
        // Under gVisor the host's process limit counts gVisor's threads, and the program's processes
        // are limited inside its kernel instead (ADR-0056).
        let host_tasks = if self.gvisor {
            spec.max_processes.max(GVISOR_HOST_TASKS)
        } else {
            spec.max_processes
        };
        let mut create = Command::new(&self.runtime);
        create
            .arg("create")
            .args(self.runtime_option())
            .args(["--label", &format!("{LABEL}={}", spec.job)])
            // A minimal init is PID 1 (ADR-0041): it reaps orphaned processes, which would
            // otherwise count against the process limit, and passes signals on to the program.
            .arg("--init")
            // No network of any kind, not even a filtered one.
            .args(["--network", "none"])
            .arg("--read-only")
            .args(["--tmpfs", "/tmp:rw,noexec,nosuid,size=64m"])
            .args(["--user", SANDBOX_USER])
            .args(["--cap-drop", "ALL"])
            .args(["--security-opt", "no-new-privileges"])
            .args(["--memory", &spec.memory_bytes.to_string()])
            .args(["--memory-swap", &spec.memory_bytes.to_string()])
            .args(["--pids-limit", &host_tasks.to_string()])
            // The image is run as asked, never pulled during a job, and its output is not kept by
            // the runtime.
            .args(["--pull", "never"])
            .args(["--log-driver", "none"])
            .arg("--mount")
            .arg(format!(
                "type=bind,src={},dst={BROKER_SOCKET_IN_SANDBOX}",
                spec.broker_socket.display()
            ));
        if let Some(skills) = &spec.skills {
            // Read-only, so nothing in the sandbox can change what the operator installed.
            create.arg("--mount").arg(format!(
                "type=bind,src={},dst={SKILLS_IN_SANDBOX},readonly",
                skills.display()
            ));
        }
        if self.gvisor {
            let n = spec.max_processes;
            create.args(["--ulimit", &format!("nproc={n}:{n}")]);
        }
        for (key, value) in &spec.env {
            create.arg("--env").arg(format!("{key}={value}"));
        }
        let created = create
            // The program is the entrypoint, so the image's own entrypoint cannot stand in front.
            .arg(format!("--entrypoint={program}"))
            .arg(&spec.image)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| StartError::Unavailable(Unavailable(e.to_string())))?;
        if !created.status.success() {
            return Err(StartError::Refused(first_line(&created.stderr)));
        }
        let id = String::from_utf8_lossy(&created.stdout).trim().to_string();
        if id.is_empty() {
            return Err(StartError::Refused(
                "the runtime gave no container id".into(),
            ));
        }

        let spawned = Command::new(&self.runtime)
            .args(["start", "--attach", &id])
            // Its own process group, so the operator's Ctrl-C reaches the engine and not the runtime;
            // the engine stops the sandbox itself (ADR-0053).
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(e) => {
                let _ = self.run(&["rm", "--force", &id]);
                return Err(StartError::Refused(format!("cannot start it: {e}")));
            }
        };
        let stdout = child.stdout.take().map(drain);
        let stderr = child.stderr.take().map(drain);
        Ok(Box::new(ContainerSandbox {
            driver: self.clone(),
            id,
            child,
            stdout,
            stderr,
            deadline_at: Instant::now() + spec.deadline,
            stopped: false,
            stop_asked: StopHandle::default(),
            outcome: None,
        }))
    }

    fn existing(&self) -> Result<Vec<String>, Unavailable> {
        let out = self.run(&[
            "ps",
            "--all",
            "--no-trunc",
            "--filter",
            &format!("label={LABEL}"),
            "--format",
            "{{.ID}}",
        ])?;
        if !out.status.success() {
            return Err(Unavailable(first_line(&out.stderr)));
        }
        Ok(String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect())
    }
}

/// Reads a stream to its end, keeping the first [`OUTPUT_LIMIT`] bytes.
fn drain<R: Read + Send + 'static>(mut stream: R) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
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

struct ContainerSandbox {
    driver: ContainerSubstrate,
    id: String,
    child: Child,
    stdout: Option<JoinHandle<Vec<u8>>>,
    stderr: Option<JoinHandle<Vec<u8>>>,
    deadline_at: Instant,
    stopped: bool,
    /// Set from another thread to stop the sandbox while `wait` runs.
    stop_asked: StopHandle,
    /// Set once the sandbox has ended and been removed.
    outcome: Option<Outcome>,
}

impl ContainerSandbox {
    fn kill(&self) {
        let _ = self.driver.run(&["kill", &self.id]);
    }

    fn exit_code(&self) -> Option<i32> {
        let out = self
            .driver
            .run(&["inspect", "--format", "{{.State.ExitCode}}", &self.id])
            .ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }

    fn remove(&self) {
        let _ = self.driver.run(&["rm", "--force", &self.id]);
    }
}

fn collect(handle: Option<JoinHandle<Vec<u8>>>) -> Vec<u8> {
    handle.and_then(|h| h.join().ok()).unwrap_or_default()
}

impl Sandbox for ContainerSandbox {
    fn id(&self) -> &str {
        &self.id
    }

    fn wait(&mut self) -> Outcome {
        if let Some(done) = &self.outcome {
            return done.clone();
        }
        let mut deadline_hit = false;
        let mut lost = false;
        // When the sandbox was last killed. A kill sent while the runtime is still starting the
        // container can be lost, so a sandbox that should be ending is killed again each second.
        let mut killed: Option<Instant> = None;
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
            let ending = self.stopped || deadline_hit;
            if ending && killed.is_none_or(|at| at.elapsed() >= KILL_AGAIN) {
                self.kill();
                killed = Some(Instant::now());
            }
            std::thread::sleep(POLL);
        }
        let code = self.exit_code();
        let _ = self.child.kill();
        let _ = self.child.wait();
        let ending = if self.stopped {
            Ending::Stopped
        } else if deadline_hit {
            Ending::DeadlineExceeded
        } else if lost {
            Ending::Lost
        } else {
            code.map_or(Ending::Lost, Ending::Exited)
        };
        let outcome = Outcome {
            ending,
            stdout: collect(self.stdout.take()),
            stderr: collect(self.stderr.take()),
        };
        self.remove();
        self.outcome = Some(outcome.clone());
        outcome
    }

    fn stop(&mut self) {
        if self.outcome.is_none() {
            self.stopped = true;
            self.kill();
        }
    }

    fn stop_handle(&self) -> StopHandle {
        self.stop_asked.clone()
    }
}

// A sandbox that is dropped without being waited for must not outlive the engine's interest in it.
impl Drop for ContainerSandbox {
    fn drop(&mut self) {
        if self.outcome.is_none() {
            self.kill();
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.remove();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_gvisor_s_kernel_counts_as_gvisor() {
        assert!(is_gvisor_kernel("4.19.0-gvisor"));
        for other in [
            "6.6.87.2-microsoft-standard-WSL2",
            "6.8.0-45-generic",
            "",
            "gvisor-6.1",
        ] {
            assert!(!is_gvisor_kernel(other), "{other}");
        }
    }

    #[test]
    fn a_gvisor_driver_names_its_runtime_and_isolation() {
        let d = ContainerSubstrate::gvisor("docker");
        assert_eq!(d.name(), "gvisor");
        assert_eq!(d.isolation(), Isolation::UserspaceKernel);
        assert_eq!(d.recorded_as().as_str(), "gvisor.docker");
        assert_eq!(d.runtime_option(), ["--runtime", GVISOR_RUNTIME]);
        assert_eq!(
            ContainerSubstrate::gvisor("/opt/nerdctl")
                .recorded_as()
                .as_str(),
            "gvisor"
        );
        assert!(
            ContainerSubstrate::new("docker")
                .runtime_option()
                .is_empty()
        );
    }
}
