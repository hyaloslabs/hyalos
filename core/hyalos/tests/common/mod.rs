//! What the tests of `hyalos run` share: a stand-in GitHub on loopback (the App's endpoints, its git
//! over `git http-backend`, and opening a pull request) and a configured engine, broker and App.

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hyalos::interrupt::Interrupt;
use hyalos::run_cmd::{Detect, Setup};
use hyalos_log::Log;
use hyalos_types::Event;

pub type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub const EXE: &str = env!("CARGO_BIN_EXE_hyalos");

/// The passphrase each world's operator key is sealed under.
pub const PASSPHRASE: &str = "the operator's own passphrase";
const PEM: &[u8] = include_bytes!("../fixtures/github-app-test.pem");
// `merge` is permitted, as an operator who wants `hyalos merge` must; the baseline still forbids a
// tainted job's and any unapproved merge (ADR-0004, ADR-0071).
const POLICY: &str = r#"permit (principal, action in [Action::"read", Action::"push", Action::"open_pr", Action::"request_host", Action::"data_flow", Action::"merge"], resource);"#;

/// The head of the stand-in's pull request 42 of `octo/repo`, and the commit merging it makes.
pub const PR_HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
pub const MERGE_COMMIT: &str = "abababababababababababababababababababab";

/// Issue 7 of the stand-in's `octo/repo`.
pub const ISSUE_TITLE: &str = "Add a CHANGES file";
pub const ISSUE_BODY: &str = "There is no CHANGES file. issue-body-marker-5b1e";

pub fn dir(name: &str) -> Res<PathBuf> {
    // Short, so the job's socket path stays under the Unix limit.
    let dir = std::env::temp_dir().join(format!("hyrj-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Whether `git` (and its `http-backend`) can be run here. CI always has it.
pub fn have_git() -> Res<bool> {
    let found = Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !found && std::env::var_os("HYALOS_REQUIRE_GIT").is_some() {
        return Err("HYALOS_REQUIRE_GIT is set and git is not installed".into());
    }
    Ok(found)
}

pub fn git(home: &Path, cwd: &Path, args: &[&str]) -> Res<Output> {
    Ok(Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .stdin(Stdio::null())
        .output()?)
}

pub fn ok(output: Output) -> Res<String> {
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!("git failed: {}", String::from_utf8_lossy(&output.stderr)).into())
    }
}

pub fn base64(bytes: &[u8]) -> String {
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

pub fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Decodes unpadded base64url, as a JWT's parts are written; anything else decodes to nothing.
pub fn base64url_decode(text: &str) -> Vec<u8> {
    let value = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'-' => Some(62),
        b'_' => Some(63),
        _ => None,
    };
    let (mut out, mut acc, mut bits) = (Vec::new(), 0u32, 0);
    for c in text.bytes() {
        let Some(v) = value(c) else { return Vec::new() };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

/// The App's key the stand-in hands back from the setup page's code.
pub const APP_PEM: &[u8] = include_bytes!("../fixtures/github-app-test.pem");
/// A secret GitHub returns with the App, which setup never reads.
pub const CLIENT_SECRET: &str = "client-secret-marker-41c9";

/// One request the stand-in received.
#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub authorization: String,
    pub body: Vec<u8>,
}

/// A stand-in GitHub on loopback: the App's endpoints for `octo/repo` (installation 77), its git
/// over `git http-backend` with the token GitHub expects, and opening a pull request.
pub struct GitHub {
    port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
    issued: Arc<Mutex<Vec<String>>>,
    /// While set, every request is answered 503, as by a forge that is down (ADR-0106's chaos).
    down: Arc<std::sync::atomic::AtomicBool>,
}

impl GitHub {
    pub fn start(root: &Path, revoke_fails: bool) -> Res<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let issued = Arc::new(Mutex::new(Vec::new()));
        let down = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (root, log, tokens) = (root.to_path_buf(), seen.clone(), issued.clone());
        let is_down = down.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let (root, log, tokens) = (root.clone(), log.clone(), tokens.clone());
                let down = is_down.load(std::sync::atomic::Ordering::SeqCst);
                std::thread::spawn(move || {
                    let _ = serve(stream, &root, &log, &tokens, revoke_fails, down);
                });
            }
        });
        Ok(Self {
            port,
            seen,
            issued,
            down,
        })
    }

    /// Answers every request from now on with 503, until [`GitHub::come_up`].
    pub fn go_down(&self) {
        self.down.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn come_up(&self) {
        self.down.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn issued(&self) -> Vec<String> {
        self.issued.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

fn serve(
    stream: TcpStream,
    root: &Path,
    log: &Mutex<Vec<Seen>>,
    issued: &Mutex<Vec<String>>,
    revoke_fails: bool,
    down: bool,
) -> Res {
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
    let authorization = get("authorization").unwrap_or_default();
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    if let Ok(mut log) = log.lock() {
        log.push(Seen {
            method: method.clone(),
            path: path.to_string(),
            authorization: authorization.clone(),
            body: body.clone(),
        });
    }
    let tokens = issued.lock().map(|t| t.clone()).unwrap_or_default();
    // GitHub takes the scheme in any case; the broker's GraphQL call sends `bearer`.
    let bearer = authorization
        .split_once(' ')
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .map_or("", |(_, token)| token);
    let mut out = stream;
    let mut reply = |status: &str, json: &str| -> Res {
        write!(
            out,
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{json}",
            json.len()
        )?;
        Ok(())
    };
    if down {
        return reply("503 Service Unavailable", "{}");
    }
    if path.starts_with("/octo/repo.git/") {
        let token = tokens.last().cloned().unwrap_or_default();
        let expected = format!(
            "Basic {}",
            base64(format!("x-access-token:{token}").as_bytes())
        );
        if token.is_empty() || authorization != expected {
            return reply("401 Unauthorized", "{}");
        }
        return git_backend(out, root, &method, path, query, &get, body);
    }
    // The setup page's code, exchanged for the App it made (ADR-0084): `goodcode1` is the App setup
    // asks for (7), `widecode1` one with more permissions, `elsecode1` one another account owns,
    // `hookcode1` one (8) whose webhook goes elsewhere.
    if let Some(code) = path
        .strip_prefix("/app-manifests/")
        .and_then(|p| p.strip_suffix("/conversions"))
        .filter(|_| method == "POST")
    {
        let mut permissions = serde_json::json!({
            "contents": "write", "issues": "write", "metadata": "read", "pull_requests": "write",
        });
        let (mut owner, mut id) = ("op", 7);
        match code {
            "goodcode1" => {}
            "hookcode1" => id = 8,
            "widecode1" => permissions["administration"] = "write".into(),
            "elsecode1" => owner = "someone",
            _ => return reply("404 Not Found", r#"{"message":"Not Found"}"#),
        }
        let app = serde_json::json!({
            "id": id, "slug": "hyalos-op", "node_id": "A_1", "name": "hyalos-op",
            "owner": { "login": owner, "type": "User" },
            "permissions": permissions, "events": [],
            "client_id": "Iv1.client", "client_secret": CLIENT_SECRET,
            "webhook_secret": null,
            "pem": String::from_utf8_lossy(APP_PEM),
        });
        return reply("201 Created", &app.to_string());
    }
    // The App's webhook, by the App the JWT names: App 8's goes elsewhere.
    if (method.as_str(), path) == ("GET", "/app/hook/config") && bearer.contains('.') {
        let claims = bearer
            .split('.')
            .nth(1)
            .map(base64url_decode)
            .unwrap_or_default();
        let claims: serde_json::Value = serde_json::from_slice(&claims).unwrap_or_default();
        let url = if claims["iss"] == 8 {
            "https://collector.example.com/hook"
        } else {
            "https://hyaloslabs.github.io/hyalos/"
        };
        return reply(
            "200 OK",
            &serde_json::json!({ "url": url, "content_type": "json", "insecure_ssl": "0" })
                .to_string(),
        );
    }
    match (method.as_str(), path) {
        // The App's JWT is checked by github_app.rs; here only that one is sent.
        ("GET", "/repos/octo/repo/installation") if bearer.contains('.') => {
            reply("200 OK", r#"{"id":77}"#)
        }
        ("POST", "/app/installations/77/access_tokens") if bearer.contains('.') => {
            let mut token = [0u8; 16];
            getrandom::fill(&mut token).map_err(|e| e.to_string())?;
            let token = format!(
                "ghs_{}",
                token.iter().map(|b| format!("{b:02x}")).collect::<String>()
            );
            if let Ok(mut issued) = issued.lock() {
                issued.push(token.clone());
            }
            let asked: serde_json::Value = serde_json::from_slice(&body)?;
            let mut granted = asked["permissions"].clone();
            granted["metadata"] = "read".into();
            let answer = serde_json::json!({
                "token": token,
                "expires_at": "2999-01-01T00:00:00Z",
                "permissions": granted,
                "repository_selection": "selected",
                "repositories": [{ "full_name": "octo/repo" }],
            });
            reply("201 Created", &answer.to_string())
        }
        // The issue query (ADR-0047): issue 7 of octo/repo exists, any other number does not.
        ("POST", "/graphql") if tokens.iter().any(|t| t == bearer) => {
            let asked: serde_json::Value = serde_json::from_slice(&body)?;
            let vars = &asked["variables"];
            let issue = if vars["owner"] == "octo" && vars["name"] == "repo" && vars["number"] == 7
            {
                serde_json::json!({
                    "number": 7, "state": "OPEN", "title": ISSUE_TITLE, "body": ISSUE_BODY,
                    "author": { "__typename": "User", "login": "someone" },
                    "labels": { "totalCount": 1, "nodes": [{ "name": "docs" }] },
                    "userContentEdits": { "totalCount": 0, "nodes": [] },
                    "timelineItems": { "totalCount": 0, "nodes": [] },
                })
            } else {
                serde_json::Value::Null
            };
            reply(
                "200 OK",
                &serde_json::json!({ "data": { "repository": { "issue": issue } } }).to_string(),
            )
        }
        // A user's permission is the role in `<root>/permissions/<login>`, and 404 without one
        // (ADR-0103).
        ("GET", p)
            if tokens.iter().any(|t| t == bearer)
                && p.starts_with("/repos/octo/repo/collaborators/")
                && p.ends_with("/permission") =>
        {
            let login = p
                .trim_start_matches("/repos/octo/repo/collaborators/")
                .trim_end_matches("/permission");
            match std::fs::read_to_string(root.join("permissions").join(login)) {
                Ok(role) => reply(
                    "200 OK",
                    &serde_json::json!({ "permission": role.trim(), "user": { "login": login } })
                        .to_string(),
                ),
                Err(_) => reply("404 Not Found", r#"{"message":"Not Found"}"#),
            }
        }
        ("POST", "/repos/octo/repo/pulls") if tokens.iter().any(|t| t == bearer) => reply(
            "201 Created",
            r#"{"number":1,"html_url":"https://github.example/octo/repo/pull/1"}"#,
        ),
        // Pull request 42 merges only at its head, as GitHub's `sha` does.
        ("PUT", "/repos/octo/repo/pulls/42/merge") if tokens.iter().any(|t| t == bearer) => {
            let asked: serde_json::Value = serde_json::from_slice(&body)?;
            if asked["sha"] == PR_HEAD {
                reply(
                    "200 OK",
                    &serde_json::json!({ "sha": MERGE_COMMIT, "merged": true, "message": "merged" })
                        .to_string(),
                )
            } else {
                reply(
                    "409 Conflict",
                    r#"{"message":"Head branch was modified. Review and try the merge again."}"#,
                )
            }
        }
        ("DELETE", "/installation/token") if revoke_fails => reply("500 Oops", "{}"),
        ("DELETE", "/installation/token") if tokens.iter().any(|t| t == bearer) => {
            if let Ok(mut issued) = issued.lock() {
                issued.retain(|t| t != bearer);
            }
            write!(out, "HTTP/1.1 204 No Content\r\nconnection: close\r\n\r\n")?;
            Ok(())
        }
        _ => reply("401 Unauthorized", r#"{"message":"Bad credentials"}"#),
    }
}

fn git_backend(
    mut out: TcpStream,
    root: &Path,
    method: &str,
    path: &str,
    query: &str,
    get: &dyn Fn(&str) -> Option<String>,
    body: Vec<u8>,
) -> Res {
    let mut cgi = Command::new("git")
        .arg("http-backend")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_PROJECT_ROOT", root)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("REQUEST_METHOD", method)
        .env("PATH_INFO", path)
        .env("QUERY_STRING", query)
        .env("CONTENT_TYPE", get("content-type").unwrap_or_default())
        .env("CONTENT_LENGTH", body.len().to_string())
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
    let rest = &response[split.0 + split.1..];
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
            rest.len()
        )
        .as_bytes(),
    )?;
    out.write_all(rest)?;
    Ok(())
}

/// A bare `octo/repo` with one commit on `main`, served from `root`.
pub fn repository(root: &Path, home: &Path) -> Res {
    let bare = root.join("octo/repo.git");
    std::fs::create_dir_all(&bare)?;
    ok(git(
        home,
        &bare,
        &["init", "--bare", "--initial-branch=main"],
    )?)?;
    let seed = root.join("seed");
    std::fs::create_dir_all(&seed)?;
    ok(git(home, &seed, &["init", "--initial-branch=main"])?)?;
    std::fs::write(seed.join("README.md"), "# repo\n")?;
    ok(git(home, &seed, &["add", "README.md"])?)?;
    ok(git(home, &seed, &["commit", "-m", "first"])?)?;
    let bare_text = bare.to_string_lossy().into_owned();
    ok(git(home, &seed, &["push", &bare_text, "main"])?)?;
    Ok(())
}

/// A local OpenAI-compatible model: answers each call with the next text it is given, and keeps
/// every request body it received. With no text left it answers 500.
pub struct Model {
    port: u16,
    sent: Arc<Mutex<Vec<serde_json::Value>>>,
}

impl Model {
    pub fn start(texts: &[&str]) -> Res<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let sent = Arc::new(Mutex::new(Vec::new()));
        let log = sent.clone();
        let mut texts: Vec<String> = texts.iter().rev().map(|t| t.to_string()).collect();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let _ = answer(stream, texts.pop(), &log);
            }
        });
        Ok(Self { port, sent })
    }

    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn sent(&self) -> Vec<serde_json::Value> {
        self.sent.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

fn answer(stream: TcpStream, text: Option<String>, log: &Mutex<Vec<serde_json::Value>>) -> Res {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" {
            break;
        }
        if let Some((n, v)) = line.split_once(':')
            && n.trim().eq_ignore_ascii_case("content-length")
        {
            length = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    if let Ok(mut log) = log.lock() {
        log.push(serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null));
    }
    let (status, json) = match text {
        Some(text) => (
            "200 OK",
            serde_json::json!({
                "object": "response",
                "output": [{"type": "message", "content": [{"type": "output_text", "text": text}]}],
                "usage": {"input_tokens": 40, "output_tokens": 8},
            })
            .to_string(),
        ),
        None => ("500 Oops", "{}".to_string()),
    };
    let mut out = stream;
    write!(
        out,
        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{json}",
        json.len()
    )?;
    Ok(())
}

/// Everything a job needs on disk: the engine's key, an approved policy, provider keys and the App.
pub struct World {
    pub root: PathBuf,
    pub config: PathBuf,
    pub data: PathBuf,
    pub runtime: PathBuf,
    pub provider_key: String,
}

impl World {
    pub fn new(name: &str, github: Option<&GitHub>) -> Res<Self> {
        let root = dir(name)?;
        let (config, data, runtime) = (root.join("c"), root.join("d"), root.join("r"));
        for d in [&config, &data] {
            std::fs::create_dir_all(d)?;
            std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700))?;
        }
        let world = Self {
            root,
            config,
            data,
            runtime,
            provider_key: format!("sk-test-{}-{}", std::process::id(), name),
        };
        ok_cli(world.cli(&["init"])?)?;
        ok_cli(world.signed(&["operator", "init"])?)?;
        let policy = world.root.join("policy.cedar");
        std::fs::write(&policy, POLICY)?;
        ok_cli(world.signed(&["policy", "approve", &policy.to_string_lossy()])?)?;
        let private = |path: PathBuf, text: &[u8]| -> Res {
            std::fs::write(&path, text)?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            Ok(())
        };
        // A model the harness never calls here; the key is still the broker's to hold. Its base is a
        // name, so the egress gate has a provider host to refuse (ADR-0051).
        private(
            world.config.join("provider.keys"),
            format!(
                "openai.key = {}\nopenai.base = http://localhost:9\n",
                world.provider_key
            )
            .as_bytes(),
        )?;
        if let Some(gh) = github {
            std::fs::write(
                world.config.join("github.app"),
                format!("app_id = 424242\napi = {0}\ngit = {0}\n", gh.base()),
            )?;
            private(world.config.join("github-app.pem"), PEM)?;
        }
        Ok(world)
    }

    /// Points the broker's OpenAI key at `model`, for the readers and planners of a workflow.
    pub fn use_model(&self, model: &Model) -> Res {
        let path = self.config.join("provider.keys");
        std::fs::write(
            &path,
            format!(
                "openai.key = {}\nopenai.base = {}\n",
                self.provider_key,
                model.base()
            ),
        )?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        Ok(())
    }

    pub fn cli(&self, args: &[&str]) -> Res<Output> {
        Ok(Command::new(EXE)
            .args(args)
            .env("HYALOS_CONFIG_DIR", &self.config)
            .env("HYALOS_DATA_DIR", &self.data)
            .env("HYALOS_RUNTIME_DIR", &self.runtime)
            .output()?)
    }

    /// A command that records the operator's decision, given the passphrase on stdin (ADR-0070).
    pub fn signed(&self, args: &[&str]) -> Res<Output> {
        self.signed_with(args, PASSPHRASE)
    }

    /// As [`World::signed`], with `passphrase` on stdin.
    pub fn signed_with(&self, args: &[&str], passphrase: &str) -> Res<Output> {
        let mut child = Command::new(EXE)
            .args(args)
            .arg("--passphrase-stdin")
            .env("HYALOS_CONFIG_DIR", &self.config)
            .env("HYALOS_DATA_DIR", &self.data)
            .env("HYALOS_RUNTIME_DIR", &self.runtime)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            // A command that refuses first may not read it.
            let _ = writeln!(stdin, "{passphrase}");
        }
        Ok(child.wait_with_output()?)
    }

    /// The operator's key, as `hyalos operator init` made it for this world.
    pub fn operator_key(&self) -> Option<hyalos_types::OperatorKey> {
        hyalos::operator::unlock(&self.config, PASSPHRASE).ok()
    }

    pub fn setup(&self, detect: Detect, timeout: Duration) -> Setup {
        self.setup_with(detect, timeout, Interrupt::none())
    }

    /// As [`World::setup`], with the run taking signals through `interrupt` (ADR-0053).
    pub fn setup_with(&self, detect: Detect, timeout: Duration, interrupt: Interrupt) -> Setup {
        Setup {
            config_dir: self.config.clone(),
            log: self.data.join("events.db"),
            runtime_dir: self.runtime.clone(),
            broker_exe: PathBuf::from(EXE),
            detect,
            detect_timeout: timeout,
            interrupt,
            on_task: None,
            operator: self.operator_key(),
        }
    }

    pub fn events(&self) -> Res<Vec<Event>> {
        let key = hyalos::keys::load(&self.config.join("engine.key"))?;
        let log = Log::open(&self.data.join("events.db"), key)?;
        let mut events = Vec::new();
        for stored in log.events()? {
            events.push(Event::decode(
                &stored.kind,
                stored.schema_version,
                &stored.payload,
            )?);
        }
        Ok(events)
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub fn ok_cli(output: Output) -> Res<String> {
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!(
            "hyalos failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into())
    }
}
