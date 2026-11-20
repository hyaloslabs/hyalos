//! What a sandbox is asked to be, and how it ended.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use hyalos_types::JobId;

/// Where the broker's socket appears inside every sandbox, whatever the driver. The forwarder in the
/// sandbox (ADR-0016) connects here. The path is short on purpose: a Unix socket path is limited to
/// about 107 characters.
pub const BROKER_SOCKET_IN_SANDBOX: &str = "/run/broker.sock";

/// Where a job's installed skills appear inside the sandbox, read-only (ADR-0089).
pub const SKILLS_IN_SANDBOX: &str = "/opt/hyalos/skills";

/// The most output a sandbox's stdout or stderr is kept to. The rest is read and thrown away, so a
/// sandbox cannot make the engine hold unbounded memory.
pub const OUTPUT_LIMIT: usize = 64 * 1024;

/// How strong a boundary a driver gives, weakest first (ADR-0011).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Isolation {
    /// Shares the host's kernel, behind namespaces and dropped privileges.
    Container,
    /// Runs on a kernel written for isolation (gVisor).
    UserspaceKernel,
    /// Its own kernel in a lightweight virtual machine.
    MicroVm,
}

/// What a sandbox is asked to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxSpec {
    pub job: JobId,
    /// The image or root filesystem to run, as the driver understands it.
    pub image: String,
    /// The program and its arguments. Never run through a shell by the driver.
    pub command: Vec<String>,
    /// The environment inside. It is the whole environment: nothing is inherited from the host. These
    /// values are visible to anyone who can list host processes or inspect the sandbox, so they hold
    /// placeholders and settings, never a secret (ADR-0002).
    pub env: BTreeMap<String, String>,
    /// The broker's Unix socket on the host. It appears at [`BROKER_SOCKET_IN_SANDBOX`] and is the
    /// only way out. It must be connectable by the unprivileged user the sandbox runs as.
    pub broker_socket: PathBuf,
    /// A directory on the host holding the skills the job was given, one directory each, which
    /// appears read-only at [`SKILLS_IN_SANDBOX`] (ADR-0089). A driver that cannot carry it refuses
    /// the spec rather than start without it.
    pub skills: Option<PathBuf>,
    /// How long the sandbox may run, from the start. At the deadline it is killed.
    pub deadline: Duration,
    /// Memory the sandbox may use.
    pub memory_bytes: u64,
    /// Processes and threads the sandbox may have.
    pub max_processes: u32,
}

impl SandboxSpec {
    pub const DEFAULT_MEMORY_BYTES: u64 = 1024 * 1024 * 1024;
    pub const DEFAULT_MAX_PROCESSES: u32 = 256;

    pub fn new(job: JobId, image: &str, command: Vec<String>, broker_socket: PathBuf) -> Self {
        Self {
            job,
            image: image.to_string(),
            command,
            env: BTreeMap::new(),
            broker_socket,
            skills: None,
            deadline: Duration::from_secs(3600),
            memory_bytes: Self::DEFAULT_MEMORY_BYTES,
            max_processes: Self::DEFAULT_MAX_PROCESSES,
        }
    }

    /// Refuses a spec no driver should act on: one that could be read as a runtime option, an
    /// environment name that is not a name, or a path a mount option could be smuggled through.
    /// Every driver checks this first, so a bad spec does the same thing everywhere.
    pub fn validate(&self) -> Result<(), SpecError> {
        if !image_ok(&self.image) {
            return Err(SpecError::Image);
        }
        if self.command.is_empty() || self.command.iter().any(|a| a.contains('\0')) {
            return Err(SpecError::Command);
        }
        for (key, value) in &self.env {
            let name_ok = key
                .bytes()
                .next()
                .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
                && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
            if !name_ok || value.contains('\0') {
                return Err(SpecError::Env(key.clone()));
            }
        }
        if !mount_source_ok(&self.broker_socket) {
            return Err(SpecError::Socket);
        }
        if self.skills.as_ref().is_some_and(|p| !mount_source_ok(p)) {
            return Err(SpecError::Skills);
        }
        if self.deadline.is_zero() || self.memory_bytes == 0 || self.max_processes == 0 {
            return Err(SpecError::Limits);
        }
        Ok(())
    }
}

/// Whether `path` can be named in a mount option: absolute, with no character the option reads as
/// syntax.
fn mount_source_ok(path: &std::path::Path) -> bool {
    let text = path.to_string_lossy();
    path.is_absolute()
        && !text.is_empty()
        && !text
            .bytes()
            .any(|b| matches!(b, b',' | b':' | b'\0' | b'\n'))
}

/// Whether `image` is a plain image reference, one that cannot be read as a runtime option.
pub(crate) fn image_ok(image: &str) -> bool {
    !image.is_empty()
        && !image.starts_with('-')
        && image.len() <= 255
        && image
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._/:@-".contains(&b))
}

/// Why a spec was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SpecError {
    /// Empty, too long, starting with `-`, or with characters an image reference does not have.
    #[error("the image is not a plain image reference")]
    Image,
    /// No program, or a NUL in an argument.
    #[error("the command is empty or has a NUL")]
    Command,
    /// An environment name that is not a name, or a NUL in a value.
    #[error("the environment variable {0:?} is not valid")]
    Env(String),
    /// Not an absolute path, or one with a character a mount option reads as syntax.
    #[error("the broker socket must be an absolute path without ',' ':' or line breaks")]
    Socket,
    /// A zero deadline, memory or process limit.
    #[error("the deadline and the limits must not be zero")]
    Limits,
    /// The skills directory is not an absolute path, or has a character a mount option reads as
    /// syntax.
    #[error("the skills directory must be an absolute path without ',' ':' or line breaks")]
    Skills,
    /// The job was given skills and this driver cannot carry them into the sandbox.
    #[error(
        "this substrate cannot carry skills into the sandbox yet; choose a container or gVisor substrate for a job with skills"
    )]
    SkillsUnsupported,
}

/// The driver cannot run on this host.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not available: {0}")]
pub struct Unavailable(pub String);

/// A sandbox could not be started. Nothing is left running.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StartError {
    #[error("cannot start: {0}")]
    Spec(SpecError),
    #[error("cannot start: {0}")]
    Unavailable(Unavailable),
    /// The runtime refused: no such image, no permission, not enough resources. The message is the
    /// runtime's, for the operator.
    #[error("cannot start: the runtime refused: {0}")]
    Refused(String),
}

/// How a sandbox ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// The program exited with this code.
    Exited(i32),
    /// It was still running at its deadline and was killed.
    DeadlineExceeded,
    /// It was stopped by the engine.
    Stopped,
    /// The runtime lost it or could not report how it ended.
    Lost,
}

/// How a sandbox ended and what it printed (at most [`OUTPUT_LIMIT`] bytes of each stream).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub ending: Ending,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Outcome {
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
}
