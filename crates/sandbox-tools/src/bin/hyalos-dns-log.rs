//! `hyalos-dns-log <program> [<args>...]`: for the image test only, never in the shipped image.
//! Answers DNS on `127.0.0.1:53`, over UDP and TCP, with "no such name", runs the program, then prints every name that
//! was asked as one `hyalos-dns-log: asked <name>` line each on standard error, and exits as the
//! program did.

use std::net::{TcpListener, UdpSocket};
use std::process::ExitCode;

use hyalos_sandbox_tools::{DnsLog, run};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(program) = args.first().cloned() else {
        eprintln!("usage: hyalos-dns-log <program> [<args>...]");
        return ExitCode::from(2);
    };
    let addr = std::env::var("HYALOS_DNS_LOG").unwrap_or_else(|_| "127.0.0.1:53".into());
    let socket = match UdpSocket::bind(&addr) {
        Ok(socket) => socket,
        Err(e) => {
            eprintln!("hyalos-dns-log: cannot listen on {addr}: {e}");
            return ExitCode::from(1);
        }
    };
    let tcp = match TcpListener::bind(&addr) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("hyalos-dns-log: cannot listen on {addr} over TCP: {e}");
            return ExitCode::from(1);
        }
    };
    let log = DnsLog::default();
    let (udp_log, tcp_log) = (log.clone(), log.clone());
    let udp = std::thread::Builder::new().spawn(move || udp_log.serve(socket));
    let tcp = std::thread::Builder::new().spawn(move || tcp_log.serve_tcp(tcp));
    if udp.is_err() || tcp.is_err() {
        return ExitCode::from(1);
    }
    let code = run(&program, &args[1..]);
    // A query sent just before the program ended may still be on its way.
    std::thread::sleep(std::time::Duration::from_millis(200));
    // On a line of its own, whatever the program wrote last.
    eprintln!();
    for name in log.names() {
        eprintln!("hyalos-dns-log: asked {name}");
    }
    eprintln!("hyalos-dns-log: done");
    match code {
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(e) => {
            eprintln!("hyalos-dns-log: cannot run {program}: {e}");
            ExitCode::from(127)
        }
    }
}
