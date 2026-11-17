//! S7.6: an air-gapped install from an offline bundle (ADR-0007, ADR-0090, ADR-0099).
//!
//! The bundle is `fixtures/marketplace-r10`, the files of release `r10` of hyaloslabs/marketplace as
//! published: a skill, the trusted-root snapshot and the revocation list, each with the pipeline's
//! Sigstore bundle. The binary verifies them against the snapshot it embeds, as for any install, with
//! no test trust. Once on the host, and once in a container with no network, and both record the same
//! evidence.
//!
//! The container test skips without a runtime that has `ubuntu:24.04`, unless
//! `HYALOS_REQUIRE_CONTAINER` is set, as CI sets it.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use hyalos_types::Event;

mod common;

use common::{EXE, PASSPHRASE, Res, World, ok_cli};

const BUNDLE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/marketplace-r10"
);
const IMAGE: &str = "ubuntu:24.04";

/// The bundle copied into `world`, as an operator would carry it in.
fn carried(world: &World) -> Res<String> {
    let to = world.root.join("bundle");
    std::fs::create_dir(&to)?;
    for entry in std::fs::read_dir(BUNDLE)? {
        let entry = entry?;
        std::fs::copy(entry.path(), to.join(entry.file_name()))?;
    }
    Ok(to.to_string_lossy().into_owned())
}

/// The fixture was signed on one day and stays in the repository: the operator's signed policy
/// widens the ages, as they may, so the test does not expire.
fn widen_ages(world: &World) -> Res {
    let ceiling = world.root.join("ceiling.json");
    std::fs::write(&ceiling, r#"{"version":1}"#)?;
    ok_cli(world.signed(&[
        "plugin",
        "policy",
        "set",
        "--ceiling",
        &ceiling.to_string_lossy(),
        "--max-snapshot-age-days",
        "3650",
        "--max-revocation-age-days",
        "3650",
    ])?)?;
    Ok(())
}

/// What an install recorded: each plugin event's kind and the digest it names.
fn evidence(world: &World) -> Res<Vec<(String, String)>> {
    Ok(world
        .events()?
        .iter()
        .filter_map(|e| match e {
            Event::PluginSnapshotImported { digest, .. }
            | Event::PluginRevocationsImported { digest, .. }
            | Event::PluginInstalled { digest, .. } => {
                Some((e.kind().to_string(), digest.to_string()))
            }
            _ => None,
        })
        .collect())
}

const INSTALL: [&str; 5] = ["plugin", "install", "--index", "", "hello"];

fn install_args(at: &str) -> Vec<&str> {
    let mut args = INSTALL.to_vec();
    args[3] = at;
    args
}

#[test]
fn a_release_directory_installs_with_the_pipelines_own_signatures() -> Res {
    let world = World::new("offline-host", None)?;
    widen_ages(&world)?;
    let at = carried(&world)?;
    let said = ok_cli(world.signed(&install_args(&at))?)?;
    assert!(said.contains("version 0.1.0 from"), "{said}");
    // The list is imported and the skill installed. The snapshot is imported too while the one
    // this build embeds is older than r10's; a later build embeds a newer one and imports nothing.
    let kinds: Vec<String> = evidence(&world)?.into_iter().map(|(k, _)| k).collect();
    let kinds: Vec<&str> = kinds
        .iter()
        .map(String::as_str)
        .filter(|k| *k != "plugin.snapshot_imported")
        .collect();
    assert_eq!(kinds, ["plugin.revocations_imported", "plugin.installed"]);
    // An archive changed in the bundle is refused, as online.
    let archive = Path::new(&at).join("hello-0.1.0.skill");
    let mut bytes = std::fs::read(&archive)?;
    bytes[40] ^= 1;
    std::fs::write(&archive, bytes)?;
    let out = world.signed(&install_args(&at))?;
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("plugin.digest_mismatch"));
    Ok(())
}

/// A container runtime with [`IMAGE`], or `None`.
fn runtime() -> Option<&'static str> {
    ["docker", "podman"].into_iter().find(|rt| {
        Command::new(rt)
            .args(["image", "inspect", IMAGE])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

fn id(flag: &str) -> Res<String> {
    let out = Command::new("id").arg(flag).output()?;
    Ok(String::from_utf8(out.stdout)?.trim().to_string())
}

/// `hyalos <args> --passphrase-stdin` in a container with no network, the world's directories
/// mounted at the same paths and the binary built for the tests.
fn in_container(rt: &str, world: &World, args: &[&str]) -> Res<Output> {
    let root = world.root.to_string_lossy().into_owned();
    let user = format!("{}:{}", id("-u")?, id("-g")?);
    let mut child = Command::new(rt)
        .args(["run", "--rm", "-i", "--network", "none", "--user", &user])
        // Rootless podman maps the host's uid to root unless told to keep it.
        .args(if rt == "podman" {
            &["--userns=keep-id"][..]
        } else {
            &[]
        })
        .args(["-v", &format!("{root}:{root}")])
        .args(["-v", &format!("{EXE}:/usr/local/bin/hyalos:ro")])
        .args([
            "-e",
            &format!("HYALOS_CONFIG_DIR={}", world.config.display()),
        ])
        .args(["-e", &format!("HYALOS_DATA_DIR={}", world.data.display())])
        .args([
            "-e",
            &format!("HYALOS_RUNTIME_DIR={}", world.runtime.display()),
        ])
        .arg(IMAGE)
        .arg("/usr/local/bin/hyalos")
        .args(args)
        .arg("--passphrase-stdin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = writeln!(stdin, "{PASSPHRASE}");
    }
    Ok(child.wait_with_output()?)
}

#[test]
fn with_no_network_a_bundle_verifies_the_same_way_as_online() -> Res {
    let required = std::env::var_os("HYALOS_REQUIRE_CONTAINER").is_some();
    let Some(rt) = runtime() else {
        assert!(
            !required,
            "no container runtime with {IMAGE} on this machine"
        );
        eprintln!("skipped: no container runtime with {IMAGE} on this machine");
        return Ok(());
    };
    let host = World::new("offline-online", None)?;
    widen_ages(&host)?;
    let at = carried(&host)?;
    ok_cli(host.signed(&install_args(&at))?)?;

    let air = World::new("offline-air-gapped", None)?;
    widen_ages(&air)?;
    let at = carried(&air)?;
    // The container really has no network: nothing outside it answers.
    let probe = Command::new(rt)
        .args([
            "run",
            "--rm",
            "--network",
            "none",
            IMAGE,
            "cat",
            "/proc/net/route",
        ])
        .output()?;
    let routes = String::from_utf8_lossy(&probe.stdout);
    assert_eq!(
        routes.lines().count(),
        1,
        "a route in the container: {routes}"
    );
    let out = in_container(rt, &air, &install_args(&at))?;
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(evidence(&air)?, evidence(&host)?);
    Ok(())
}
