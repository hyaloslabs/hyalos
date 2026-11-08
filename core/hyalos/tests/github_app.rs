//! S3.9 and S3.4: the engine mints a per-job token from the GitHub App, scoped to the manifest's
//! repository and actions, hands it to the broker with the manifest, and revokes it when the job
//! ends (ADR-0004, ADR-0044). GitHub is a stand-in on loopback that checks the App's JWT with the
//! App's public key and answers as the API documents.
//!
//! `tests/fixtures/github-app-test.pem` is an RSA key made for these tests alone. It is not, and
//! never was, the key of any App.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hyalos::clock::SystemClock;
use hyalos::github::{AppSettings, GitHubApp, MintError};
use hyalos::launch::{BrokerProcess, fresh_manifest_key};
use hyalos_types::wire::{ToBroker, ToEngine, write_frame};
use hyalos_types::{Budget, Clock, JobId, MANIFEST_VERSION, Manifest, Role, Timestamp};
use ring::signature::{RSA_PKCS1_2048_8192_SHA256, RsaKeyPair, UnparsedPublicKey};
use rustls_pki_types::PrivateKeyDer;
use rustls_pki_types::pem::PemObject;
use serde_json::{Value, json};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

const EXE: &str = env!("CARGO_BIN_EXE_hyalos");
const KEY: &[u8] = include_bytes!("fixtures/github-app-test.pem");
const APP_ID: u64 = 424242;
const WAIT: Duration = Duration::from_secs(20);

/// One request the stand-in received.
#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    authorization: String,
    user_agent: bool,
    body: Value,
}

/// How the stand-in answers a token request.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Grants exactly what was asked, for the one repository.
    Exact,
    /// Grants one permission more than was asked.
    Wider,
    /// Issues a token with an expiry this build cannot read.
    BadExpiry,
    /// Refuses, with 422, a token asking for pull requests, as for an installation that has not
    /// accepted that permission (#256).
    NoPullRequests,
}

struct GitHub {
    port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl GitHub {
    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn api(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

fn public_key() -> Res<Vec<u8>> {
    let PrivateKeyDer::Pkcs1(der) = PrivateKeyDer::from_pem_slice(KEY)? else {
        return Err("fixture is not PKCS#1".into());
    };
    let pair = RsaKeyPair::from_der(der.secret_pkcs1_der()).map_err(|e| e.to_string())?;
    Ok(pair.public().as_ref().to_vec())
}

fn b64url_decode(text: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut bits = 0u32;
    let mut count = 0;
    let mut out = Vec::new();
    for c in text.bytes() {
        bits = bits << 6 | ALPHABET.iter().position(|a| *a == c)? as u32;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
        }
    }
    Some(out)
}

/// The claims of a JWT whose RS256 signature checks against `public`, or `None`.
fn checked_claims(jwt: &str, public: &[u8]) -> Option<Value> {
    let mut parts = jwt.split('.');
    let (header, claims, signature) = (parts.next()?, parts.next()?, parts.next()?);
    let key = UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, public);
    key.verify(
        format!("{header}.{claims}").as_bytes(),
        &b64url_decode(signature)?,
    )
    .ok()?;
    let header: Value = serde_json::from_slice(&b64url_decode(header)?).ok()?;
    (header["alg"] == "RS256").then_some(())?;
    serde_json::from_slice(&b64url_decode(claims)?).ok()
}

/// A stand-in for the GitHub API. The App is installed on `octo/repo` only (installation 77).
fn github(mode: Mode) -> Res<GitHub> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let public = public_key()?;
    std::thread::spawn(move || {
        let mut issued: Vec<String> = Vec::new();
        for (n, stream) in listener.incoming().enumerate() {
            let Ok(mut stream) = stream else { continue };
            let Ok(read_half) = stream.try_clone() else {
                continue;
            };
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            let mut parts = line.split_whitespace();
            let (method, path) = (
                parts.next().unwrap_or("").to_string(),
                parts.next().unwrap_or("").to_string(),
            );
            let (mut length, mut authorization, mut user_agent) = (0usize, String::new(), false);
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).unwrap_or(0) == 0 || header.trim().is_empty() {
                    break;
                }
                let (name, value) = header.split_once(':').unwrap_or(("", ""));
                match name.trim().to_ascii_lowercase().as_str() {
                    "content-length" => length = value.trim().parse().unwrap_or(0),
                    "authorization" => authorization = value.trim().to_string(),
                    "user-agent" => user_agent = true,
                    _ => {}
                }
            }
            let mut body = vec![0; length];
            let _ = reader.read_exact(&mut body);
            let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            if let Ok(mut log) = log.lock() {
                log.push(Seen {
                    method: method.clone(),
                    path: path.clone(),
                    authorization: authorization.clone(),
                    user_agent,
                    body: body.clone(),
                });
            }
            let bearer = authorization.strip_prefix("Bearer ").unwrap_or("");
            let as_app =
                checked_claims(bearer, &public).is_some_and(|c| c["iss"].as_u64() == Some(APP_ID));
            let (status, reply) = match (method.as_str(), path.as_str()) {
                (_, _) if method != "DELETE" && !as_app => {
                    (401, json!({"message": "Bad credentials"}))
                }
                ("GET", "/repos/octo/repo/installation") => (200, json!({"id": 77})),
                ("GET", _) => (404, json!({"message": "Not Found"})),
                ("POST", "/app/installations/77/access_tokens")
                    if mode == Mode::NoPullRequests
                        && body["permissions"].get("pull_requests").is_some() =>
                {
                    (
                        422,
                        json!({"message": "The permissions requested are not granted to this installation."}),
                    )
                }
                ("POST", "/app/installations/77/access_tokens") => {
                    // As long as GitHub's are now (about 400 characters, ADR-0045).
                    let token = format!("ghs_stand_in_{n}_{port}_{}", "x".repeat(360));
                    issued.push(token.clone());
                    let mut granted = body["permissions"].clone();
                    granted["metadata"] = json!("read");
                    if mode == Mode::Wider {
                        granted["administration"] = json!("write");
                    }
                    (
                        201,
                        json!({
                            "token": token,
                            "expires_at": if mode == Mode::BadExpiry { "soon" } else { "2999-01-01T00:00:00Z" },
                            "permissions": granted,
                            "repository_selection": "selected",
                            "repositories": [{ "full_name": "octo/repo" }],
                        }),
                    )
                }
                ("DELETE", "/installation/token") if issued.iter().any(|t| t == bearer) => {
                    issued.retain(|t| t != bearer);
                    (204, Value::Null)
                }
                _ => (401, json!({"message": "Bad credentials"})),
            };
            let text = if reply.is_null() {
                String::new()
            } else {
                reply.to_string()
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
                text.len()
            );
        }
    });
    Ok(GitHub { port, seen })
}

fn app(gh: &GitHub) -> Res<GitHubApp> {
    let settings = AppSettings::parse(&format!(
        "app_id = {APP_ID}\napi = {0}\ngit = {0}\n",
        gh.api()
    ))?;
    Ok(GitHubApp::new(&settings, KEY)?)
}

fn manifest(job: u64, actions: &[&str]) -> Manifest {
    Manifest {
        version: MANIFEST_VERSION,
        job: JobId::from_seq(job),
        role: Role::Actor,
        tainted: false,
        repo: "octo/repo".into(),
        branch_prefix: "hyalos/".into(),
        pr_base: None,
        actions: actions.iter().map(|a| a.to_string()).collect(),
        egress_hosts: BTreeSet::new(),
        tools: BTreeSet::new(),
        data_flows: BTreeSet::new(),
        budget: Budget { tokens: 1 },
        deadline: Timestamp::from_millis(SystemClock.now().as_millis() + 3_600_000),
        approval: None,
        handles: Default::default(),
        result: None,
    }
}

fn dir(name: &str) -> Res<PathBuf> {
    let dir = std::env::temp_dir().join(format!("hygh-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn the_engine_mints_a_token_for_the_manifest_hands_it_to_the_broker_and_revokes_it() -> Res {
    let gh = github(Mode::Exact)?;
    let app = app(&gh)?;
    let m = manifest(5, &["read", "push", "open_pr", "use_tool"]);
    let now = SystemClock.now();
    let minted = app.mint(&m, now)?.ok_or("no token minted")?;
    assert_eq!(minted.installation, 77);

    // Two calls, both as the App: find the installation, then ask for exactly this repository and
    // what the actions need.
    let seen = gh.seen();
    assert_eq!(
        seen.iter()
            .map(|s| format!("{} {}", s.method, s.path))
            .collect::<Vec<_>>(),
        [
            "GET /repos/octo/repo/installation",
            "POST /app/installations/77/access_tokens"
        ]
    );
    assert!(seen.iter().all(|s| s.user_agent));
    assert_eq!(
        seen[1].body,
        json!({
            "repositories": ["repo"],
            "permissions": {"contents": "write", "issues": "read", "pull_requests": "write"},
        })
    );
    // The JWT is the App's, short-lived and signed with its key.
    let jwt = seen[1]
        .authorization
        .strip_prefix("Bearer ")
        .ok_or("no bearer")?;
    let claims = checked_claims(jwt, &public_key()?).ok_or("the JWT does not verify")?;
    assert_eq!(claims["iss"].as_u64(), Some(APP_ID));
    let (iat, exp) = (
        claims["iat"].as_u64().ok_or("iat")?,
        claims["exp"].as_u64().ok_or("exp")?,
    );
    let now_s = now.as_millis() / 1000;
    assert!(iat <= now_s && exp > now_s && exp - iat <= 600, "{claims}");

    // The broker gets the token with the manifest, over the channel, and admits the job.
    let runtime = dir("runtime")?;
    let key = fresh_manifest_key()?;
    let mut broker = BrokerProcess::launch(Path::new(EXE), &runtime, &key.public(), WAIT)?;
    let socket_dir = dir("job")?;
    let start = ToBroker::StartJob {
        job: m.job,
        role: m.role,
        manifest: key.sign(&m)?.to_bytes()?,
        socket: socket_dir.join("b.sock").to_string_lossy().into_owned(),
        forge_token: Some(minted.token.clone()),
    };
    broker.send(&start)?;
    assert_eq!(broker.recv()?, Some(ToEngine::Admitted { job: m.job }));

    // What crossed to the broker holds the token and nothing of the App's key, and the broker was
    // started with no argument that names it.
    let mut frame = Vec::new();
    write_frame(&mut frame, &start)?;
    assert!(contains(&frame, minted.token.expose().as_bytes()));
    let der = public_key()?;
    for line in std::str::from_utf8(KEY)?
        .lines()
        .filter(|l| !l.starts_with("-----"))
    {
        assert!(!contains(&frame, line.as_bytes()));
    }
    assert!(!contains(&frame, &der[..32]));
    let cmdline = std::fs::read(format!("/proc/{}/cmdline", broker.pid()))?;
    assert!(!contains(&cmdline, b"github"), "{cmdline:?}");

    // The job ends: the broker forgets it and the engine revokes the token at GitHub.
    broker.send(&ToBroker::EndJob { job: m.job })?;
    app.revoke(&minted.token)?;
    let last = gh.seen().pop().ok_or("nothing seen")?;
    assert_eq!(
        (last.method.as_str(), last.path.as_str()),
        ("DELETE", "/installation/token")
    );
    assert_eq!(
        last.authorization,
        format!("Bearer {}", minted.token.expose())
    );
    // A second revoke is refused: the stand-in, like GitHub, no longer knows the token.
    assert!(matches!(
        app.revoke(&minted.token),
        Err(MintError::Refused { status: 401, .. })
    ));
    broker.send(&ToBroker::Shutdown)?;
    Ok(())
}

#[test]
fn a_job_that_needs_no_forge_access_gets_no_token_and_github_is_not_asked() -> Res {
    let gh = github(Mode::Exact)?;
    let minted = app(&gh)?.mint(&manifest(1, &["use_tool", "data_flow"]), SystemClock.now())?;
    assert!(minted.is_none());
    assert!(gh.seen().is_empty());
    Ok(())
}

#[test]
fn an_action_no_token_is_minted_for_is_refused_before_github_is_asked() -> Res {
    let gh = github(Mode::Exact)?;
    let err = app(&gh)?
        .mint(&manifest(1, &["read", "publish"]), SystemClock.now())
        .err()
        .ok_or("minted")?;
    assert_eq!(err, MintError::Action("publish".into()));
    assert!(gh.seen().is_empty());
    Ok(())
}

#[test]
fn a_repository_the_app_is_not_installed_on_is_refused() -> Res {
    let gh = github(Mode::Exact)?;
    let mut m = manifest(1, &["read"]);
    m.repo = "octo/elsewhere".into();
    let err = app(&gh)?
        .mint(&m, SystemClock.now())
        .err()
        .ok_or("minted")?;
    assert_eq!(
        err,
        MintError::NotInstalled {
            repo: "octo/elsewhere".into()
        }
    );
    assert!(err.to_string().contains("docs/operations/github-app.md"));
    Ok(())
}

#[test]
fn a_grant_wider_than_asked_is_revoked_and_never_used() -> Res {
    let gh = github(Mode::Wider)?;
    let err = app(&gh)?
        .mint(&manifest(1, &["read"]), SystemClock.now())
        .err()
        .ok_or("minted")?;
    assert_eq!(err, MintError::Wider);
    let seen = gh.seen();
    let last = seen.last().ok_or("nothing seen")?;
    assert_eq!(
        (last.method.as_str(), last.path.as_str()),
        ("DELETE", "/installation/token")
    );
    Ok(())
}

#[test]
fn a_key_that_is_not_the_apps_is_refused_by_github() -> Res {
    let gh = github(Mode::Exact)?;
    let settings = AppSettings::parse(&format!(
        "app_id = {}\napi = {1}\ngit = {1}\n",
        APP_ID + 1,
        gh.api()
    ))?;
    let err = GitHubApp::new(&settings, KEY)?
        .mint(&manifest(1, &["read"]), SystemClock.now())
        .err()
        .ok_or("minted")?;
    assert!(
        matches!(err, MintError::Refused { status: 401, .. }),
        "{err}"
    );
    assert!(err.to_string().contains("app_id"), "{err}");
    Ok(())
}

fn config(name: &str, gh: &GitHub, key_mode: u32) -> Res<PathBuf> {
    let config = dir(name)?;
    std::fs::write(
        config.join("github.app"),
        format!("app_id = {APP_ID}\napi = {0}\ngit = {0}\n", gh.api()),
    )?;
    let key = config.join("github-app.pem");
    std::fs::write(&key, KEY)?;
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(key_mode))?;
    Ok(config)
}

#[test]
fn the_key_file_must_be_the_operators_alone() -> Res {
    let gh = github(Mode::Exact)?;
    assert!(GitHubApp::load(&config("private", &gh, 0o600)?).is_ok());
    let err = GitHubApp::load(&config("open", &gh, 0o644)?)
        .err()
        .ok_or("loaded")?;
    assert!(matches!(err, MintError::Key(_)), "{err}");
    let missing = dir("missing")?;
    assert!(matches!(
        GitHubApp::load(&missing),
        Err(MintError::Settings(_))
    ));
    Ok(())
}

#[test]
fn github_check_mints_shows_and_revokes_without_printing_the_token() -> Res {
    let gh = github(Mode::Exact)?;
    let config = config("check", &gh, 0o600)?;
    let run = |repo: &str| {
        Command::new(EXE)
            .args(["github", "check", repo])
            .env_clear()
            .env("HYALOS_CONFIG_DIR", &config)
            .output()
    };
    let out = run("octo/repo")?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        text.contains("installation 77 can mint for octo/repo"),
        "{text}"
    );
    assert!(
        text.contains("every permission a job needs is granted"),
        "{text}"
    );
    assert!(!text.contains("ghs_"), "{text}");
    // One token per permission a job needs, each at its level and each revoked (#256).
    let seen = gh.seen();
    let asked: Vec<Value> = seen
        .iter()
        .filter(|s| s.path.ends_with("/access_tokens"))
        .map(|s| s.body["permissions"].clone())
        .collect();
    assert_eq!(
        asked,
        [
            json!({"contents": "write"}),
            json!({"issues": "read"}),
            json!({"pull_requests": "write"}),
        ]
    );
    let revoked = seen.iter().filter(|s| s.method == "DELETE").count();
    assert_eq!(
        (revoked, text.matches("revoked it").count()),
        (3, 3),
        "{text}"
    );

    let out = run("octo/elsewhere")?;
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not installed on octo/elsewhere"));
    for bad in [
        &["github"][..],
        &["github", "check"],
        &["github", "mint", "octo/repo"],
    ] {
        let out = Command::new(EXE).args(bad).env_clear().output()?;
        assert_eq!(out.status.code(), Some(2), "{bad:?}");
    }
    Ok(())
}

#[test]
fn a_token_this_build_cannot_use_is_still_revoked() -> Res {
    // GitHub issued it, so leaving it alive until it expires is not an option (ADR-0045).
    let gh = github(Mode::BadExpiry)?;
    let err = app(&gh)?
        .mint(&manifest(1, &["read"]), SystemClock.now())
        .err()
        .ok_or("minted")?;
    assert!(matches!(err, MintError::Response(_)), "{err}");
    let seen = gh.seen();
    let last = seen.last().ok_or("nothing seen")?;
    assert_eq!(
        (last.method.as_str(), last.path.as_str()),
        ("DELETE", "/installation/token")
    );
    assert!(
        last.authorization.contains(&"x".repeat(360)),
        "the long token was revoked"
    );
    Ok(())
}

#[test]
fn github_check_names_a_permission_the_installation_has_not_granted() -> Res {
    let gh = github(Mode::NoPullRequests)?;
    let config = config("check-pr", &gh, 0o600)?;
    let out = Command::new(EXE)
        .args(["github", "check", "octo/repo"])
        .env_clear()
        .env("HYALOS_CONFIG_DIR", &config)
        .output()?;
    let (stdout, stderr) = (
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert_eq!(out.status.code(), Some(1), "{stdout}{stderr}");
    assert!(
        stderr.contains("pull_requests: write (status 422)")
            && stderr.contains("has not accepted it")
            && stderr.contains("settings/installations"),
        "{stderr}"
    );
    // The two it could mint were minted and revoked, and named as granted.
    assert!(
        stdout.contains("contents: write") && stdout.contains("issues: read"),
        "{stdout}"
    );
    assert!(!stdout.contains("every permission"), "{stdout}");
    let seen = gh.seen();
    assert_eq!(seen.iter().filter(|s| s.method == "DELETE").count(), 2);
    Ok(())
}
