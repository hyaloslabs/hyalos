//! The egress gate, through a running broker and a job's socket, on generated manifests and proxy
//! requests (S8.3, ADR-0009, ADR-0042, ADR-0051, ADR-0109).
//!
//! Each case starts a job whose manifest lists a few generated hosts, and sends one request the
//! generator draws together with what the gate must do with it: a listed host in any spelling a
//! resolver reads the same, on its one port, passes; an unlisted host, a name under a listed one, a
//! provider's host, an address in any of its spellings, another port or a target that is not one is
//! refused. Every name resolves to one server, which counts its connections, so a refused request
//! that connected anyway is caught.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hyalos_broker::Broker;
use hyalos_broker::egress::{Egress, Resolve, inputs_digest};
use hyalos_broker::model::Meter;
use hyalos_broker::sandbox::{Models, Sandboxes};
use hyalos_props::{SeededRng, forall, one_in, pick, seeds, text, within};
use hyalos_types::wire::{ToBroker, ToEngine};
use hyalos_types::{
    Answer, Budget, Clock, Digest, Gate, JobId, MANIFEST_VERSION, Manifest, ManifestKey, Role,
    Timestamp,
};

type Res<T = ()> = Result<T, String>;

const LOWER: &[char] = &[
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's',
    't', 'u', 'v', 'w', 'x', 'y', 'z', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '-',
];

struct Shared(AtomicU64);

impl Clock for Shared {
    fn now(&self) -> Timestamp {
        Timestamp::from_millis(self.0.load(Ordering::SeqCst))
    }
}

/// Every name resolves to the one server: whatever the gate lets through, the server sees.
struct Everything(SocketAddr);

impl Resolve for Everything {
    fn resolve(&self, _: &str, _: u16) -> std::io::Result<Vec<SocketAddr>> {
        Ok(vec![self.0])
    }
}

/// A server that counts connections and answers each: a plain-HTTP request with a short page, a
/// tunnel by closing it. It keeps each request's head.
struct Server {
    addr: SocketAddr,
    connections: Arc<AtomicUsize>,
    heads: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
}

impl Server {
    fn start() -> Res<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let addr = listener.local_addr().map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let connections = Arc::new(AtomicUsize::new(0));
        let heads = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (c, h, s) = (connections.clone(), heads.clone(), stop.clone());
        std::thread::spawn(move || {
            while !s.load(Ordering::SeqCst) {
                let Ok((stream, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                };
                c.fetch_add(1, Ordering::SeqCst);
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
                let mut reader = BufReader::new(&stream);
                let mut head = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    head.push_str(&line);
                }
                if !head.is_empty() {
                    let _ = (&stream).write_all(
                        b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok",
                    );
                }
                if let Ok(mut heads) = h.lock() {
                    heads.push(head);
                }
            }
        });
        Ok(Self {
            addr,
            connections,
            heads,
            stop,
        })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// What the gate must do with a request.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Due {
    /// Pass it to `host` (as compared with the manifest) on `port`; a plain-HTTP one asks for `path`.
    Pass {
        host: String,
        port: u16,
        path: Option<String>,
    },
    /// Refuse it with this code; `about` is the `host:port` the report's digest is of, when the gate
    /// read one.
    Refuse {
        code: &'static str,
        about: Option<String>,
    },
}

/// A request line's method and target, and what is due.
#[derive(Debug)]
struct Case {
    method: &'static str,
    target: String,
    due: Due,
}

fn label(rng: &mut SeededRng) -> String {
    // A letter first and last, so no label starts or ends with '-'.
    format!(
        "{}{}{}",
        pick(rng, &LOWER[..26]),
        text(rng, LOWER, 8),
        pick(rng, &LOWER[..26])
    )
}

fn name(rng: &mut SeededRng) -> String {
    let labels: Vec<String> = (0..within(rng, 1..3)).map(|_| label(rng)).collect();
    format!(
        "{}.{}",
        labels.join("."),
        pick(rng, &["org", "dev", "example", "io"])
    )
}

/// `host` as someone might write it and a resolver read the same: some letters upper case, and
/// perhaps the trailing dot of an absolute name.
fn respelled(rng: &mut SeededRng, host: &str) -> String {
    let mut out: String = host
        .chars()
        .map(|c| {
            if one_in(rng, 3) {
                c.to_ascii_uppercase()
            } else {
                c
            }
        })
        .collect();
    if one_in(rng, 4) {
        out.push('.');
    }
    out
}

fn path(rng: &mut SeededRng) -> String {
    let mut p = format!("/{}", text(rng, LOWER, 10));
    if one_in(rng, 3) {
        p.push_str(&format!("?{}={}", label(rng), label(rng)));
    }
    p
}

/// A request to `host` (as written) on `port`, CONNECT or plain HTTP.
fn request(
    rng: &mut SeededRng,
    host: &str,
    port: Option<u16>,
) -> (&'static str, String, Option<String>) {
    if one_in(rng, 2) {
        let port = port.unwrap_or(443);
        ("CONNECT", format!("{host}:{port}"), None)
    } else {
        let p = path(rng);
        let target = match port {
            Some(port) => format!("http://{host}:{port}{p}"),
            None if one_in(rng, 2) => format!("http://{host}:80{p}"),
            None => format!("http://{host}{p}"),
        };
        (*pick(rng, &["GET", "POST", "PUT"]), target, Some(p))
    }
}

/// The manifest's hosts, the provider gateway the broker refuses, and the case to send.
fn draw(rng: &mut SeededRng) -> (BTreeSet<String>, String, Case) {
    let mut listed: BTreeSet<String> = (0..within(rng, 1..4)).map(|_| name(rng)).collect();
    let gateway = format!("gw-{}.example", label(rng));
    // A provider's host is refused even when the manifest lists it.
    if one_in(rng, 2) {
        listed.insert(gateway.clone());
    }
    if one_in(rng, 3) {
        listed.insert("api.anthropic.com".into());
    }
    let mine: Vec<String> = listed
        .iter()
        .filter(|h| *h != &gateway && !h.starts_with("api."))
        .cloned()
        .collect();
    let host = pick(rng, &mine).clone();
    let refuse = |code, about: Option<String>| Due::Refuse { code, about };

    let case = match rng.below(8) {
        // A listed host, on its port: it passes.
        0..=2 => {
            let written = respelled(rng, &host);
            let (method, target, p) = request(rng, &written, None);
            let port = if method == "CONNECT" { 443 } else { 80 };
            Case {
                method,
                target,
                due: Due::Pass {
                    host,
                    port,
                    path: p,
                },
            }
        }
        // A host the manifest does not list, or a name under one it does.
        3 => {
            let other = if one_in(rng, 2) {
                format!("{}.{host}", label(rng))
            } else {
                loop {
                    let n = name(rng);
                    if !listed.contains(&n) {
                        break n;
                    }
                }
            };
            let written = respelled(rng, &other);
            let (method, target, _) = request(rng, &written, None);
            let port = if method == "CONNECT" { 443 } else { 80 };
            Case {
                method,
                target,
                due: refuse("egress.host_not_allowed", Some(format!("{other}:{port}"))),
            }
        }
        // A provider's host, or a name under it.
        4 => {
            let provider = match rng.below(3) {
                0 => gateway.clone(),
                1 => format!("{}.{gateway}", label(rng)),
                _ => pick(rng, &["api.anthropic.com", "api.openai.com"]).to_string(),
            };
            let written = respelled(rng, &provider);
            let (method, target, _) = request(rng, &written, None);
            let port = if method == "CONNECT" { 443 } else { 80 };
            Case {
                method,
                target,
                due: refuse("egress.provider_host", Some(format!("{provider}:{port}"))),
            }
        }
        // An address, in a spelling a resolver reads as one.
        5 => {
            let (a, b, c, d) = (
                rng.below(256),
                rng.below(256),
                rng.below(256),
                rng.below(256),
            );
            let written = match rng.below(4) {
                0 => format!("{a}.{b}.{c}.{d}"),
                1 => format!("{a}.{}", rng.below(1 << 24)),
                2 => format!("0x{:08x}", (a << 24) | (b << 16) | (c << 8) | d),
                _ => format!("{}", (a << 24) | (b << 16) | (c << 8) | d),
            };
            let (method, target, _) = request(rng, &written, None);
            Case {
                method,
                target,
                due: refuse("egress.ip_literal", None),
            }
        }
        // A listed host on another port.
        6 => {
            let connect = one_in(rng, 2);
            let right = if connect { 443 } else { 80 };
            let port = loop {
                let p = *pick(rng, &[22, 80, 443, 8080, 8443]) as u64;
                let p = if one_in(rng, 3) {
                    within(rng, 1..65_536)
                } else {
                    p
                } as u16;
                if p != right {
                    break p;
                }
            };
            let (method, target) = if connect {
                ("CONNECT", format!("{}:{port}", respelled(rng, &host)))
            } else {
                (
                    "GET",
                    format!("http://{}:{port}{}", respelled(rng, &host), path(rng)),
                )
            };
            Case {
                method,
                target,
                due: refuse("egress.port_not_allowed", None),
            }
        }
        // A target that is not a host and port.
        _ => {
            let target = match rng.below(6) {
                0 => ("CONNECT", host.clone()),
                1 => ("GET", format!("https://{host}/")),
                2 => ("GET", format!("http://{}@{host}/", label(rng))),
                3 => ("CONNECT", format!("{}_{host}:443", label(rng))),
                4 => ("CONNECT", format!("{}..{host}:443", label(rng))),
                _ => (
                    "CONNECT",
                    format!("{host}:{}", within(rng, 65_536..1_000_000)),
                ),
            };
            Case {
                method: target.0,
                target: target.1,
                due: refuse("egress.bad_target", None),
            }
        }
    };
    (listed, gateway, case)
}

fn sandboxes(egress: Egress, clock: Arc<Shared>) -> Sandboxes {
    Sandboxes {
        models: Arc::new(Models::new(
            Err("model.no_provider"),
            Err("model.no_provider"),
            Arc::new(Meter::default()),
        )),
        egress,
        git: hyalos_broker::git::Git::github(),
        forge: hyalos_broker::forge::Forge::github(),
        clock,
    }
}

fn socket_path(seed: u64) -> Res<PathBuf> {
    let dir = std::env::temp_dir().join(format!("hybc-{}-{seed}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("job.sock"))
}

/// Sends a request head and reads the answer until the broker closes.
fn ask(socket: &Path, head: &str) -> Res<(u16, String)> {
    let mut stream = UnixStream::connect(socket).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;
    stream
        .write_all(head.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut answer = String::new();
    stream
        .read_to_string(&mut answer)
        .map_err(|e| e.to_string())?;
    let status = answer
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("no status in {answer:?}"))?;
    Ok((status, answer))
}

fn denials(reports: &Receiver<ToEngine>) -> Vec<(String, Digest)> {
    std::thread::sleep(Duration::from_millis(20));
    reports
        .try_iter()
        .map(|m| match m {
            ToEngine::Gate {
                gate: Gate::Egress,
                answer: Answer::Deny,
                reason,
                inputs,
                ..
            } => (reason.as_str().to_string(), inputs),
            other => (format!("unexpected {other:?}"), inputs_digest("")),
        })
        .collect()
}

#[test]
fn a_request_reaches_its_host_exactly_when_the_manifest_allows_it_and_is_else_refused_and_reported()
-> Res {
    let key = ManifestKey::from_seed([5; 32]);
    let mut outcomes: BTreeSet<String> = BTreeSet::new();
    let ran = forall("broker.egress", seeds(200)?, |rng| {
        let (listed, gateway, case) = draw(rng);
        let server = Server::start()?;
        let egress = Egress::with_resolver(Arc::new(Everything(server.addr)))
            .refusing_bases([format!("https://{gateway}").as_str()]);
        let clock = Arc::new(Shared(AtomicU64::new(100)));
        let (to, reports) = mpsc::channel();
        let mut broker =
            Broker::with_sandboxes(key.public(), sandboxes(egress, clock.clone())).report_to(to);
        let job = JobId::from_seq(within(rng, 1..10_000));
        let manifest = Manifest {
            version: MANIFEST_VERSION,
            job,
            role: Role::Actor,
            tainted: one_in(rng, 2),
            repo: "hyaloslabs/hyalos".into(),
            branch_prefix: "hyalos/".into(),
            pr_base: None,
            actions: BTreeSet::new(),
            egress_hosts: listed.clone(),
            tools: BTreeSet::new(),
            data_flows: hyalos_types::flow::ALL.map(String::from).into(),
            budget: Budget { tokens: 1_000 },
            deadline: Timestamp::from_millis(1_000_000),
            approval: None,
            handles: Default::default(),
            result: None,
            closes: None,
        };
        let socket = socket_path(rng.seed())?;
        let answers = broker
            .handle(
                ToBroker::StartJob {
                    job,
                    role: Role::Actor,
                    manifest: key
                        .sign(&manifest)
                        .and_then(|s| s.to_bytes())
                        .map_err(|e| e.to_string())?,
                    socket: socket.to_string_lossy().into_owned(),
                    forge_token: None,
                },
                clock.as_ref(),
            )
            .map_err(|e| e.to_string())?
            .ok_or("the broker stopped")?;
        if answers != vec![ToEngine::Admitted { job }] {
            return Err(format!("the job was not admitted: {answers:?}"));
        }

        let head = format!(
            "{} {} HTTP/1.1\r\nhost: x\r\n\r\n",
            case.method, case.target
        );
        let (status, answer) = ask(&socket, &head)?;
        let denied = denials(&reports);
        let connections = server.connections.load(Ordering::SeqCst);
        let what = format!("{case:?} with {listed:?}");
        match &case.due {
            Due::Pass { host, port, path } => {
                if status != 200 || connections != 1 || !denied.is_empty() {
                    return Err(format!(
                        "{what}: due to pass, got {status}, {connections} connections and {denied:?}"
                    ));
                }
                if let Some(path) = path {
                    let heads = server.heads.lock().map_err(|e| e.to_string())?.clone();
                    let first = heads.first().cloned().unwrap_or_default();
                    let line = format!("{} {path} HTTP/1.1\r\n", case.method);
                    if !first.starts_with(&line) || !first.contains(&format!("host: {host}\r\n")) {
                        return Err(format!("{what}: the host was asked {first:?}"));
                    }
                }
                outcomes.insert(format!("pass {port}"));
            }
            Due::Refuse { code, about } => {
                if status != 403 || !answer.contains(code) || connections != 0 {
                    return Err(format!(
                        "{what}: due to be refused {code}, got {status} {answer:?} and {connections} connections"
                    ));
                }
                let reported: Vec<&str> = denied.iter().map(|(c, _)| c.as_str()).collect();
                if reported != [*code] {
                    return Err(format!("{what}: reported {reported:?}, expected [{code}]"));
                }
                if let Some(about) = about
                    && denied[0].1 != inputs_digest(about)
                {
                    return Err(format!("{what}: the report is not of {about}"));
                }
                outcomes.insert(code.to_string());
            }
        }
        let _ = std::fs::remove_dir_all(socket.parent().unwrap_or(&socket));
        Ok(())
    })?;
    if ran >= 200 {
        assert_eq!(
            outcomes,
            [
                "egress.bad_target",
                "egress.host_not_allowed",
                "egress.ip_literal",
                "egress.port_not_allowed",
                "egress.provider_host",
                "pass 443",
                "pass 80",
            ]
            .map(String::from)
            .into(),
        );
    }
    Ok(())
}
