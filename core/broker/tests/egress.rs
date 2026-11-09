//! The egress gate and the allowlisting proxy (S3.5, ADR-0042): a CONNECT or plain-HTTP request to a
//! manifest host on its port, resolving to a public address, passes; anything else is refused with
//! 403 and reported once to the engine as the egress gate's `deny`.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hyalos_broker::egress::{
    Egress, MAX_REPORTS, Resolve, destination, host_name, inputs_digest, is_public,
};
use hyalos_broker::model::Meter;
use hyalos_broker::sandbox::{Models, Sandboxes};
use hyalos_broker::{Broker, serve_with};
use hyalos_types::wire::{ToBroker, ToEngine, WIRE_VERSION, read_frame, write_frame};
use hyalos_types::{
    Answer, Budget, Clock, Gate, JobId, MANIFEST_VERSION, Manifest, ManifestKey, Role, Timestamp,
};

type Res<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

struct Shared(AtomicU64);

impl Clock for Shared {
    fn now(&self) -> Timestamp {
        Timestamp::from_millis(self.0.load(Ordering::SeqCst))
    }
}

/// Every name it knows resolves to one loopback port, as a test's servers are on loopback.
struct Fixed(BTreeMap<String, SocketAddr>);

impl Resolve for Fixed {
    fn resolve(&self, host: &str, _: u16) -> std::io::Result<Vec<SocketAddr>> {
        self.0
            .get(host)
            .map(|a| vec![*a])
            .ok_or_else(|| std::io::ErrorKind::NotFound.into())
    }
}

fn key() -> ManifestKey {
    ManifestKey::from_seed([3; 32])
}

fn manifest(job: u64, hosts: &[&str]) -> Manifest {
    Manifest {
        version: MANIFEST_VERSION,
        job: JobId::from_seq(job),
        role: Role::Actor,
        tainted: false,
        repo: "hyaloslabs/hyalos".into(),
        branch_prefix: "hyalos/".into(),
        pr_base: None,
        actions: BTreeSet::new(),
        egress_hosts: hosts.iter().map(|h| h.to_string()).collect(),
        tools: BTreeSet::new(),
        data_flows: hyalos_types::flow::ALL.map(String::from).into(),
        budget: Budget { tokens: 1_000 },
        deadline: Timestamp::from_millis(10_000),
        approval: None,
        handles: Default::default(),
        result: None,
        closes: None,
    }
}

fn job_dir(name: &str) -> Res<PathBuf> {
    let dir = std::env::temp_dir().join(format!("hye-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
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

struct Rig {
    broker: Broker,
    clock: Arc<Shared>,
    reports: Receiver<ToEngine>,
}

fn rig(egress: Egress) -> Rig {
    let clock = Arc::new(Shared(AtomicU64::new(100)));
    let (to, reports) = mpsc::channel();
    let broker =
        Broker::with_sandboxes(key().public(), sandboxes(egress, clock.clone())).report_to(to);
    Rig {
        broker,
        clock,
        reports,
    }
}

fn start(r: &mut Rig, m: &Manifest, socket: &Path) -> Res {
    let answers = r
        .broker
        .handle(
            ToBroker::StartJob {
                job: m.job,
                role: m.role,
                manifest: key().sign(m)?.to_bytes()?,
                socket: socket.to_string_lossy().into_owned(),
                forge_token: None,
            },
            r.clock.as_ref(),
        )?
        .ok_or("stopped")?;
    assert_eq!(answers, vec![ToEngine::Admitted { job: m.job }]);
    Ok(())
}

/// Sends a request head and reads the status line and whatever follows until the broker closes.
fn ask(socket: &Path, head: &str) -> Res<(u16, String)> {
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.write_all(head.as_bytes())?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer)?;
    let status = answer.split(' ').nth(1).ok_or("no status")?.parse()?;
    Ok((status, answer))
}

fn denials(reports: &Receiver<ToEngine>) -> Vec<(String, hyalos_types::Digest)> {
    std::thread::sleep(Duration::from_millis(50));
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

/// A server that echoes on one connection, for a tunnel.
fn echo() -> Res<SocketAddr> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    std::thread::spawn(move || {
        if let Ok((mut s, _)) = listener.accept() {
            let mut buf = [0u8; 64];
            while let Ok(n) = s.read(&mut buf) {
                if n == 0 || s.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
        }
    });
    Ok(addr)
}

/// A web server that answers one request and keeps its head.
fn web() -> Res<(SocketAddr, Arc<Mutex<String>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let seen = Arc::new(Mutex::new(String::new()));
    let kept = seen.clone();
    std::thread::spawn(move || {
        let Ok((s, _)) = listener.accept() else {
            return;
        };
        let mut reader = BufReader::new(&s);
        let mut head = String::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                break;
            }
            head.push_str(&line);
        }
        let mut body = [0u8; 4];
        let _ = reader.read_exact(&mut body);
        head.push_str(&String::from_utf8_lossy(&body));
        if let Ok(mut k) = kept.lock() {
            *k = head;
        }
        let _ = (&s)
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 5\r\nconnection: close\r\n\r\nhello");
    });
    Ok((addr, seen))
}

#[test]
fn a_target_is_read_as_a_host_and_its_one_port_or_refused() {
    let to = |m: &str, t: &str| destination(m, t).map(|d| (d.host, d.port, d.path));
    assert_eq!(
        to("CONNECT", "API.GitHub.com.:443"),
        Ok(("api.github.com".into(), 443, None))
    );
    assert_eq!(
        to("GET", "http://example.org"),
        Ok(("example.org".into(), 80, Some("/".into())))
    );
    assert_eq!(
        to("POST", "http://example.org:80/a/b?c=d"),
        Ok(("example.org".into(), 80, Some("/a/b?c=d".into())))
    );
    assert_eq!(
        to("GET", "http://example.org?q"),
        Ok(("example.org".into(), 80, Some("/?q".into())))
    );
    for (method, target, code) in [
        ("CONNECT", "example.org", "egress.bad_target"),
        ("CONNECT", "example.org:22", "egress.port_not_allowed"),
        ("CONNECT", "example.org:80", "egress.port_not_allowed"),
        ("GET", "http://example.org:443/", "egress.port_not_allowed"),
        ("GET", "https://example.org/", "egress.bad_target"),
        ("GET", "http://user@example.org/", "egress.bad_target"),
        ("CONNECT", "1.1.1.1:443", "egress.ip_literal"),
        ("CONNECT", "[2606:4700::1111]:443", "egress.ip_literal"),
        ("CONNECT", "::1:443", "egress.ip_literal"),
        ("GET", "http://127.1/", "egress.ip_literal"),
        ("GET", "http://0x7f000001/", "egress.ip_literal"),
        ("CONNECT", "exa_mple.org:443", "egress.bad_target"),
        ("CONNECT", "-a.org:443", "egress.bad_target"),
        ("CONNECT", "a..org:443", "egress.bad_target"),
        ("CONNECT", ":443", "egress.bad_target"),
        ("CONNECT", "example.org:99999", "egress.bad_target"),
    ] {
        assert_eq!(
            destination(method, target).map(|d| d.host),
            Err(code),
            "{method} {target}"
        );
    }
    assert_eq!(host_name(&"a".repeat(64)), Err("egress.bad_target"));
}

#[test]
fn only_public_addresses_are_public() {
    for public in ["1.1.1.1", "8.8.8.8", "2606:4700::1111", "::ffff:1.1.1.1"] {
        assert!(
            is_public(public.parse::<IpAddr>().unwrap_or(IpAddr::from([0; 4]))),
            "{public}"
        );
    }
    for private in [
        "127.0.0.1",
        "10.1.2.3",
        "172.16.0.1",
        "192.168.1.1",
        "169.254.169.254",
        "100.64.0.1",
        "0.0.0.0",
        "0.1.2.3",
        "192.0.0.1",
        "198.18.0.1",
        "224.0.0.1",
        "240.0.0.1",
        "255.255.255.255",
        "192.0.2.1",
        "::1",
        "::",
        "fe80::1",
        "fc00::1",
        "fd12::1",
        "ff02::1",
        "2001:db8::1",
        "2002:7f00:1::1",
        "2001:0:1::1",
        "::ffff:127.0.0.1",
        "::ffff:169.254.169.254",
        "192.88.99.1",
        "3fff::1",
    ] {
        let ip: IpAddr = private.parse().unwrap_or(IpAddr::from([1, 1, 1, 1]));
        assert!(!is_public(ip), "{private}");
    }
}

#[test]
fn an_allowed_host_is_tunnelled_and_reached_over_plain_http() -> Res {
    let (echo, (web, seen)) = (echo()?, web()?);
    let names = BTreeMap::from([
        ("allowed.test".to_string(), echo),
        ("web.test".to_string(), web),
    ]);
    let mut r = rig(Egress::with_resolver(Arc::new(Fixed(names))));
    let dir = job_dir("pass")?;
    let socket = dir.join("broker.sock");
    start(&mut r, &manifest(1, &["Allowed.Test", "web.test"]), &socket)?;

    // A tunnel: the bytes sent with the CONNECT, as a TLS hello would be, arrive first.
    let mut stream = UnixStream::connect(&socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream
        .write_all(b"CONNECT allowed.test:443 HTTP/1.1\r\nhost: allowed.test:443\r\n\r\nearly")?;
    let established = b"HTTP/1.1 200 Connection established\r\n\r\n";
    let mut answer = vec![0u8; established.len() + 5];
    stream.read_exact(&mut answer)?;
    assert_eq!(&answer[..established.len()], established);
    assert_eq!(&answer[established.len()..], b"early");
    stream.write_all(b"later")?;
    let mut more = [0u8; 5];
    stream.read_exact(&mut more)?;
    assert_eq!(&more, b"later");
    drop(stream);

    // Plain HTTP: asked of the host in origin form, with proxy headers dropped.
    let (status, answer) = ask(
        &socket,
        "POST http://web.test/page?x=1 HTTP/1.1\r\nhost: web.test\r\nproxy-authorization: Basic eA==\r\nuser-agent: t\r\ncontent-length: 4\r\n\r\nbody",
    )?;
    assert_eq!(status, 200, "{answer}");
    assert!(answer.ends_with("hello"), "{answer}");
    let head = seen.lock().map(|s| s.clone()).unwrap_or_default();
    assert!(
        head.starts_with("POST /page?x=1 HTTP/1.1\r\nhost: web.test\r\n"),
        "{head}"
    );
    assert!(
        head.contains("user-agent: t\r\n") && head.ends_with("body"),
        "{head}"
    );
    assert!(!head.contains("proxy-authorization"), "{head}");
    assert!(denials(&r.reports).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn anything_else_is_refused_with_403_and_reported_once() -> Res {
    let echo = echo()?;
    let names = BTreeMap::from([("allowed.test".to_string(), echo)]);
    let mut r = rig(Egress::with_resolver(Arc::new(Fixed(names))));
    let dir = job_dir("deny")?;
    let socket = dir.join("broker.sock");
    start(
        &mut r,
        &manifest(2, &["allowed.test", "1.1.1.1", "API.Anthropic.com"]),
        &socket,
    )?;

    for (head, code, inputs) in [
        (
            "CONNECT other.test:443 HTTP/1.1\r\n\r\n",
            "egress.host_not_allowed",
            "other.test:443",
        ),
        (
            "GET http://other.test/ HTTP/1.1\r\n\r\n",
            "egress.host_not_allowed",
            "other.test:80",
        ),
        // Listed or not, a provider's API host is never reached through the proxy (ADR-0016).
        (
            "CONNECT api.anthropic.com:443 HTTP/1.1\r\n\r\n",
            "egress.provider_host",
            "api.anthropic.com:443",
        ),
        // Listed or not, an address is never a destination.
        (
            "CONNECT 1.1.1.1:443 HTTP/1.1\r\n\r\n",
            "egress.ip_literal",
            "1.1.1.1:443",
        ),
        (
            "CONNECT allowed.test:22 HTTP/1.1\r\n\r\n",
            "egress.port_not_allowed",
            "allowed.test:22",
        ),
    ] {
        let (status, answer) = ask(&socket, head)?;
        assert_eq!(status, 403, "{head}: {answer}");
        assert!(answer.contains(code), "{answer}");
        assert_eq!(
            denials(&r.reports),
            vec![(code.to_string(), inputs_digest(inputs))],
            "{head}"
        );
    }
    // A plain-HTTP request that could be read as another is refused before anything is sent.
    for head in [
        "GET\thttp://evil/ http://allowed.test/ HTTP/1.1\r\n\r\n",
        "get http://allowed.test/ HTTP/1.1\r\n\r\n",
        "GET http://allowed.test/a\rb HTTP/1.1\r\n\r\n",
        "GET http://allowed.test/ HTTP/1.1\r\nx: a\rhost: evil\r\n\r\n",
        "GET http://allowed.test/ HTTP/1.1\r\nx y: a\r\n\r\n",
        "GET http://allowed.test/ HTTP/1.1\r\nx: a\u{0}b\r\n\r\n",
    ] {
        let (status, answer) = ask(&socket, head)?;
        assert_eq!(status, 400, "{head:?}: {answer}");
        assert!(answer.contains("egress.bad_request"), "{answer}");
    }
    assert!(denials(&r.reports).is_empty());

    // Asked again, refused again, and not reported again.
    let (status, _) = ask(&socket, "CONNECT other.test:443 HTTP/1.1\r\n\r\n")?;
    assert_eq!(status, 403);
    assert!(denials(&r.reports).is_empty());

    // Past the deadline even an allowed host is refused.
    r.clock.0.store(20_000, Ordering::SeqCst);
    let (status, answer) = ask(&socket, "CONNECT allowed.test:443 HTTP/1.1\r\n\r\n")?;
    assert_eq!(status, 403);
    assert!(answer.contains("admission.expired"), "{answer}");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// A provider base's host and a provider's subdomain are refused even when the manifest lists them
/// and they resolve to a public address; a name beside them that is listed is reached (ADR-0051).
#[test]
fn provider_hosts_are_refused_even_when_listed() -> Res {
    let echo = echo()?;
    let names = [
        "llm.corp.test",
        "x.llm.corp.test",
        "eu.api.openai.com",
        "docs.anthropic.com",
    ]
    .into_iter()
    .map(|n| (n.to_string(), echo))
    .collect();
    let egress = Egress::with_resolver(Arc::new(Fixed(names)))
        .refusing_bases(["https://LLM.corp.test:8443", "http://127.0.0.1:9"]);
    let mut r = rig(egress);
    let dir = job_dir("prov")?;
    let socket = dir.join("broker.sock");
    let listed = [
        "llm.corp.test",
        "x.llm.corp.test",
        "eu.api.openai.com",
        "docs.anthropic.com",
    ];
    start(&mut r, &manifest(7, &listed), &socket)?;
    for host in &listed[..3] {
        let (status, answer) = ask(&socket, &format!("CONNECT {host}.:443 HTTP/1.1\r\n\r\n"))?;
        assert_eq!(status, 403, "{host}: {answer}");
        assert!(answer.contains("egress.provider_host"), "{answer}");
        assert_eq!(
            denials(&r.reports),
            vec![(
                "egress.provider_host".to_string(),
                inputs_digest(&format!("{host}:443"))
            )],
            "{host}"
        );
    }
    // Plain HTTP is refused the same way.
    let (status, answer) = ask(
        &socket,
        "GET http://LLM.corp.test/v1/messages HTTP/1.1\r\n\r\n",
    )?;
    assert_eq!(status, 403, "{answer}");
    assert!(answer.contains("egress.provider_host"), "{answer}");
    // A provider's other names, such as its documentation, are reached when listed.
    let mut stream = UnixStream::connect(&socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.write_all(b"CONNECT docs.anthropic.com:443 HTTP/1.1\r\n\r\n")?;
    let mut established = [0u8; 39];
    stream.read_exact(&mut established)?;
    assert_eq!(&established, b"HTTP/1.1 200 Connection established\r\n\r\n");
    drop(stream);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn an_allowed_name_that_resolves_to_a_private_address_is_refused() -> Res {
    let mut r = rig(Egress::public());
    let dir = job_dir("priv")?;
    let socket = dir.join("broker.sock");
    start(&mut r, &manifest(3, &["localhost"]), &socket)?;
    let (status, answer) = ask(&socket, "CONNECT localhost:443 HTTP/1.1\r\n\r\n")?;
    assert_eq!(status, 403, "{answer}");
    assert!(answer.contains("egress.not_public"), "{answer}");
    assert_eq!(
        denials(&r.reports),
        vec![(
            "egress.not_public".to_string(),
            inputs_digest("localhost:443")
        )]
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn reports_stop_after_a_limit_with_one_that_says_so() -> Res {
    let mut r = rig(Egress::with_resolver(Arc::new(Fixed(BTreeMap::new()))));
    let dir = job_dir("cap")?;
    let socket = dir.join("broker.sock");
    start(&mut r, &manifest(4, &[]), &socket)?;
    for n in 0..MAX_REPORTS + 10 {
        let (status, _) = ask(&socket, &format!("CONNECT h{n}.test:443 HTTP/1.1\r\n\r\n"))?;
        assert_eq!(status, 403);
    }
    let reasons: Vec<String> = denials(&r.reports).into_iter().map(|(r, _)| r).collect();
    assert_eq!(reasons.len(), MAX_REPORTS + 1);
    assert!(
        reasons[..MAX_REPORTS]
            .iter()
            .all(|r| r == "egress.host_not_allowed")
    );
    assert_eq!(reasons[MAX_REPORTS], "egress.too_many_refusals");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_tunnel_ends_when_its_job_does() -> Res {
    let echo = echo()?;
    let names = BTreeMap::from([("allowed.test".to_string(), echo)]);
    let mut r = rig(Egress::with_resolver(Arc::new(Fixed(names))));
    let dir = job_dir("end")?;
    let socket = dir.join("broker.sock");
    let m = manifest(5, &["allowed.test"]);
    start(&mut r, &m, &socket)?;
    let mut stream = UnixStream::connect(&socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.write_all(b"CONNECT allowed.test:443 HTTP/1.1\r\n\r\n")?;
    let mut established = [0u8; 39];
    stream.read_exact(&mut established)?;
    r.broker
        .handle(ToBroker::EndJob { job: m.job }, r.clock.as_ref())?;
    let mut rest = Vec::new();
    let began = std::time::Instant::now();
    let _ = stream.read_to_end(&mut rest);
    assert!(
        began.elapsed() < Duration::from_secs(5),
        "the tunnel outlived its job"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_refusal_reaches_the_engine_unasked_over_the_channel() -> Res {
    let (engine, broker_end) = UnixStream::pair()?;
    let clock = Arc::new(Shared(AtomicU64::new(100)));
    let shared = sandboxes(Egress::public(), clock.clone());
    let serving = std::thread::spawn(move || {
        let (mut input, mut output) = (&broker_end, &broker_end);
        serve_with(&mut input, &mut output, clock.as_ref(), Some(shared)).map_err(|e| e.to_string())
    });
    let mut to_broker = &engine;
    write_frame(
        &mut to_broker,
        &ToBroker::Hello {
            version: WIRE_VERSION,
            manifest_key: key().public(),
            forge: None,
        },
    )?;
    let mut from_broker = &engine;
    assert_eq!(
        read_frame::<_, ToEngine>(&mut from_broker)?,
        Some(ToEngine::Ready {
            version: WIRE_VERSION
        })
    );
    let dir = job_dir("wire")?;
    let socket = dir.join("broker.sock");
    let m = manifest(6, &[]);
    write_frame(
        &mut to_broker,
        &ToBroker::StartJob {
            job: m.job,
            role: m.role,
            manifest: key().sign(&m)?.to_bytes()?,
            socket: socket.to_string_lossy().into_owned(),
            forge_token: None,
        },
    )?;
    assert_eq!(
        read_frame::<_, ToEngine>(&mut from_broker)?,
        Some(ToEngine::Admitted { job: m.job })
    );
    let (status, _) = ask(&socket, "CONNECT elsewhere.test:443 HTTP/1.1\r\n\r\n")?;
    assert_eq!(status, 403);
    match read_frame::<_, ToEngine>(&mut from_broker)? {
        Some(ToEngine::Gate {
            job,
            gate: Gate::Egress,
            answer: Answer::Deny,
            inputs,
            reason,
        }) => {
            assert_eq!(job, m.job);
            assert_eq!(inputs, inputs_digest("elsewhere.test:443"));
            assert_eq!(reason.as_str(), "egress.host_not_allowed");
        }
        other => return Err(format!("expected the egress gate's deny, got {other:?}").into()),
    }
    write_frame(&mut to_broker, &ToBroker::Shutdown)?;
    let exit = serving.join().map_err(|_| "the broker panicked")??;
    assert_eq!(exit, hyalos_broker::Exit::Shutdown);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
