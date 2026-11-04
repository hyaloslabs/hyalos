//! The engine starts the broker and talks to it (ADR-0002, ADR-0035).
//!
//! The broker is this same executable run as `hyalos broker <socket>`, with an empty environment and
//! no standard input or output. The engine mints the manifest key for this run from the operating
//! system's entropy and keeps it in memory ([ADR-0022](../../../docs/adr/0022-engine-keys-generation-storage-backup-and-loss.md));
//! the broker is given only the public half, in `Hello`, over the channel that only it can reach.

use std::fmt;
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use hyalos_types::wire::{ToBroker, ToEngine, WIRE_VERSION, WireError, read_frame, write_frame};
use hyalos_types::{ManifestKey, ManifestPublicKey};
use std::os::unix::net::UnixStream;
use zeroize::Zeroizing;

use crate::channel::{ChannelError, Listener, Peer};

/// How long a read or write on the channel may block before the broker is treated as hung.
pub const IO_TIMEOUT: Duration = Duration::from_secs(30);

/// A fresh manifest key from the operating system. It is not stored anywhere.
pub fn fresh_manifest_key() -> Result<ManifestKey, io::Error> {
    let mut seed = Zeroizing::new([0u8; 32]);
    getrandom::fill(&mut *seed).map_err(|e| io::Error::other(e.to_string()))?;
    Ok(ManifestKey::from_seed(*seed))
}

/// A directory made for one job's broker socket (ADR-0038). Its name is drawn from the operating
/// system, so only a process told the path can reach the socket in it; it can be entered but not
/// listed by others (0711), because the sandbox runs as another user. Dropping it removes it.
#[derive(Debug)]
pub struct JobDir {
    path: PathBuf,
}

impl JobDir {
    /// Makes a fresh directory in `runtime_dir`, refusing one that exists.
    pub fn create(runtime_dir: &Path) -> io::Result<Self> {
        let mut name = [0u8; 16];
        getrandom::fill(&mut name).map_err(|e| io::Error::other(e.to_string()))?;
        let hex: String = name.iter().map(|b| format!("{b:02x}")).collect();
        let path = runtime_dir.join(format!("hyalos-job-{hex}"));
        std::fs::DirBuilder::new().mode(0o711).create(&path)?;
        // The mode given is narrowed by the umask; set it exactly.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o711))?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The socket the broker listens on for this job, and the sandbox's `broker_socket`.
    pub fn socket(&self) -> PathBuf {
        self.path.join("broker.sock")
    }
}

impl Drop for JobDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[derive(Debug)]
pub enum LaunchError {
    Channel(ChannelError),
    Spawn(io::ErrorKind),
    Wire(WireError),
    /// The broker answered `Hello` with something other than `Ready` at this protocol version.
    Handshake,
}

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LaunchError::Channel(e) => write!(f, "{e}"),
            LaunchError::Spawn(kind) => write!(f, "the broker could not be started: {kind}"),
            LaunchError::Wire(e) => write!(f, "{e}"),
            LaunchError::Handshake => f.write_str("the broker did not answer hello as it should"),
        }
    }
}

impl std::error::Error for LaunchError {}

impl From<ChannelError> for LaunchError {
    fn from(e: ChannelError) -> Self {
        LaunchError::Channel(e)
    }
}

impl From<WireError> for LaunchError {
    fn from(e: WireError) -> Self {
        LaunchError::Wire(e)
    }
}

/// A running broker and the channel to it. Dropping it stops the broker.
#[derive(Debug)]
pub struct BrokerProcess {
    child: Child,
    stream: Option<UnixStream>,
    refused: Vec<Option<Peer>>,
}

impl BrokerProcess {
    /// Starts `exe` as the broker, with `runtime_dir` for its socket and no provider keys.
    pub fn launch(
        exe: &Path,
        runtime_dir: &Path,
        key: &ManifestPublicKey,
        wait: Duration,
    ) -> Result<Self, LaunchError> {
        Self::launch_with(exe, runtime_dir, key, None, wait)
    }

    /// Starts `exe` as the broker, which reads the provider key file at `provider_keys` itself, so
    /// the keys never pass through the engine (ADR-0038). The path is not a secret; the file is the
    /// operator's alone.
    pub fn launch_with(
        exe: &Path,
        runtime_dir: &Path,
        key: &ManifestPublicKey,
        provider_keys: Option<&Path>,
        wait: Duration,
    ) -> Result<Self, LaunchError> {
        let listener = Listener::bind(runtime_dir)?;
        let mut command = Command::new(exe);
        command.arg("broker").arg(listener.path());
        if let Some(keys) = provider_keys {
            command.arg(keys);
        }
        command
            // Nothing from the engine's environment, and no way in or out but the channel.
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        Self::start(command, listener, key, wait)
    }

    /// Starts this very executable as the broker, with the provider key file at `provider_keys`.
    pub fn launch_self(
        runtime_dir: &Path,
        key: &ManifestPublicKey,
        provider_keys: Option<&Path>,
        wait: Duration,
    ) -> Result<Self, LaunchError> {
        let exe = std::env::current_exe().map_err(|e| LaunchError::Spawn(e.kind()))?;
        Self::launch_with(&exe, runtime_dir, key, provider_keys, wait)
    }

    /// Runs `command` as the broker, accepts its connection on `listener` and says hello. Anything
    /// that is not the process it started is turned away. [`launch`](Self::launch) builds the command
    /// with an empty environment and no standard streams; this is split from it so a test can stand in
    /// for the broker.
    pub fn start(
        mut command: Command,
        mut listener: Listener,
        key: &ManifestPublicKey,
        wait: Duration,
    ) -> Result<Self, LaunchError> {
        let mut child = command.spawn().map_err(|e| LaunchError::Spawn(e.kind()))?;
        let stream = match listener.accept(&mut child, wait) {
            Ok(stream) => stream,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.into());
            }
        };
        let refused = listener.refused().to_vec();
        // Nothing more can connect: the socket and its directory go now.
        drop(listener);
        let mut broker = Self {
            child,
            stream: Some(stream),
            refused,
        };
        broker.handshake(key)?;
        Ok(broker)
    }

    fn handshake(&mut self, key: &ManifestPublicKey) -> Result<(), LaunchError> {
        let stream = self.stream.as_ref().ok_or(WireError::Truncated)?;
        stream
            .set_read_timeout(Some(IO_TIMEOUT))
            .and_then(|()| stream.set_write_timeout(Some(IO_TIMEOUT)))
            .map_err(|e| LaunchError::Wire(WireError::Io(e.kind())))?;
        self.send(&ToBroker::Hello {
            version: WIRE_VERSION,
            manifest_key: *key,
        })?;
        match self.recv()? {
            Some(ToEngine::Ready { version }) if version == WIRE_VERSION => Ok(()),
            _ => {
                self.close();
                Err(LaunchError::Handshake)
            }
        }
    }

    /// Everyone who connected to the socket and was turned away before the broker did.
    pub fn refused_peers(&self) -> &[Option<Peer>] {
        &self.refused
    }

    /// The broker's process id.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Sends a message. A failure closes the channel and stops the broker.
    pub fn send(&mut self, message: &ToBroker) -> Result<(), WireError> {
        let Some(stream) = self.stream.as_mut() else {
            return Err(WireError::Truncated);
        };
        let result = write_frame(stream, message);
        if result.is_err() {
            self.close();
        }
        result
    }

    /// Reads the next message from the broker, which is a typed value or an error. Anything that is
    /// not a message the broker may send (a field the type does not have, a variant it does not know,
    /// free text where a code goes, an oversize frame) closes the channel and stops the broker: the
    /// engine does not go on talking to a broker that has sent it something else. `Ok(None)` is the
    /// broker closing the channel.
    pub fn recv(&mut self) -> Result<Option<ToEngine>, WireError> {
        let Some(stream) = self.stream.as_mut() else {
            return Err(WireError::Truncated);
        };
        let result = read_frame::<_, ToEngine>(stream);
        if !matches!(result, Ok(Some(_))) {
            self.close();
        }
        result
    }

    /// Whether the channel is still open.
    pub fn is_open(&self) -> bool {
        self.stream.is_some()
    }

    /// Closes the channel and stops the broker. Safe to call again.
    pub fn close(&mut self) {
        self.stream = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// Waits for the broker to exit on its own, after `Shutdown` or a closed channel.
    pub fn wait(&mut self) -> io::Result<Option<i32>> {
        self.stream = None;
        Ok(self.child.wait()?.code())
    }
}

impl Drop for BrokerProcess {
    fn drop(&mut self) {
        self.close();
    }
}
