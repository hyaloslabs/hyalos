//! The engine starts the broker and talks to it (ADR-0002, ADR-0035).
//!
//! The broker is this same executable run as `hyalos broker <socket>`, with an empty environment and
//! no standard input or output. The engine mints the manifest key for this run from the operating
//! system's entropy and keeps it in memory ([ADR-0022](../../../docs/adr/0022-engine-keys-generation-storage-backup-and-loss.md));
//! the broker is given only the public half, in `Hello`, over the channel that only it can reach.

use std::fmt;
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use hyalos_types::wire::{
    ForgeBases, MAX_SOCKET_PATH, ReaderCall, ToBroker, ToEngine, WIRE_VERSION, WireError,
    read_frame, write_frame,
};
use hyalos_types::workflow::{TypedValue, values_fit};
use hyalos_types::{CommitSha, Digest, JobId, ManifestKey, ManifestPublicKey, ReasonCode};
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

/// The job's socket, in its directory.
const SOCKET_NAME: &str = "broker.sock";

/// A directory made for one job's broker socket (ADR-0038). Its name is drawn from the operating
/// system, so only a process told the path can reach the socket in it; it can be entered but not
/// listed by others (0711), because the sandbox runs as another user. Dropping it removes it.
#[derive(Debug)]
pub struct JobDir {
    path: PathBuf,
}

impl JobDir {
    /// Makes a fresh directory in `runtime_dir`, refusing one that exists, and refusing a runtime
    /// directory whose path would make the job's socket longer than a socket path may be.
    pub fn create(runtime_dir: &Path) -> io::Result<Self> {
        let mut name = [0u8; 16];
        getrandom::fill(&mut name).map_err(|e| io::Error::other(e.to_string()))?;
        let hex: String = name.iter().map(|b| format!("{b:02x}")).collect();
        let path = runtime_dir.join(format!("hyalos-job-{hex}"));
        let socket = path.join(SOCKET_NAME).as_os_str().len();
        if socket > MAX_SOCKET_PATH {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "the runtime directory {} is too long for a job's socket: the socket's path \
                     would be {socket} bytes, and a socket path may be at most {MAX_SOCKET_PATH}. \
                     Use a shorter runtime directory.",
                    runtime_dir.display()
                ),
            ));
        }
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
        self.path.join(SOCKET_NAME)
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

/// What the broker said, or that it said nothing yet.
#[derive(Debug, PartialEq, Eq)]
pub enum Incoming {
    Message(ToEngine),
    /// The broker closed the channel.
    Closed,
    /// Nothing arrived in the time given. The channel is still open.
    Nothing,
}

/// A running broker and the channel to it. Dropping it stops the broker.
///
/// After the handshake one thread reads the channel and hands each frame over, so the engine can wait
/// for a report from the broker and for something else at once, and a job can be quiet for longer
/// than [`IO_TIMEOUT`] without the broker being taken for hung (ADR-0050).
#[derive(Debug)]
pub struct BrokerProcess {
    child: Child,
    stream: Option<UnixStream>,
    incoming: Option<Receiver<Result<Option<ToEngine>, WireError>>>,
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
        Self::launch_configured(exe, runtime_dir, key, None, None, wait)
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
        Self::launch_configured(exe, runtime_dir, key, provider_keys, None, wait)
    }

    /// As [`launch_with`](Self::launch_with), and tells the broker in `Hello` where the forge is
    /// (ADR-0050); `None` is github.com.
    pub fn launch_configured(
        exe: &Path,
        runtime_dir: &Path,
        key: &ManifestPublicKey,
        provider_keys: Option<&Path>,
        forge: Option<ForgeBases>,
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
            .stdout(Stdio::null())
            // Its own process group, so the operator's Ctrl-C at a terminal reaches the engine and
            // not the broker, and the engine can still end the job in it (ADR-0053). It stops on
            // `shutdown` or when the engine's end of the channel closes.
            .process_group(0);
        Self::start_with(command, listener, key, forge, wait)
    }

    /// Starts this very executable as the broker, with the provider key file at `provider_keys` and
    /// the forge at `forge`.
    pub fn launch_self(
        runtime_dir: &Path,
        key: &ManifestPublicKey,
        provider_keys: Option<&Path>,
        forge: Option<ForgeBases>,
        wait: Duration,
    ) -> Result<Self, LaunchError> {
        let exe = std::env::current_exe().map_err(|e| LaunchError::Spawn(e.kind()))?;
        Self::launch_configured(&exe, runtime_dir, key, provider_keys, forge, wait)
    }

    /// Runs `command` as the broker, accepts its connection on `listener` and says hello. Anything
    /// that is not the process it started is turned away. [`launch`](Self::launch) builds the command
    /// with an empty environment and no standard streams; this is split from it so a test can stand in
    /// for the broker.
    pub fn start(
        command: Command,
        listener: Listener,
        key: &ManifestPublicKey,
        wait: Duration,
    ) -> Result<Self, LaunchError> {
        Self::start_with(command, listener, key, None, wait)
    }

    /// As [`start`](Self::start), naming the forge in `Hello`.
    pub fn start_with(
        mut command: Command,
        mut listener: Listener,
        key: &ManifestPublicKey,
        forge: Option<ForgeBases>,
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
            incoming: None,
            refused,
        };
        broker.handshake(key, forge)?;
        broker.start_reading()?;
        Ok(broker)
    }

    /// Hands reading to a thread of its own, which reads frames until the channel ends or a frame is
    /// bad, and passes each one on. It reads with no timeout: how long the broker may be quiet is for
    /// whoever waits on [`poll`](Self::poll) to say.
    fn start_reading(&mut self) -> Result<(), LaunchError> {
        let stream = self.stream.as_ref().ok_or(WireError::Truncated)?;
        let mut reader = stream
            .try_clone()
            .map_err(|e| LaunchError::Wire(WireError::Io(e.kind())))?;
        reader
            .set_read_timeout(None)
            .map_err(|e| LaunchError::Wire(WireError::Io(e.kind())))?;
        let (to_engine, incoming) = mpsc::channel();
        std::thread::spawn(move || {
            loop {
                let frame = read_frame::<_, ToEngine>(&mut reader);
                let last = !matches!(frame, Ok(Some(_)));
                if to_engine.send(frame).is_err() || last {
                    return;
                }
            }
        });
        self.incoming = Some(incoming);
        Ok(())
    }

    fn handshake(
        &mut self,
        key: &ManifestPublicKey,
        forge: Option<ForgeBases>,
    ) -> Result<(), LaunchError> {
        let stream = self.stream.as_ref().ok_or(WireError::Truncated)?;
        stream
            .set_read_timeout(Some(IO_TIMEOUT))
            .and_then(|()| stream.set_write_timeout(Some(IO_TIMEOUT)))
            .map_err(|e| LaunchError::Wire(WireError::Io(e.kind())))?;
        self.send(&ToBroker::Hello {
            version: WIRE_VERSION,
            manifest_key: *key,
            forge,
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
        match self.poll(IO_TIMEOUT)? {
            Incoming::Message(message) => Ok(Some(message)),
            Incoming::Closed => Ok(None),
            // The broker did not answer in time: it is treated as hung.
            Incoming::Nothing => {
                self.close();
                Err(WireError::Io(io::ErrorKind::TimedOut))
            }
        }
    }

    /// Waits up to `wait` for the broker's next message. Unlike [`recv`](Self::recv), silence is not
    /// an error and leaves the channel open; anything else that is not a message closes it, as there.
    pub fn poll(&mut self, wait: Duration) -> Result<Incoming, WireError> {
        if self.stream.is_none() {
            return Err(WireError::Truncated);
        }
        let result = match &self.incoming {
            // Before the reader starts: the handshake reads here itself, under the channel's timeout.
            None => match self.stream.as_mut() {
                Some(stream) => read_frame::<_, ToEngine>(stream),
                None => Err(WireError::Truncated),
            },
            Some(incoming) => match incoming.recv_timeout(wait) {
                Ok(frame) => frame,
                Err(RecvTimeoutError::Timeout) => return Ok(Incoming::Nothing),
                Err(RecvTimeoutError::Disconnected) => Err(WireError::Truncated),
            },
        };
        match result {
            Ok(Some(message)) => Ok(Incoming::Message(message)),
            Ok(None) => {
                self.close();
                Ok(Incoming::Closed)
            }
            Err(e) => {
                self.close();
                Err(e)
            }
        }
    }

    /// Runs a reader step in the broker (ADR-0061) and waits up to `wait` for its typed values,
    /// checked against the engine's own copy of the step's output. `Err` is why there are none: the
    /// broker's refusal (it has ended the job), or one of the engine's own codes. A broker that answers
    /// values that do not fit, or answers something else, has broken the protocol and is stopped.
    pub fn read(
        &mut self,
        job: JobId,
        call: &ReaderCall,
        wait: Duration,
    ) -> Result<Vec<TypedValue>, ReasonCode> {
        let code = |text: &str| ReasonCode::new(text).unwrap_or_else(ReasonCode::unavailable);
        self.send(&ToBroker::Read {
            job,
            call: call.clone(),
        })
        .map_err(|_| code("reader.broker_unavailable"))?;
        match self.poll(wait) {
            Ok(Incoming::Message(ToEngine::Values { job: read, values }))
                if read == job && values_fit(&call.output, &values) =>
            {
                Ok(values)
            }
            Ok(Incoming::Message(ToEngine::Refused { job: read, reason })) if read == job => {
                Err(reason)
            }
            Ok(Incoming::Nothing) => {
                self.close();
                Err(code("reader.no_answer"))
            }
            Ok(Incoming::Message(_)) => {
                self.close();
                Err(code("reader.bad_answer"))
            }
            Ok(Incoming::Closed) | Err(_) => Err(code("reader.broker_unavailable")),
        }
    }

    /// Asks the broker to hold issue `number` as a handle for `job` (ADR-0063) and waits up to `wait`
    /// for its digest, which is all the engine learns of it. `Err` is why there is none: the broker's
    /// refusal (it has ended the job), or one of the engine's own codes. A broker that answers
    /// something else has broken the protocol and is stopped.
    pub fn hold_issue(
        &mut self,
        job: JobId,
        number: u64,
        wait: Duration,
    ) -> Result<Digest, ReasonCode> {
        let code = |text: &str| ReasonCode::new(text).unwrap_or_else(ReasonCode::unavailable);
        self.send(&ToBroker::HoldIssue { job, number })
            .map_err(|_| code("handle.broker_unavailable"))?;
        match self.poll(wait) {
            Ok(Incoming::Message(ToEngine::Held { job: held, handle })) if held == job => {
                Ok(handle)
            }
            Ok(Incoming::Message(ToEngine::Refused { job: held, reason })) if held == job => {
                Err(reason)
            }
            Ok(Incoming::Nothing) => {
                self.close();
                Err(code("handle.no_answer"))
            }
            Ok(Incoming::Message(_)) => {
                self.close();
                Err(code("handle.bad_answer"))
            }
            Ok(Incoming::Closed) | Err(_) => Err(code("handle.broker_unavailable")),
        }
    }

    /// Asks the broker to merge the pull request `job`'s manifest approval names (ADR-0071) and waits
    /// up to `wait` for the merge commit. `Err` is why there is none: the broker's refusal (it has
    /// ended the job), or one of the engine's own codes. A broker that answers something else has
    /// broken the protocol and is stopped.
    pub fn merge(&mut self, job: JobId, wait: Duration) -> Result<CommitSha, ReasonCode> {
        let code = |text: &str| ReasonCode::new(text).unwrap_or_else(ReasonCode::unavailable);
        self.send(&ToBroker::Merge { job })
            .map_err(|_| code("merge.broker_unavailable"))?;
        match self.poll(wait) {
            Ok(Incoming::Message(ToEngine::Merged { job: merged, sha })) if merged == job => {
                Ok(sha)
            }
            Ok(Incoming::Message(ToEngine::Refused {
                job: merged,
                reason,
            })) if merged == job => Err(reason),
            Ok(Incoming::Nothing) => {
                self.close();
                Err(code("merge.no_answer"))
            }
            Ok(Incoming::Message(_)) => {
                self.close();
                Err(code("merge.bad_answer"))
            }
            Ok(Incoming::Closed) | Err(_) => Err(code("merge.broker_unavailable")),
        }
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
