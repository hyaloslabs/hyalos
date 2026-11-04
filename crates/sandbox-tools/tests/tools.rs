//! The forwarder relays both ways and ends when the broker does; `prepare` makes the harness's
//! directories; `submit` posts and reads the answer; the DNS log answers and records (ADR-0040).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::thread;

use hyalos_sandbox_tools::{DnsLog, forward, prepare, question, submit};

type Res = Result<(), Box<dyn std::error::Error>>;

fn dir(name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let dir = std::env::temp_dir().join(format!("hyst-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// A broker stand-in on a Unix socket: reads one request head and a body, answers, closes.
fn broker(
    socket: &std::path::Path,
    answer: &'static str,
) -> Result<thread::JoinHandle<String>, Box<dyn std::error::Error>> {
    let listener = UnixListener::bind(socket)?;
    Ok(thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else {
            return String::new();
        };
        let mut reader = BufReader::new(&stream);
        let (mut seen, mut length) = (String::new(), 0usize);
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                break;
            }
            if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = v.trim().parse().unwrap_or(0);
            }
            seen.push_str(&line);
        }
        let mut body = vec![0; length];
        let _ = reader.read_exact(&mut body);
        seen.push_str(&String::from_utf8_lossy(&body));
        let _ = (&stream).write_all(answer.as_bytes());
        seen
    }))
}

#[test]
fn the_forwarder_relays_a_call_and_closes_when_the_broker_does() -> Res {
    let d = dir("fwd")?;
    let socket = d.join("b.sock");
    let seen = broker(
        &socket,
        "HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok",
    )?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let to = socket.clone();
    thread::spawn(move || forward(listener, to));

    let (status, body) = submit(&addr.to_string(), br#"{"summary":"done"}"#)?;
    assert_eq!((status, body), (200, b"ok".to_vec()));
    let seen = seen.join().map_err(|_| "broker panicked")?;
    assert!(
        seen.starts_with("POST /hyalos/submit HTTP/1.1\r\n"),
        "{seen}"
    );
    assert!(seen.ends_with(r#"{"summary":"done"}"#), "{seen}");

    // With nothing on the socket, the harness's connection is closed rather than left hanging.
    std::fs::remove_file(&socket)?;
    let mut stream = TcpStream::connect(addr)?;
    stream.write_all(b"POST / HTTP/1.1\r\n\r\n")?;
    let mut rest = Vec::new();
    match stream.read_to_end(&mut rest) {
        // Closed with the request unread, which the kernel reports as a reset.
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => {}
        other => assert_eq!((other?, rest.len()), (0, 0)),
    }
    let _ = std::fs::remove_dir_all(&d);
    Ok(())
}

#[test]
fn a_refused_or_oversized_result_is_reported() -> Res {
    let d = dir("sub")?;
    let socket = d.join("b.sock");
    let _seen = broker(
        &socket,
        "HTTP/1.1 404 Not Found\r\ncontent-length: 9\r\nconnection: close\r\n\r\nnot found",
    )?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?.to_string();
    thread::spawn(move || forward(listener, socket));
    assert_eq!(submit(&addr, b"{}")?.0, 404);
    assert!(submit(&addr, &vec![b' '; 2 * 1024 * 1024]).is_err());
    let _ = std::fs::remove_dir_all(&d);
    Ok(())
}

#[test]
fn the_harness_directories_are_made_and_codex_gets_its_settings() -> Res {
    let d = dir("prep")?;
    let seed = d.join("seed");
    std::fs::create_dir(&seed)?;
    std::fs::write(seed.join("config.toml"), "web_search = \"disabled\"\n")?;
    let (home, codex) = (d.join("tmp/home"), d.join("tmp/codex"));
    prepare(Some(&home), Some(&codex), &seed)?;
    assert!(home.is_dir());
    assert_eq!(
        std::fs::metadata(&codex)?.permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::read_to_string(codex.join("config.toml"))?,
        "web_search = \"disabled\"\n"
    );
    // Running again, with the directories there, is fine; nothing named is nothing made.
    prepare(Some(&home), Some(&codex), &seed)?;
    prepare(None, None, &seed)?;
    let _ = std::fs::remove_dir_all(&d);
    Ok(())
}

fn query(id: u16, name: &str) -> Vec<u8> {
    let mut q = id.to_be_bytes().to_vec();
    q.extend_from_slice(&[1, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in name.split('.') {
        q.push(u8::try_from(label.len()).unwrap_or(0));
        q.extend_from_slice(label.as_bytes());
    }
    q.extend_from_slice(&[0, 0, 1, 0, 1]);
    q
}

#[test]
fn the_dns_log_answers_no_such_name_and_records_each_question() -> Res {
    let server = UdpSocket::bind("127.0.0.1:0")?;
    let addr = server.local_addr()?;
    let log = DnsLog::default();
    let serving = log.clone();
    thread::spawn(move || serving.serve(server));

    let client = UdpSocket::bind("127.0.0.1:0")?;
    client.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    for (id, name) in [(7, "api.anthropic.com"), (8, "statsig.anthropic.com")] {
        client.send_to(&query(id, name), addr)?;
        let mut buf = [0u8; 512];
        let n = client.recv(&mut buf)?;
        assert_eq!(&buf[..2], &u16::to_be_bytes(id));
        assert_eq!(buf[3] & 0x0f, 3, "no such name");
        assert_eq!(
            question(&query(id, name)).map(|q| q.0),
            Some(name.to_string())
        );
        assert!(n > 12);
    }
    assert_eq!(log.names(), ["api.anthropic.com", "statsig.anthropic.com"]);
    assert_eq!(question(&[0; 5]), None);
    assert_eq!(
        question(&[0, 0, 0x81, 0x80, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1]),
        None,
        "an answer"
    );
    Ok(())
}
