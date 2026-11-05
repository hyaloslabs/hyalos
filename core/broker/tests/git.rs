//! The git proxy (S3.3, ADR-0046): the real `git` client clones and pushes through a job's socket to
//! a real git server (`git http-backend`) that accepts only the job's forge token. A push outside the
//! manifest's repository or branch prefix, a tag, a deletion or a push the actions do not allow is
//! refused before anything reaches the server, and reported as the manifest gate's `deny`.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hyalos_broker::Broker;
use hyalos_broker::egress::{Egress, inputs_digest};
use hyalos_broker::git::Git;
use hyalos_broker::model::Meter;
use hyalos_broker::sandbox::{Models, Sandboxes};
use hyalos_types::wire::{ToBroker, ToEngine};
use hyalos_types::{
    Answer, Budget, Clock, Digest, ForgeToken, Gate, JobId, MANIFEST_VERSION, Manifest,
    ManifestKey, Role, Timestamp,
};

type Res<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

const TOKEN: &str = "ghs_git_proxy_test_token";

struct Shared(AtomicU64);

impl Clock for Shared {
    fn now(&self) -> Timestamp {
        Timestamp::from_millis(self.0.load(Ordering::SeqCst))
    }
}

/// Whether `git` (and its `http-backend`) can be run here. CI always has it.
fn have_git() -> Res<bool> {
    let found = Command::new("git").arg("--version").output().is_ok();
    if !found && std::env::var_os("HYALOS_REQUIRE_GIT").is_some() {
        return Err("git is required (HYALOS_REQUIRE_GIT) and was not found".into());
    }
    if !found {
        eprintln!("skipping: no git on this host");
    }
    Ok(found)
}

fn dir(name: &str) -> Res<PathBuf> {
    let dir = std::env::temp_dir().join(format!("hyg-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Runs git with no configuration but this test's, no prompt and no proxy.
fn git(home: &Path, cwd: &Path, args: &[&str]) -> Res<Output> {
    Ok(Command::new("git")
        .args([
            "-c",
            "user.name=Hyalos Test",
            "-c",
            "user.email=test@hyalos.test",
            "-c",
            "init.defaultBranch=main",
            "-c",
            "protocol.version=2",
        ])
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()?)
}

fn ok(output: Output) -> Res<String> {
    if !output.status.success() {
        return Err(format!(
            "git failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = c
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            out.push(if i <= c.len() {
                char::from(T[(n >> (18 - 6 * i)) as usize & 63])
            } else {
                '='
            });
        }
    }
    out
}

/// One request the git server was sent.
#[derive(Debug, Clone)]
struct Seen {
    path: String,
    authorized: bool,
}

/// A git server on loopback: `git http-backend` over the bare repositories under `root`, answering
/// only requests that carry the job's token as GitHub expects it.
struct Server {
    port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Server {
    fn start(root: &Path) -> Res<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (root, log) = (root.to_path_buf(), Arc::clone(&seen));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let (root, log) = (root.clone(), Arc::clone(&log));
                std::thread::spawn(move || {
                    let _ = serve_one(stream, &root, &log);
                });
            }
        });
        Ok(Self { port, seen })
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

fn serve_one(stream: TcpStream, root: &Path, log: &Mutex<Vec<Seen>>) -> Res {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, target) = (
        parts.next().ok_or("no method")?.to_string(),
        parts.next().ok_or("no target")?.to_string(),
    );
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        reader.read_line(&mut h)?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((n, v)) = h.split_once(':') {
            headers.push((n.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    let get = |n: &str| headers.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
    let length: usize = get("content-length")
        .unwrap_or_default()
        .parse()
        .unwrap_or(0);
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    let expected = format!(
        "Basic {}",
        base64(format!("x-access-token:{TOKEN}").as_bytes())
    );
    let authorized = get("authorization").as_deref() == Some(expected.as_str());
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    if let Ok(mut log) = log.lock() {
        log.push(Seen {
            path: path.to_string(),
            authorized,
        });
    }
    let mut out = stream;
    if !authorized {
        out.write_all(b"HTTP/1.1 401 Unauthorized\r\nwww-authenticate: Basic realm=\"GitHub\"\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")?;
        return Ok(());
    }
    let mut cgi = Command::new("git")
        .arg("http-backend")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_PROJECT_ROOT", root)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("REQUEST_METHOD", &method)
        .env("PATH_INFO", path)
        .env("QUERY_STRING", query)
        .env("CONTENT_TYPE", get("content-type").unwrap_or_default())
        .env("CONTENT_LENGTH", length.to_string())
        .env("GIT_PROTOCOL", get("git-protocol").unwrap_or_default())
        .env(
            "HTTP_CONTENT_ENCODING",
            get("content-encoding").unwrap_or_default(),
        )
        .env("REMOTE_USER", "x-access-token")
        .env("REMOTE_ADDR", "127.0.0.1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdin = cgi.stdin.take().ok_or("no stdin")?;
    let writer = std::thread::spawn(move || stdin.write_all(&body));
    let mut response = Vec::new();
    cgi.stdout
        .take()
        .ok_or("no stdout")?
        .read_to_end(&mut response)?;
    let _ = writer.join();
    cgi.wait()?;
    // CGI headers, a blank line, the body.
    let split = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| (i, 4))
        .or_else(|| {
            response
                .windows(2)
                .position(|w| w == b"\n\n")
                .map(|i| (i, 2))
        })
        .ok_or("no CGI head")?;
    let head = String::from_utf8_lossy(&response[..split.0]).to_string();
    let body = &response[split.0 + split.1..];
    let mut status = "200 OK".to_string();
    let mut lines = String::new();
    for h in head.lines() {
        match h.split_once(':') {
            Some((n, v)) if n.eq_ignore_ascii_case("status") => status = v.trim().to_string(),
            Some(_) => lines.push_str(&format!("{h}\r\n")),
            None => {}
        }
    }
    out.write_all(
        format!(
            "HTTP/1.1 {status}\r\n{lines}content-length: {}\r\nconnection: close\r\n\r\n",
            body.len()
        )
        .as_bytes(),
    )?;
    out.write_all(body)?;
    Ok(())
}

/// Relays TCP on loopback to the job's socket, as the sandbox's forwarder does.
fn bridge(socket: &Path) -> Res<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let socket = socket.to_path_buf();
    std::thread::spawn(move || {
        for tcp in listener.incoming() {
            let Ok(tcp) = tcp else { continue };
            let Ok(unix) = UnixStream::connect(&socket) else {
                continue;
            };
            let (Ok(mut tcp_in), Ok(mut unix_out)) = (tcp.try_clone(), unix.try_clone()) else {
                continue;
            };
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut tcp_in, &mut unix_out);
                let _ = unix_out.shutdown(std::net::Shutdown::Write);
            });
            let (mut unix_in, mut tcp_out) = (unix, tcp);
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut unix_in, &mut tcp_out);
                let _ = tcp_out.shutdown(std::net::Shutdown::Both);
            });
        }
    });
    Ok(port)
}

fn key() -> ManifestKey {
    ManifestKey::from_seed([9; 32])
}

fn manifest(job: u64, actions: &[&str]) -> Manifest {
    Manifest {
        version: MANIFEST_VERSION,
        job: JobId::from_seq(job),
        role: Role::Actor,
        tainted: true,
        repo: "octo/repo".into(),
        branch_prefix: "hyalos/".into(),
        pr_base: None,
        actions: actions.iter().map(|a| a.to_string()).collect(),
        egress_hosts: BTreeSet::new(),
        tools: BTreeSet::new(),
        data_flows: BTreeSet::new(),
        budget: Budget { tokens: 1_000 },
        deadline: Timestamp::from_millis(10_000),
        approval: None,
    }
}

/// A git host with two bare repositories, `octo/repo` and `octo/other`, each with one commit on `main`.
struct World {
    home: PathBuf,
    root: PathBuf,
    server: Server,
    main: String,
}

fn world(name: &str) -> Res<World> {
    let home = dir(&format!("{name}-home"))?;
    let root = dir(&format!("{name}-root"))?;
    let seed = home.join("seed");
    std::fs::create_dir_all(&seed)?;
    ok(git(&home, &seed, &["init", "-q"])?)?;
    std::fs::write(seed.join("README"), "hello\n")?;
    ok(git(&home, &seed, &["add", "README"])?)?;
    ok(git(&home, &seed, &["commit", "-q", "-m", "first"])?)?;
    let main = ok(git(&home, &seed, &["rev-parse", "HEAD"])?)?;
    for repo in ["octo/repo.git", "octo/other.git"] {
        let bare = root.join(repo);
        let bare_text = bare.to_string_lossy().into_owned();
        ok(git(&home, &seed, &["init", "-q", "--bare", &bare_text])?)?;
        ok(git(&home, &seed, &["push", "-q", &bare_text, "main"])?)?;
    }
    Ok(World {
        server: Server::start(&root)?,
        home,
        root,
        main,
    })
}

impl World {
    fn rev(&self, repo: &str, refname: &str) -> Option<String> {
        let out = git(
            &self.home,
            &self.root.join(repo),
            &["rev-parse", "--verify", "-q", refname],
        )
        .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    fn calls(&self, path_end: &str) -> usize {
        self.server
            .seen()
            .iter()
            .filter(|s| s.path.ends_with(path_end) || s.path.contains(path_end))
            .count()
    }
}

struct Job {
    _broker: Broker,
    port: u16,
    reports: Receiver<ToEngine>,
}

fn start(w: &World, m: &Manifest, token: Option<&str>) -> Res<Job> {
    let clock = Arc::new(Shared(AtomicU64::new(100)));
    let (to, reports) = mpsc::channel();
    let sandboxes = Sandboxes {
        models: Arc::new(Models::new(
            Err("model.no_provider"),
            Err("model.no_provider"),
            Arc::new(Meter::default()),
        )),
        egress: Egress::public(),
        git: Git::with_base(&format!("http://127.0.0.1:{}", w.server.port)),
        forge: hyalos_broker::forge::Forge::github(),
        clock: clock.clone(),
    };
    let mut broker = Broker::with_sandboxes(key().public(), sandboxes).report_to(to);
    let socket = dir(&format!("sock-{}", m.job.seq()))?.join("job.sock");
    let answers = broker
        .handle(
            ToBroker::StartJob {
                job: m.job,
                role: m.role,
                manifest: key().sign(m)?.to_bytes()?,
                socket: socket.to_string_lossy().into_owned(),
                forge_token: token
                    .map(|t| ForgeToken::new(t, Timestamp::from_millis(99_999)).ok_or("token"))
                    .transpose()?,
            },
            clock.as_ref(),
        )?
        .ok_or("stopped")?;
    assert_eq!(answers, vec![ToEngine::Admitted { job: m.job }]);
    Ok(Job {
        port: bridge(&socket)?,
        _broker: broker,
        reports,
    })
}

impl Job {
    fn url(&self, repo: &str) -> String {
        format!("http://127.0.0.1:{}/git/{repo}.git", self.port)
    }

    /// The manifest gate's denials reported so far, as (reason, inputs).
    fn denials(&self) -> Vec<(String, Digest)> {
        std::thread::sleep(Duration::from_millis(50));
        self.reports
            .try_iter()
            .filter_map(|m| match m {
                ToEngine::Gate {
                    gate: Gate::Manifest,
                    answer: Answer::Deny,
                    reason,
                    inputs,
                    ..
                } => Some((reason.as_str().to_string(), inputs)),
                _ => None,
            })
            .collect()
    }
}

fn denial(reason: &str, input: &str) -> (String, Digest) {
    (reason.to_string(), inputs_digest(input))
}

#[test]
fn a_job_clones_and_pushes_its_branch_and_nothing_else() -> Res {
    if !have_git()? {
        return Ok(());
    }
    let w = world("main")?;
    let job = start(&w, &manifest(1, &["read", "push"]), Some(TOKEN))?;
    let work = w.home.join("work");
    ok(git(
        &w.home,
        &w.home,
        &[
            "clone",
            "-q",
            &job.url("octo/repo"),
            &work.to_string_lossy(),
        ],
    )?)?;
    assert_eq!(std::fs::read_to_string(work.join("README"))?, "hello\n");

    std::fs::write(work.join("CHANGE"), "made in the sandbox\n")?;
    ok(git(&w.home, &work, &["add", "CHANGE"])?)?;
    ok(git(&w.home, &work, &["commit", "-q", "-m", "change"])?)?;
    let head = ok(git(&w.home, &work, &["rev-parse", "HEAD"])?)?;
    ok(git(
        &w.home,
        &work,
        &["push", "-q", "origin", "HEAD:refs/heads/hyalos/work"],
    )?)?;
    assert_eq!(
        w.rev("octo/repo.git", "refs/heads/hyalos/work").as_deref(),
        Some(head.as_str())
    );
    assert!(job.denials().is_empty());
    let pushes = w.calls("/git-receive-pack");

    // Outside the prefix, a tag, a look-alike prefix, a deletion: each refused before the server.
    for (refspec, reason, refname) in [
        (
            "HEAD:refs/heads/main",
            "git.ref_not_allowed",
            "refs/heads/main",
        ),
        ("HEAD:refs/tags/v1", "git.ref_not_allowed", "refs/tags/v1"),
        (
            "HEAD:refs/heads/hyalos-other",
            "git.ref_not_allowed",
            "refs/heads/hyalos-other",
        ),
        (
            ":refs/heads/hyalos/work",
            "git.delete_refused",
            "refs/heads/hyalos/work",
        ),
    ] {
        let out = git(&w.home, &work, &["push", "-q", "origin", refspec])?;
        assert!(!out.status.success(), "{refspec} was pushed");
        assert_eq!(job.denials(), vec![denial(reason, refname)], "{refspec}");
    }
    // One allowed ref does not carry a refused one with it.
    let out = git(
        &w.home,
        &work,
        &[
            "push",
            "-q",
            "origin",
            "HEAD:refs/heads/hyalos/two",
            "HEAD:refs/heads/main",
        ],
    )?;
    assert!(!out.status.success());
    assert_eq!(w.rev("octo/repo.git", "refs/heads/hyalos/two"), None);
    assert_eq!(
        w.rev("octo/repo.git", "refs/heads/main").as_deref(),
        Some(w.main.as_str())
    );
    assert_eq!(w.rev("octo/repo.git", "refs/tags/v1"), None);
    assert!(w.rev("octo/repo.git", "refs/heads/hyalos/work").is_some());
    assert_eq!(
        w.calls("/git-receive-pack"),
        pushes,
        "a refused push reached the server"
    );

    // Every request the server saw carried the token; the sandbox's git had none to send.
    assert!(
        w.server.seen().iter().all(|s| s.authorized),
        "{:?}",
        w.server.seen()
    );
    Ok(())
}

#[test]
fn a_large_push_is_read_in_chunks_and_still_checked() -> Res {
    if !have_git()? {
        return Ok(());
    }
    let w = world("large")?;
    let job = start(&w, &manifest(2, &["read", "push"]), Some(TOKEN))?;
    let work = w.home.join("work");
    ok(git(
        &w.home,
        &w.home,
        &[
            "clone",
            "-q",
            &job.url("octo/repo"),
            &work.to_string_lossy(),
        ],
    )?)?;
    // Bytes that do not compress, so the pack is larger than git's 1 MiB post buffer.
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let noise: Vec<u8> = (0..3 * 1024 * 1024)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect();
    std::fs::write(work.join("BIG"), noise)?;
    ok(git(&w.home, &work, &["add", "BIG"])?)?;
    ok(git(&w.home, &work, &["commit", "-q", "-m", "big"])?)?;
    let head = ok(git(&w.home, &work, &["rev-parse", "HEAD"])?)?;
    let out = git(
        &w.home,
        &work,
        &["push", "-q", "origin", "HEAD:refs/heads/main"],
    )?;
    assert!(!out.status.success());
    assert_eq!(
        job.denials(),
        vec![denial("git.ref_not_allowed", "refs/heads/main")]
    );
    ok(git(
        &w.home,
        &work,
        &["push", "-q", "origin", "HEAD:refs/heads/hyalos/big"],
    )?)?;
    assert_eq!(
        w.rev("octo/repo.git", "refs/heads/hyalos/big").as_deref(),
        Some(head.as_str())
    );
    Ok(())
}

#[test]
fn another_repository_is_refused_and_reported() -> Res {
    if !have_git()? {
        return Ok(());
    }
    let w = world("other")?;
    let job = start(&w, &manifest(3, &["read", "push"]), Some(TOKEN))?;
    let out = git(
        &w.home,
        &w.home,
        &["clone", "-q", &job.url("octo/other"), "other"],
    )?;
    assert!(!out.status.success());
    assert_eq!(
        job.denials(),
        vec![denial("git.repo_not_allowed", "octo/other")]
    );
    assert_eq!(
        w.calls("other"),
        0,
        "the server was asked for another repository"
    );
    // The same repository spelled in another case is the manifest's.
    ok(git(
        &w.home,
        &w.home,
        &["clone", "-q", &job.url("Octo/Repo"), "same"],
    )?)?;
    Ok(())
}

#[test]
fn a_push_needs_the_push_action_and_any_call_needs_a_token() -> Res {
    if !have_git()? {
        return Ok(());
    }
    let w = world("actions")?;
    let reader = start(&w, &manifest(4, &["read"]), Some(TOKEN))?;
    let work = w.home.join("work");
    ok(git(
        &w.home,
        &w.home,
        &[
            "clone",
            "-q",
            &reader.url("octo/repo"),
            &work.to_string_lossy(),
        ],
    )?)?;
    let out = git(
        &w.home,
        &work,
        &["push", "-q", "origin", "HEAD:refs/heads/hyalos/x"],
    )?;
    assert!(!out.status.success());
    assert_eq!(
        reader.denials(),
        vec![denial("git.action_not_allowed", "git-receive-pack")]
    );
    assert_eq!(w.calls("receive-pack"), 0);

    let tokenless = start(&w, &manifest(5, &["read", "push"]), None)?;
    let before = w.server.seen().len();
    let out = git(
        &w.home,
        &w.home,
        &["clone", "-q", &tokenless.url("octo/repo"), "none"],
    )?;
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("403"),
        "{out:?}"
    );
    assert_eq!(
        w.server.seen().len(),
        before,
        "a call without a token reached the server"
    );
    Ok(())
}
