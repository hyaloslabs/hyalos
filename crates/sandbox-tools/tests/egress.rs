//! S3.5: a harness in the sandbox reaches an allowed host through the broker's allowlisting proxy, and
//! an unlisted one is refused and reported (ADR-0042). Claude Code fetches with its web fetch tool and
//! Codex runs `curl` with its shell tool, each asked to by the scripted model.
//!
//! The allowed host is `allowed.hyalos.test`: an HTTPS server on this machine's loopback with a
//! certificate from a test authority the image's test target trusts. The broker in this process
//! resolves that one name to the server; anything else it would resolve publicly. Like the image
//! test, it skips without the image unless `HYALOS_REQUIRE_SANDBOX_IMAGE` is set.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};

use common::{
    Res, asked_name, image_present, job_with, not_model_calls, sandboxed_over, sandboxed_with,
    scripted_model,
};
use hyalos_broker::egress::{Egress, Resolve, inputs_digest};
use hyalos_types::wire::ToEngine;
use hyalos_types::{Answer, Gate};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};

const ALLOWED: &str = "allowed.hyalos.test";
const UNLISTED: &str = "elsewhere.hyalos.test";
/// What the allowed host's page says, which the harness must have read and passed to the model.
const PAGE: &str = "hyalos-allowed-page-7f3a";

struct OnlyAllowed(SocketAddr);

impl Resolve for OnlyAllowed {
    fn resolve(&self, host: &str, _: u16) -> std::io::Result<Vec<SocketAddr>> {
        if host == ALLOWED {
            Ok(vec![self.0])
        } else {
            Err(std::io::ErrorKind::NotFound.into())
        }
    }
}

/// The allowed host: answers every request with the page and records each request line.
fn allowed_host() -> Res<(SocketAddr, Arc<Mutex<Vec<String>>>)> {
    let config = Arc::new(
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()?
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from_pem_slice(include_bytes!(
                    "fixtures/tls/leaf.pem"
                ))?],
                PrivateKeyDer::from_pem_slice(include_bytes!("fixtures/tls/leaf.key"))?,
            )?,
    );
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&seen);
    std::thread::spawn(move || {
        for tcp in listener.incoming().flatten() {
            let (config, kept) = (Arc::clone(&config), Arc::clone(&kept));
            std::thread::spawn(move || {
                let Ok(connection) = ServerConnection::new(config) else {
                    return;
                };
                let mut tls = StreamOwned::new(connection, tcp);
                let mut reader = BufReader::new(&mut tls);
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    return;
                }
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" {
                        break;
                    }
                }
                if let Ok(mut k) = kept.lock() {
                    k.push(line.trim_end().to_string());
                }
                let page = format!("<html><body><p>{PAGE}</p></body></html>");
                let _ = write!(
                    tls,
                    "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{page}",
                    page.len()
                );
                tls.conn.send_close_notify();
                let _ = tls.flush();
            });
        }
    });
    Ok((addr, seen))
}

#[test]
fn a_harness_reaches_an_allowed_host_through_the_broker_and_an_unlisted_one_is_refused() -> Res {
    if !image_present()? {
        return Ok(());
    }
    let model = scripted_model()?;
    let (server, served) = allowed_host()?;
    let resolv = std::env::temp_dir().join(format!("hyegr-{}-resolv.conf", std::process::id()));
    std::fs::write(&resolv, "nameserver 127.0.0.1\n")?;
    let ca = ("NODE_EXTRA_CA_CERTS", "/opt/hyalos/harness/test/ca.pem");

    let harnesses: [(&str, &[&str]); 2] = [
        (
            "Claude Code web fetch",
            &[
                "claude",
                "-p",
                "fetch the page",
                "--tools",
                "WebFetch",
                "--allowedTools",
                "WebFetch",
            ],
        ),
        (
            "Codex shell",
            &["codex", "exec", "--skip-git-repo-check", "fetch the page"],
        ),
    ];
    let mut failures = Vec::new();
    let mut seq = 20u64;
    for (name, command) in harnesses {
        for host in [ALLOWED, UNLISTED] {
            seq += 1;
            let url = format!("https://{host}/page");
            if let Ok(mut fetch) = model.fetch.lock() {
                *fetch = Some(url.clone());
            }
            model.clear();
            let before = served.lock().map(|s| s.len()).unwrap_or(0);
            let job = job_with(
                model.port,
                seq,
                &[ALLOWED],
                Egress::with_resolver(Arc::new(OnlyAllowed(server))),
            )?;
            let (code, output) = sandboxed_with(&job.socket, &resolv, &[ca], command)?;
            let asked: Vec<&str> = output.lines().filter_map(asked_name).collect();
            let other = not_model_calls(&job);
            let reached = served.lock().map(|s| s.len()).unwrap_or(0) - before;
            let read_page = model
                .bodies
                .lock()
                .map(|b| b.iter().any(|b| b.to_string().contains(PAGE)))
                .unwrap_or(false);
            let reports: Vec<ToEngine> = job.reports.try_iter().collect();
            let said = format!(
                "{name} to {host}: exit {code}, lookups {asked:?}, other requests {other:?}, \
                 requests reaching the host {reached}, page read {read_page}, reports {reports:?}"
            );
            eprintln!("{said}");
            let connect = format!("CONNECT {host}:443 HTTP/1.1");
            // The harness's only other request is the CONNECT it was asked for: no lookup, and no
            // web fetch preflight to api.anthropic.com (skipWebFetchPreflight, ADR-0040).
            let mut ok = code == 0 && asked.is_empty() && other == vec![connect];
            if host == ALLOWED {
                ok &= reached >= 1 && read_page && reports.is_empty();
            } else {
                let refused = ToEngine::Gate {
                    job: hyalos_types::JobId::from_seq(seq),
                    gate: Gate::Egress,
                    inputs: inputs_digest(&format!("{UNLISTED}:443")),
                    answer: Answer::Deny,
                    reason: hyalos_types::ReasonCode::new("egress.host_not_allowed")
                        .ok_or("code")?,
                };
                ok &= reached == 0 && !read_page && reports == vec![refused];
            }
            if !ok {
                failures.push(format!("{said}\n{output}"));
            }
        }
    }

    // The control: with Claude Code's managed settings emptied, so `skipWebFetchPreflight` is not
    // set, the same fetch first asks api.anthropic.com about the host. The check above would see it.
    if let Ok(mut fetch) = model.fetch.lock() {
        *fetch = Some(format!("https://{ALLOWED}/page"));
    }
    let empty = std::env::temp_dir().join(format!("hyegr-{}-settings.json", std::process::id()));
    std::fs::write(&empty, "{}\n")?;
    let job = job_with(
        model.port,
        seq + 1,
        &[ALLOWED],
        Egress::with_resolver(Arc::new(OnlyAllowed(server))),
    )?;
    let (_, output) = sandboxed_over(
        &job.socket,
        &resolv,
        &[ca],
        &[(&empty, "/etc/claude-code/managed-settings.json")],
        harnesses[0].1,
    )?;
    let other = not_model_calls(&job);
    eprintln!("Claude Code web fetch without skipWebFetchPreflight: other requests {other:?}");
    if !other
        .iter()
        .any(|r| r.starts_with("CONNECT api.anthropic.com:"))
    {
        failures.push(format!(
            "the preflight control's call was not seen\n{output}"
        ));
    }
    let _ = std::fs::remove_file(&empty);
    let _ = std::fs::remove_file(&resolv);
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    Ok(())
}
