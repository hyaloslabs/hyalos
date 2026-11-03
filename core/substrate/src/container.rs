//! The container driver: sandboxes as OCI containers, run through the `docker` or `podman` command.
//!
//! Every sandbox gets no network at all (not a filtered one), a read-only root filesystem with a small
//! private `/tmp`, an unprivileged user, no capabilities, no way to gain privileges, limits on memory
//! and processes, and nothing inherited from the host. The one thing mounted in is the broker's
//! socket. Whether the host's kernel is shared with the sandbox, which is what makes this the weakest
//! boundary (ADR-0011), depends on the runtime: a rootless Podman is stronger than a root Docker
//! daemon, and gVisor behind either is stronger again (story S2.2).
//!
//! Commands are run directly, never through a shell, and a spec is validated before anything is run.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::spec::{BROKER_SOCKET_IN_SANDBOX, OUTPUT_LIMIT};
use crate::{Ending, Isolation, Outcome, Sandbox, SandboxSpec, StartError, Substrate, Unavailable};

/// The user the sandbox runs as: `nobody`, which owns nothing on the host.
const SANDBOX_USER: &str = "65534:65534";

/// How often a waiting driver looks at the clock and the process.
const POLL: Duration = Duration::from_millis(10);

/// The label that marks a container as one of ours, for listing and for cleaning up after a crash.
const LABEL: &str = "hyalos.job";

#[derive(Debug, Clone)]
pub struct ContainerSubstrate {
    runtime: String,
}

impl ContainerSubstrate {
    /// A driver for this command (`docker`, `podman` or the path to either).
    pub fn new(runtime: &str) -> Self {
        Self {
            runtime: runtime.to_string(),
        }
    }

    /// The first of `podman` and `docker` that is installed and answers, or `None`. Podman first: it
    /// runs without a root daemon.
    pub fn detect() -> Option<Self> {
        ["podman", "docker"]
            .into_iter()
            .map(Self::new)
            .find(|d| d.available().is_ok())
    }

    pub fn runtime(&self) -> &str {
        &self.runtime
    }

    fn run(&self, args: &[&str]) -> Result<std::process::Output, Unavailable> {
        Command::new(&self.runtime)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| Unavailable(format!("cannot run {}: {e}", self.runtime)))
    }
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
        "container"
    }

    fn isolation(&self) -> Isolation {
        Isolation::Container
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
        let mut create = Command::new(&self.runtime);
        create
            .arg("create")
            .args(["--label", &format!("{LABEL}={}", spec.job)])
            // No network of any kind, not even a filtered one.
            .args(["--network", "none"])
            .arg("--read-only")
            .args(["--tmpfs", "/tmp:rw,noexec,nosuid,size=64m"])
            .args(["--user", SANDBOX_USER])
            .args(["--cap-drop", "ALL"])
            .args(["--security-opt", "no-new-privileges"])
            .args(["--memory", &spec.memory_bytes.to_string()])
            .args(["--memory-swap", &spec.memory_bytes.to_string()])
            .args(["--pids-limit", &spec.max_processes.to_string()])
            // The image is run as asked, never pulled during a job, and its output is not kept by
            // the runtime.
            .args(["--pull", "never"])
            .args(["--log-driver", "none"])
            .arg("--mount")
            .arg(format!(
                "type=bind,src={},dst={BROKER_SOCKET_IN_SANDBOX}",
                spec.broker_socket.display()
            ));
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
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {}
                Err(_) => {
                    lost = true;
                    break;
                }
            }
            if !deadline_hit && !self.stopped && Instant::now() >= self.deadline_at {
                deadline_hit = true;
                self.kill();
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
