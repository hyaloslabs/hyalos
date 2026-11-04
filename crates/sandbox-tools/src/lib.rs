//! The programs inside the sandbox image (ADR-0016, ADR-0040).
//!
//! * `hyalos-forward` listens on a loopback port and relays each connection to the broker's socket,
//!   which is the sandbox's only way out. It prepares the harness's home directories, then runs the
//!   harness and exits with its status.
//! * `hyalos-submit` posts a job's typed result through the forwarder to the broker.
//! * `hyalos-dns-log` is for the image test only and is not in the shipped image: it answers DNS on
//!   loopback, records every name asked, and runs a program.
//!
//! None of these is trusted: they run as the sandbox's user, next to the harness. The broker checks
//! everything that arrives through them.

use std::fs;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, UdpSocket};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Where the forwarder listens, and what the image's base URLs and proxy variables name.
pub const FORWARDER: &str = "127.0.0.1:8089";
/// Where every substrate mounts the broker's socket (ADR-0032).
pub const BROKER_SOCKET: &str = "/run/broker.sock";
/// Connections open at once; the broker has its own, lower, per-job limit.
pub const MAX_CONNECTIONS: usize = 64;
/// The Codex settings the image ships, copied into `$CODEX_HOME` before the harness starts.
pub const CODEX_SEED: &str = "/etc/hyalos/codex";
/// A submitted result larger than this is refused before it is sent.
pub const MAX_SUBMISSION: usize = 1024 * 1024;

/// Relays connections on `listener` to the Unix socket at `socket`, each on its own threads, until
/// the process ends.
pub fn forward(listener: TcpListener, socket: PathBuf) {
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let Ok(tcp) = stream else {
            thread::sleep(Duration::from_millis(20));
            continue;
        };
        if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            active.fetch_sub(1, Ordering::SeqCst);
            continue;
        }
        let (active, socket) = (Arc::clone(&active), socket.clone());
        let spawned = thread::Builder::new().spawn(move || {
            let _ = relay(tcp, &socket);
            active.fetch_sub(1, Ordering::SeqCst);
        });
        if spawned.is_err() {
            // The closure, and the connection with it, is gone; give the slot back.
            continue;
        }
    }
}

/// Copies bytes both ways between `tcp` and a new connection to `socket` until either side ends.
pub fn relay(tcp: TcpStream, socket: &Path) -> io::Result<()> {
    let unix = UnixStream::connect(socket)?;
    let (mut tcp_in, mut unix_out) = (tcp.try_clone()?, unix.try_clone()?);
    let up = thread::Builder::new().spawn(move || {
        let _ = io::copy(&mut tcp_in, &mut unix_out);
        let _ = unix_out.shutdown(Shutdown::Write);
    })?;
    let (mut unix_in, mut tcp_out) = (unix, tcp);
    let _ = io::copy(&mut unix_in, &mut tcp_out);
    // The broker answered and closed: end the harness's side too, which also ends the copy above.
    let _ = tcp_out.shutdown(Shutdown::Both);
    let _ = unix_in.shutdown(Shutdown::Both);
    let _ = up.join();
    Ok(())
}

/// Makes the directories the image's environment names, which live on the sandbox's empty `/tmp`:
/// `$HOME`, and `$CODEX_HOME` with the shipped Codex settings in it (Codex needs it to exist at
/// start, ADR-0016).
pub fn prepare(
    home: Option<&Path>,
    codex_home: Option<&Path>,
    codex_seed: &Path,
) -> io::Result<()> {
    let make = |dir: &Path| {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    };
    if let Some(home) = home {
        make(home)?;
    }
    if let Some(codex) = codex_home {
        make(codex)?;
        if codex_seed.is_dir() {
            for entry in fs::read_dir(codex_seed)? {
                let path = entry?.path();
                if let (true, Some(name)) = (path.is_file(), path.file_name()) {
                    fs::copy(&path, codex.join(name))?;
                }
            }
        }
    }
    Ok(())
}

/// Runs `program` with `args` and the inherited environment and returns the exit code it ended with,
/// 128 plus the signal if one ended it.
pub fn run(program: &str, args: &[String]) -> io::Result<i32> {
    let status = Command::new(program).args(args).status()?;
    Ok(code(status))
}

fn code(status: ExitStatus) -> i32 {
    status
        .code()
        .or_else(|| status.signal().map(|s| 128 + s))
        .unwrap_or(1)
}

/// Posts `body` to the broker's submit endpoint through the forwarder at `forwarder`, and returns the
/// status and the body of the answer.
pub fn submit(forwarder: &str, body: &[u8]) -> io::Result<(u16, Vec<u8>)> {
    if body.len() > MAX_SUBMISSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("a result is at most {MAX_SUBMISSION} bytes"),
        ));
    }
    let mut stream = TcpStream::connect(forwarder)?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    write!(
        stream,
        "POST /hyalos/submit HTTP/1.1\r\nhost: localhost\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    let mut answer = Vec::new();
    stream
        .take(MAX_SUBMISSION as u64)
        .read_to_end(&mut answer)?;
    let bad = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "the broker's answer is not HTTP",
        )
    };
    let split = answer
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(bad)?;
    let head = String::from_utf8_lossy(&answer[..split]);
    let status = head
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(bad)?;
    Ok((status, answer[split + 4..].to_vec()))
}

/// A DNS server on `socket` that answers every question "no such name" and records the name.
#[derive(Debug, Clone, Default)]
pub struct DnsLog {
    names: Arc<Mutex<Vec<String>>>,
}

impl DnsLog {
    /// Answers on `socket` until the process ends.
    pub fn serve(&self, socket: UdpSocket) {
        let mut buf = [0u8; 512];
        loop {
            let Ok((n, from)) = socket.recv_from(&mut buf) else {
                continue;
            };
            let Some((name, end)) = question(&buf[..n]) else {
                continue;
            };
            if let Ok(mut names) = self.names.lock() {
                names.push(name);
            }
            let mut answer = Vec::with_capacity(end);
            answer.extend_from_slice(&buf[..2]); // the id
            // A response, recursion desired and available, "no such name"; one question, no answers.
            answer.extend_from_slice(&[0x81, 0x83, 0, 1, 0, 0, 0, 0, 0, 0]);
            answer.extend_from_slice(&buf[12..end]);
            let _ = socket.send_to(&answer, from);
        }
    }

    /// Every name asked so far, in order.
    pub fn names(&self) -> Vec<String> {
        self.names.lock().map(|n| n.clone()).unwrap_or_default()
    }
}

/// The name in a DNS query's first question, and where the question ends.
pub fn question(packet: &[u8]) -> Option<(String, usize)> {
    if packet.len() < 12 || packet[2] & 0x80 != 0 {
        return None;
    }
    let mut at = 12;
    let mut labels = Vec::new();
    loop {
        let len = usize::from(*packet.get(at)?);
        at += 1;
        if len == 0 {
            break;
        }
        if len > 63 {
            return None;
        }
        labels.push(String::from_utf8_lossy(packet.get(at..at + len)?).into_owned());
        at += len;
    }
    // The type and class follow the name.
    packet.get(at..at + 4)?;
    Some((labels.join("."), at + 4))
}
