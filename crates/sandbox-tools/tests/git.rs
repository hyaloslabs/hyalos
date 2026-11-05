//! S3.3b: the image's own `git` clones and pushes through the broker's git proxy (ADR-0046, ADR-0049).
//! It runs under the substrate's isolation against the real broker and a git server on this machine's
//! loopback (`git http-backend`) that accepts only the job's token, which the broker adds. A push
//! outside the branch prefix is refused and reported as the manifest gate's `deny`, git makes no DNS
//! lookup, and every request on the job's channel is a git call to the forwarder, never a proxy request.
//!
//! The control: without the image's `http.<forwarder>.proxy` setting and with `NO_PROXY` emptied, the
//! same clone goes out as a proxy request for `127.0.0.1`, which the broker refuses, so the check above
//! would see it. Like the other image tests, it skips without the image unless
//! `HYALOS_REQUIRE_SANDBOX_IMAGE` is set, and needs the host's `git` for the server.

mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};

use common::{Res, Spec, asked_name, image_present, job_spec, not_model_calls, sandboxed_over};
use hyalos_broker::egress::inputs_digest;
use hyalos_broker::git::Git;
use hyalos_types::wire::ToEngine;
use hyalos_types::{Answer, Gate};

const TOKEN: &str = "ghs_sandbox_git_test_token";

/// What the sandbox runs: clone, commit, push a branch inside the prefix, then try `main`.
const SCRIPT: &str = "set -e
cd /tmp
git clone -q http://127.0.0.1:8089/git/octo/repo.git work
cd work
echo from-the-sandbox > job.txt
git add job.txt
git commit -q -m 'from the sandbox'
git push -q origin HEAD:refs/heads/hyalos/fix
echo HYALOS_PUSHED
if git push -q origin HEAD:refs/heads/main; then echo HYALOS_OUTSIDE_PUSHED; else echo HYALOS_OUTSIDE_REFUSED; fi
";

fn dir(name: &str) -> Res<PathBuf> {
    let dir = std::env::temp_dir().join(format!("hyimg-git-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Runs the host's git with no configuration but this test's.
fn git(home: &Path, cwd: &Path, args: &[&str]) -> Res<Output> {
    Ok(Command::new("git")
        .args([
            "-c",
            "user.name=Hyalos Test",
            "-c",
            "user.email=test@hyalos.test",
        ])
        .args(["-c", "init.defaultBranch=main"])
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
            "git failed: {}{}",
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

/// `git http-backend` over the bare repositories under `root`, on loopback, answering only requests
/// that carry the job's token as GitHub expects it. Records each request's path.
fn server(root: &Path) -> Res<(u16, Arc<Mutex<Vec<String>>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (root, log) = (root.to_path_buf(), Arc::clone(&seen));
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (root, log) = (root.clone(), Arc::clone(&log));
            std::thread::spawn(move || {
                let _ = serve_one(stream, &root, &log);
            });
        }
    });
    Ok((port, seen))
}

fn serve_one(stream: TcpStream, root: &Path, log: &Mutex<Vec<String>>) -> Res {
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
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    if let Ok(mut log) = log.lock() {
        log.push(path.to_string());
    }
    let mut out = stream;
    if get("authorization").as_deref() != Some(expected.as_str()) {
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

/// A bare `octo/repo` under a new root, with one commit on `main`. Returns the root and that commit.
fn forge(home: &Path) -> Res<(PathBuf, String)> {
    let root = dir("root")?;
    let seed = home.join("seed");
    std::fs::create_dir_all(&seed)?;
    ok(git(home, &seed, &["init", "-q"])?)?;
    std::fs::write(seed.join("README"), "hello\n")?;
    ok(git(home, &seed, &["add", "README"])?)?;
    ok(git(home, &seed, &["commit", "-q", "-m", "first"])?)?;
    let main = ok(git(home, &seed, &["rev-parse", "HEAD"])?)?;
    let bare = root.join("octo/repo.git");
    let bare_text = bare.to_string_lossy().into_owned();
    ok(git(home, &seed, &["init", "-q", "--bare", &bare_text])?)?;
    ok(git(home, &seed, &["push", "-q", &bare_text, "main"])?)?;
    Ok((root, main))
}

fn rev(home: &Path, root: &Path, refname: &str) -> Option<String> {
    let out = git(
        home,
        &root.join("octo/repo.git"),
        &["rev-parse", "--verify", "-q", refname],
    )
    .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[test]
fn the_images_git_clones_and_pushes_through_the_git_proxy_and_nothing_else() -> Res {
    if !image_present()? {
        return Ok(());
    }
    if Command::new("git").arg("--version").output().is_err() {
        return Err("the git server needs the host's git".into());
    }
    let home = dir("home")?;
    let (root, main) = forge(&home)?;
    let (port, served) = server(&root)?;
    let resolv = dir("resolv")?.join("resolv.conf");
    std::fs::write(&resolv, "nameserver 127.0.0.1\n")?;
    let spec = || Spec {
        git: Git::with_base(&format!("http://127.0.0.1:{port}")),
        repo: "octo/repo",
        actions: &["read", "push"],
        forge_token: Some(TOKEN),
        ..Spec::default()
    };

    let job = job_spec(1, 40, spec())?;
    let (code, output) = sandboxed_over(&job.socket, &resolv, &[], &[], &["sh", "-c", SCRIPT])?;
    let asked: Vec<&str> = output.lines().filter_map(asked_name).collect();
    let requests = not_model_calls(&job);
    let reports: Vec<ToEngine> = job.reports.try_iter().collect();
    let pushed = rev(&home, &root, "refs/heads/hyalos/fix");
    let said = format!(
        "exit {code}, lookups {asked:?}, requests {requests:?}, reports {reports:?}, \
         hyalos/fix {pushed:?}, served {:?}",
        served.lock().map(|s| s.clone()).unwrap_or_default()
    );
    eprintln!("{said}");
    let mut failures = Vec::new();
    if !(code == 0 && output.contains("HYALOS_PUSHED") && output.contains("HYALOS_OUTSIDE_REFUSED"))
    {
        failures.push(format!(
            "the script did not clone, push and get refused: {said}\n{output}"
        ));
    }
    if !asked.is_empty() {
        failures.push(format!("git looked up a name: {asked:?}"));
    }
    // Every request is a git call on the forwarder, in origin form: none went to a proxy.
    if requests.is_empty()
        || !requests.iter().all(|r| {
            r.starts_with("GET /git/octo/repo.git/") || r.starts_with("POST /git/octo/repo.git/")
        })
    {
        failures.push(format!("a request was not a git call: {requests:?}"));
    }
    // The branch inside the prefix holds the sandbox's commit; main is untouched.
    let message = pushed.as_deref().and_then(|p| {
        ok(git(
            &home,
            &root.join("octo/repo.git"),
            &["log", "-1", "--format=%s", p],
        )
        .ok()?)
        .ok()
    });
    if message.as_deref() != Some("from the sandbox") {
        failures.push(format!(
            "hyalos/fix is not the sandbox's commit: {message:?}"
        ));
    }
    if rev(&home, &root, "refs/heads/main").as_deref() != Some(main.as_str()) {
        failures.push("main moved".into());
    }
    let refused = ToEngine::Gate {
        job: hyalos_types::JobId::from_seq(40),
        gate: Gate::Manifest,
        inputs: inputs_digest("refs/heads/main"),
        answer: Answer::Deny,
        reason: hyalos_types::ReasonCode::new("git.ref_not_allowed").ok_or("code")?,
    };
    if reports != vec![refused] {
        failures.push(format!(
            "the refused push was not reported once as the manifest gate's deny: {reports:?}"
        ));
    }

    // The control: without the forwarder's proxy setting and with NO_PROXY emptied, git sends the
    // clone to the proxy variables' address as a proxy request, which the broker refuses.
    let bare_config = dir("config")?.join("gitconfig");
    let shipped = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../images/sandbox/etc/git/gitconfig"
    ))?;
    let without: String = shipped
        .split("\n[")
        .filter(|section| !section.starts_with("http \"http://127.0.0.1:8089/\"]"))
        .collect::<Vec<_>>()
        .join("\n[");
    if without == shipped {
        failures.push("the control did not remove the forwarder's proxy setting".into());
    }
    std::fs::write(&bare_config, without)?;
    let job = job_spec(1, 41, spec())?;
    let (_, output) = sandboxed_over(
        &job.socket,
        &resolv,
        &[("NO_PROXY", ""), ("no_proxy", "")],
        &[(&bare_config, "/etc/gitconfig")],
        &[
            "sh",
            "-c",
            "cd /tmp && git clone -q http://127.0.0.1:8089/git/octo/repo.git work",
        ],
    )?;
    let requests = not_model_calls(&job);
    eprintln!("without the proxy setting: requests {requests:?}");
    if !requests
        .iter()
        .any(|r| r.starts_with("GET http://127.0.0.1:8089/git/"))
    {
        failures.push(format!(
            "the control's proxy request was not seen: {requests:?}\n{output}"
        ));
    }

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&home);
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    Ok(())
}
