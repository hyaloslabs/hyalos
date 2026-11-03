//! What a sandbox is asked to be, and how it ended.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use hyalos_types::JobId;

/// Where the broker's socket appears inside every sandbox, whatever the driver. The forwarder in the
/// sandbox (ADR-0016) connects here. The path is short on purpose: a Unix socket path is limited to
/// about 107 characters.
pub const BROKER_SOCKET_IN_SANDBOX: &str = "/run/broker.sock";

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
            deadline: Duration::from_secs(3600),
            memory_bytes: Self::DEFAULT_MEMORY_BYTES,
            max_processes: Self::DEFAULT_MAX_PROCESSES,
        }
    }

    /// Refuses a spec no driver should act on: one that could be read as a runtime option, an
    /// environment name that is not a name, or a path a mount option could be smuggled through.
    /// Every driver checks this first, so a bad spec does the same thing everywhere.
    pub fn validate(&self) -> Result<(), SpecError> {
        let image_ok = !self.image.is_empty()
            && !self.image.starts_with('-')
            && self.image.len() <= 255
            && self
                .image
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._/:@-".contains(&b));
        if !image_ok {
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
        let socket = self.broker_socket.to_string_lossy();
        let socket_ok = self.broker_socket.is_absolute()
            && !socket.is_empty()
            && !socket
                .bytes()
                .any(|b| matches!(b, b',' | b':' | b'\0' | b'\n'));
        if !socket_ok {
            return Err(SpecError::Socket);
        }
        if self.deadline.is_zero() || self.memory_bytes == 0 || self.max_processes == 0 {
            return Err(SpecError::Limits);
        }
        Ok(())
    }
}

/// Why a spec was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecError {
    /// Empty, too long, starting with `-`, or with characters an image reference does not have.
    Image,
    /// No program, or a NUL in an argument.
    Command,
    /// An environment name that is not a name, or a NUL in a value.
    Env(String),
    /// Not an absolute path, or one with a character a mount option reads as syntax.
    Socket,
    /// A zero deadline, memory or process limit.
    Limits,
}

impl fmt::Display for SpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpecError::Image => f.write_str("the image is not a plain image reference"),
            SpecError::Command => f.write_str("the command is empty or has a NUL"),
            SpecError::Env(name) => write!(f, "the environment variable {name:?} is not valid"),
            SpecError::Socket => f.write_str(
                "the broker socket must be an absolute path without ',' ':' or line breaks",
            ),
            SpecError::Limits => f.write_str("the deadline and the limits must not be zero"),
        }
    }
}

impl std::error::Error for SpecError {}

/// The driver cannot run on this host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unavailable(pub String);

impl fmt::Display for Unavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not available: {}", self.0)
    }
}

impl std::error::Error for Unavailable {}

/// A sandbox could not be started. Nothing is left running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartError {
    Spec(SpecError),
    Unavailable(Unavailable),
    /// The runtime refused: no such image, no permission, not enough resources. The message is the
    /// runtime's, for the operator.
    Refused(String),
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StartError::Spec(e) => write!(f, "cannot start: {e}"),
            StartError::Unavailable(e) => write!(f, "cannot start: {e}"),
            StartError::Refused(e) => write!(f, "cannot start: the runtime refused: {e}"),
        }
    }
}

impl std::error::Error for StartError {}

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
