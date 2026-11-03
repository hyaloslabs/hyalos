//! The private channel between the engine and the broker (ADR-0002, ADR-0035).
//!
//! The engine makes a directory only it can enter, listens on a socket in it, starts the broker and
//! hands it the path. It accepts exactly one connection, from the process it started, and checks that
//! with the kernel's record of who is on the other end (`SO_PEERCRED`: the process id and user id of
//! the peer, which the peer cannot choose). Any other connection is closed unread. Then it stops
//! listening and removes the socket, so there is nothing left to connect to. The broker checks the
//! other way: whoever it reached must be its own parent, running as the same user.
//!
//! This relies on the kernel's peer credentials, which Linux has. On another system the channel does
//! not open, rather than open unchecked.

use std::fmt;
use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::{Duration, Instant};

/// Who is on the other end of a socket, as the kernel says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Peer {
    pub pid: u32,
    pub uid: u32,
}

/// Why the channel did not open.
#[derive(Debug)]
pub enum ChannelError {
    Io(io::ErrorKind),
    /// The kernel would not say who the peer is, or this system cannot.
    PeerUnknown,
    /// The process that was started ended before it connected.
    ChildExited(Option<i32>),
    /// Nothing the right process connected in time.
    Timeout,
    /// The broker reached a process that is not its parent, or not the same user's.
    NotMyEngine(Peer),
    /// The broker was given a path that is not a socket path it can use.
    BadPath,
}

impl fmt::Display for ChannelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChannelError::Io(kind) => write!(f, "the channel could not be set up: {kind}"),
            ChannelError::PeerUnknown => {
                f.write_str("cannot tell who is on the other end of the channel, so it is not used")
            }
            ChannelError::ChildExited(Some(code)) => {
                write!(
                    f,
                    "the broker exited with status {code} before it connected"
                )
            }
            ChannelError::ChildExited(None) => {
                f.write_str("the broker was stopped before it connected")
            }
            ChannelError::Timeout => f.write_str("the broker did not connect in time"),
            ChannelError::NotMyEngine(p) => write!(
                f,
                "the process at the other end (pid {}, uid {}) is not the engine that started this broker",
                p.pid, p.uid
            ),
            ChannelError::BadPath => {
                f.write_str("the broker needs the path of the engine's socket")
            }
        }
    }
}

impl std::error::Error for ChannelError {}

impl From<io::Error> for ChannelError {
    fn from(e: io::Error) -> Self {
        ChannelError::Io(e.kind())
    }
}

/// The peer's process and user, from the kernel.
#[cfg(target_os = "linux")]
pub fn peer_of(stream: &UnixStream) -> Result<Peer, ChannelError> {
    let cred =
        rustix::net::sockopt::socket_peercred(stream).map_err(|_| ChannelError::PeerUnknown)?;
    let pid = u32::try_from(cred.pid.as_raw_pid()).map_err(|_| ChannelError::PeerUnknown)?;
    Ok(Peer {
        pid,
        uid: cred.uid.as_raw(),
    })
}

#[cfg(not(target_os = "linux"))]
pub fn peer_of(_stream: &UnixStream) -> Result<Peer, ChannelError> {
    Err(ChannelError::PeerUnknown)
}

/// Whether `peer` is the process `pid`, running as user `uid`. Both must match.
pub fn is_expected(peer: Peer, pid: u32, uid: u32) -> bool {
    peer.pid == pid && peer.uid == uid
}

/// The engine's listening socket, in a directory only the engine's user can enter.
#[derive(Debug)]
pub struct Listener {
    dir: PathBuf,
    socket: PathBuf,
    inner: UnixListener,
    refused: Vec<Option<Peer>>,
}

impl Listener {
    /// Makes a new directory inside `base` (mode 0700, a name drawn from the operating system, and
    /// refused if it already exists) and listens on a socket in it (mode 0600).
    pub fn bind(base: &Path) -> Result<Self, ChannelError> {
        let mut name = [0u8; 8];
        getrandom::fill(&mut name).map_err(|_| ChannelError::Io(io::ErrorKind::Other))?;
        let dir = base.join(format!("hyalos-{}", hyalos_types::hex::to_hex(&name)));
        fs::DirBuilder::new().mode(0o700).create(&dir)?;
        let socket = dir.join("broker.sock");
        let bound = UnixListener::bind(&socket).and_then(|inner| {
            fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
            inner.set_nonblocking(true)?;
            Ok(inner)
        });
        match bound {
            Ok(inner) => Ok(Self {
                dir,
                socket,
                inner,
                refused: Vec::new(),
            }),
            Err(e) => {
                let _ = fs::remove_file(&socket);
                let _ = fs::remove_dir(&dir);
                Err(e.into())
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.socket
    }

    /// Everyone who connected and was turned away, as the kernel named them (`None` if it would
    /// not).
    pub fn refused(&self) -> &[Option<Peer>] {
        &self.refused
    }

    /// Waits for `child` to connect, up to `wait`. A connection from anyone else, whatever it sends,
    /// is closed unread and noted, and waiting goes on. Fails if `child` exits first.
    pub fn accept(
        &mut self,
        child: &mut Child,
        wait: Duration,
    ) -> Result<UnixStream, ChannelError> {
        let uid = rustix::process::geteuid().as_raw();
        let pid = child.id();
        let deadline = Instant::now() + wait;
        loop {
            match self.inner.accept() {
                Ok((stream, _)) => match peer_of(&stream) {
                    Ok(peer) if is_expected(peer, pid, uid) => {
                        stream.set_nonblocking(false)?;
                        return Ok(stream);
                    }
                    Ok(peer) => self.refused.push(Some(peer)),
                    Err(_) => self.refused.push(None),
                },
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    if let Some(status) = child.try_wait()? {
                        return Err(ChannelError::ChildExited(status.code()));
                    }
                    if Instant::now() >= deadline {
                        return Err(ChannelError::Timeout);
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket);
        let _ = fs::remove_dir(&self.dir);
    }
}

/// The broker's side: connects to the engine's socket and checks that the process that answers is
/// its own parent, running as the same user. If it is not, nothing is sent.
pub fn connect_to_engine(path: &Path) -> Result<UnixStream, ChannelError> {
    if !path.is_absolute() {
        return Err(ChannelError::BadPath);
    }
    let stream = UnixStream::connect(path)?;
    let peer = peer_of(&stream)?;
    let parent = rustix::process::getppid()
        .and_then(|p| u32::try_from(p.as_raw_pid()).ok())
        .ok_or(ChannelError::PeerUnknown)?;
    let uid = rustix::process::geteuid().as_raw();
    if !is_expected(peer, parent, uid) {
        return Err(ChannelError::NotMyEngine(peer));
    }
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_peer_is_expected_only_if_both_its_process_and_its_user_match() {
        let peer = Peer { pid: 10, uid: 1000 };
        assert!(is_expected(peer, 10, 1000));
        assert!(!is_expected(peer, 11, 1000));
        assert!(!is_expected(peer, 10, 0));
        assert!(!is_expected(peer, 11, 0));
    }
}
