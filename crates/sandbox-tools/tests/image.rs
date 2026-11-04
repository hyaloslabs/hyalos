//! S2.9: each harness in the sandbox image makes no lookup other than its model calls, which go
//! through the forwarder to the broker (ADR-0016, ADR-0040).
//!
//! It needs the image's `test` target, built from the repository root:
//!
//!     docker build -f images/sandbox/Containerfile --target test -t hyalos-sandbox:test .
//!
//! It skips if that image is missing, unless `HYALOS_REQUIRE_SANDBOX_IMAGE` is set (the
//! `sandbox image` CI workflow sets it). The broker is the real one, run in this process, with both
//! providers pointed at a scripted model on loopback, so no key and no model is involved. Each
//! harness runs with the container substrate's isolation (no network, read-only root, an empty
//! `/tmp`, an unprivileged user, no capabilities) under `hyalos-dns-log`, which answers DNS on the
//! sandbox's loopback and lists every name asked.

mod common;

use common::{Res, asked_name, image_present, job, not_model_calls, sandboxed, scripted_model};

#[test]
fn every_harness_in_the_image_makes_no_lookup_and_reaches_its_model_through_the_broker() -> Res {
    if !image_present()? {
        return Ok(());
    }
    let model = scripted_model()?;
    let port = model.port;
    let resolv = std::env::temp_dir().join(format!("hyimg-{}-resolv.conf", std::process::id()));
    std::fs::write(&resolv, "nameserver 127.0.0.1\n")?;

    let harnesses: [(&str, &[&str], &str); 4] = [
        (
            "Claude Code",
            &["claude", "-p", "say ok", "--tools", "Bash"],
            "/v1/messages",
        ),
        (
            "Codex",
            &["codex", "exec", "--skip-git-repo-check", "say ok"],
            "/v1/responses",
        ),
        (
            "Claude Agent SDK",
            &["node", "/opt/hyalos/harness/test/claude-sdk.mjs"],
            "/v1/messages",
        ),
        (
            "OpenAI Agents SDK",
            &["node", "/opt/hyalos/harness/test/openai-sdk.mjs"],
            "/v1/responses",
        ),
    ];
    let mut failures = Vec::new();
    for (seq, (name, command, path)) in (1u64..).zip(harnesses) {
        let job = job(port, seq)?;
        model.clear();
        let (code, output) = sandboxed(&job.socket, &resolv, command)?;
        let asked: Vec<&str> = output.lines().filter_map(asked_name).collect();
        let reached = model.reached(path);
        let ran = output.contains("hyalos-dns-log: done");
        let other = not_model_calls(&job);
        let said = format!(
            "{name}: exit {code}, lookups {asked:?}, other requests {other:?}, model reached {reached}"
        );
        eprintln!("{said}");
        if code != 0 || !asked.is_empty() || !other.is_empty() || !reached || !ran {
            failures.push(format!("{said}\n{output}"));
        }
    }

    // The controls: with one of the image's settings undone, the tracking call it prevents (Codex's
    // analytics host, spike #72) is seen, so the empty results above mean none was made. Through
    // the proxy variables it arrives on the channel as a request to that host; without them it is a
    // lookup the DNS log sees.
    let analytics = [
        "codex",
        "exec",
        "--skip-git-repo-check",
        "-c",
        "analytics.enabled=true",
        "say ok",
    ];
    let job_a = job(port, 9)?;
    let (_, output) = sandboxed(&job_a.socket, &resolv, &analytics)?;
    let control = not_model_calls(&job_a);
    eprintln!("Codex with analytics on: other requests {control:?}");
    if !control.iter().any(|r| r.contains("chatgpt.com")) {
        failures.push(format!(
            "the channel control's tracking call was not seen\n{output}"
        ));
    }
    let job_b = job(port, 10)?;
    let unproxied: Vec<&str> = ["env", "-u", "HTTPS_PROXY", "-u", "https_proxy"]
        .into_iter()
        .chain(["-u", "HTTP_PROXY", "-u", "http_proxy"])
        .chain(analytics)
        .collect();
    let (_, output) = sandboxed(&job_b.socket, &resolv, &unproxied)?;
    let asked: Vec<&str> = output.lines().filter_map(asked_name).collect();
    eprintln!("Codex with analytics on and no proxy: lookups {asked:?}");
    if !asked.iter().any(|n| n.contains("chatgpt.com")) {
        failures.push(format!("the DNS control saw no lookup\n{output}"));
    }
    // And for Claude Code: with its non-essential traffic allowed again, the call that setting
    // prevents arrives on the channel as a request to Anthropic's host. The other three Claude
    // settings stay set, so this shows this one setting takes effect.
    let job_c = job(port, 11)?;
    let (_, output) = sandboxed(
        &job_c.socket,
        &resolv,
        &[
            "env",
            "-u",
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
            "claude",
            "-p",
            "say ok",
            "--tools",
            "Bash",
        ],
    )?;
    let control = not_model_calls(&job_c);
    eprintln!("Claude Code with non-essential traffic on: other requests {control:?}");
    if !control
        .iter()
        .any(|r| r.starts_with("CONNECT api.anthropic.com:"))
    {
        failures.push(format!(
            "the Claude Code control's call was not seen\n{output}"
        ));
    }
    let _ = std::fs::remove_file(&resolv);
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    Ok(())
}
