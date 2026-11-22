//! A stand-in for the model provider, for tests only. It never contacts the real provider.
//!
//! `standin <root file> <server name> [--present <name>] [--reply plain|chunked] [--status <code>]
//! [--quiet] [--drop]` makes a test root and a certificate, writes the root's DER bytes to the
//! file, listens on a local port, and prints `listening <address>`. It answers every request with
//! one fixed typed reply, and exits when its standard input closes, so it cannot outlive the test
//! that started it.
//!
//! `--present` puts another name on the certificate. `--reply` sends the body plain or chunked.
//! `--status` answers with that error status. `--quiet` reads the request and never answers.
//! `--drop` reads the request and closes the connection with no reply.

use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, KeyPair, KeyUsagePurpose};
use std::{io::Read, sync::Arc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        ServerConfig,
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    },
};

const REPLY: &str = r#"{"id":"msg_standin","type":"message","role":"assistant","model":"standin","content":[{"type":"text","text":"{\"label\":\"bug\"}"}],"stop_reason":"end_turn"}"#;

type Failure = Box<dyn std::error::Error + Send + Sync>;

/// How this stand-in behaves, from its command line.
#[derive(Clone, Default)]
struct Controls {
    present: Option<String>,
    chunked: bool,
    status: Option<u16>,
    quiet: bool,
    drop: bool,
}

fn controls(mut args: impl Iterator<Item = String>) -> Result<Controls, Failure> {
    let mut controls = Controls::default();
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--present" => controls.present = Some(args.next().ok_or("--present needs a name")?),
            "--reply" => controls.chunked = match args.next().as_deref() {
                Some("plain") => false,
                Some("chunked") => true,
                _ => return Err("--reply is plain or chunked".into()),
            },
            "--status" => controls.status = Some(args.next().ok_or("--status needs a code")?.parse()?),
            "--quiet" => controls.quiet = true,
            "--drop" => controls.drop = true,
            _ => return Err("unknown control".into()),
        }
    }
    Ok(controls)
}

fn acceptor(root_file: &str, name: &str) -> Result<TlsAcceptor, Failure> {
    let root_key = KeyPair::generate()?;
    let mut root = CertificateParams::new(Vec::<String>::new())?;
    root.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    root.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::DigitalSignature];
    root.distinguished_name.push(DnType::CommonName, "hyalos stand-in test root");
    let root = root.self_signed(&root_key)?;
    let key = KeyPair::generate()?;
    let mut leaf = CertificateParams::new(vec![name.to_string()])?;
    leaf.distinguished_name.push(DnType::CommonName, name);
    let leaf = leaf.signed_by(&key, &root, &root_key)?;
    std::fs::write(root_file, root.der())?;
    let config = ServerConfig::builder_with_provider(Arc::new(tokio_rustls::rustls::crypto::ring::default_provider()))
        .with_protocol_versions(&[&tokio_rustls::rustls::version::TLS12])?
        .with_no_client_auth()
        .with_single_cert(vec![CertificateDer::from(leaf.der().to_vec())], PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())))?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

/// Reads one request to the end of its body, then sends the fixed reply and closes.
async fn serve(acceptor: TlsAcceptor, socket: tokio::net::TcpStream, controls: Controls) -> Result<(), Failure> {
    let mut tls = acceptor.accept(socket).await?;
    let mut request = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = tls.read(&mut chunk).await?;
        if read == 0 {
            return Err("the request ended early".into());
        }
        request.extend_from_slice(&chunk[..read]);
        let Some(head) = request.windows(4).position(|w| w == b"\r\n\r\n") else { continue };
        let length = String::from_utf8_lossy(&request[..head])
            .lines()
            .find_map(|line| line.strip_prefix("content-length: ").and_then(|n| n.trim().parse::<usize>().ok()))
            .unwrap_or(0);
        if request.len() >= head + 4 + length {
            break;
        }
    }
    if controls.quiet {
        std::future::pending::<()>().await;
    }
    if controls.drop {
        return Ok(());
    }
    let status = controls.status.map_or("200 OK".to_string(), |code| format!("{code} Refused"));
    let reply = if controls.chunked {
        let (first, rest) = REPLY.split_at(REPLY.len() / 2);
        format!("HTTP/1.1 {status}\r\ncontent-type: application/json\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n{:x}\r\n{first}\r\n{:x}\r\n{rest}\r\n0\r\n\r\n", first.len(), rest.len())
    } else {
        format!("HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{REPLY}", REPLY.len())
    };
    tls.write_all(reply.as_bytes()).await?;
    tls.shutdown().await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Failure> {
    let args: Vec<String> = std::env::args().collect();
    let (Some(root_file), Some(name)) = (args.get(1), args.get(2)) else {
        return Err("usage: standin <root file> <server name>".into());
    };
    let controls = controls(args.iter().skip(3).cloned())?;
    let acceptor = acceptor(root_file, controls.present.as_deref().unwrap_or(name))?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    println!("listening {}", listener.local_addr()?);
    std::thread::spawn(|| {
        let _ = std::io::stdin().read_to_end(&mut Vec::new());
        std::process::exit(0);
    });
    loop {
        let (socket, _) = listener.accept().await?;
        let (acceptor, controls) = (acceptor.clone(), controls.clone());
        tokio::spawn(async move {
            let _ = serve(acceptor, socket, controls).await;
        });
    }
}
