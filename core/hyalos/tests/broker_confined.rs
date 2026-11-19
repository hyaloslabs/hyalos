//! ADR-0118 (#337): the broker confines itself, so a broker process cannot open `engine.key` or
//! `github-app.pem`, though its user can.
//!
//! The engine names the plugin store in each `RunPlugin`, and the broker reads every file of the skill
//! there. So a store whose skill directory holds the key files makes the broker try to read them. In
//! the store the engine granted, the read succeeds and the load fails later (`plugin.altered`, the
//! files are not the ones installed). In the config directory, it fails at once (`plugin.unreadable`).
//! The key files here are stand-ins made for the test.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use hyalos::clock::SystemClock;
use hyalos::launch::{BrokerProcess, fresh_manifest_key};
use hyalos_types::wire::{PluginCall, ToBroker, ToEngine};
use hyalos_types::workflow::Field;
use hyalos_types::{
    Budget, Clock, Digest, JobId, MANIFEST_VERSION, Manifest, ManifestKey, PluginName, Role,
    Timestamp,
};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

const EXE: &str = env!("CARGO_BIN_EXE_hyalos");
const WAIT: Duration = Duration::from_secs(20);

fn dir(name: &str) -> Res<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("bcf-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn manifest(job: u64) -> Manifest {
    Manifest {
        version: MANIFEST_VERSION,
        job: JobId::from_seq(job),
        role: Role::Deterministic,
        tainted: false,
        repo: "hyaloslabs/hyalos".into(),
        branch_prefix: "hyalos/".into(),
        pr_base: None,
        actions: BTreeSet::new(),
        egress_hosts: BTreeSet::new(),
        tools: BTreeSet::new(),
        data_flows: BTreeSet::new(),
        budget: Budget { tokens: 1 },
        deadline: Timestamp::from_millis(SystemClock.now().as_millis().saturating_add(3_600_000)),
        approval: None,
        handles: Default::default(),
        result: None,
        closes: None,
    }
}

/// A skill directory, under `store`, that holds the engine's key files beside its `SKILL.md`.
fn keys_as_a_skill(store: &Path, digest: &Digest) -> Res<PathBuf> {
    let skill = store.join(digest.to_string());
    std::fs::create_dir_all(&skill)?;
    std::fs::write(skill.join("SKILL.md"), "---\nname: probe\n---\nNothing.\n")?;
    for key in ["engine.key", "github-app.pem"] {
        std::fs::write(skill.join(key), "a stand-in, not a key\n")?;
        // The broker's user can read it: only the confinement stands in the way.
        std::fs::read(skill.join(key))?;
    }
    Ok(skill)
}

/// Admits deterministic job `job` and asks it to load the skill `digest` from `store`; the reason
/// the broker refuses it.
fn load(
    broker: &mut BrokerProcess,
    key: &ManifestKey,
    runtime: &Path,
    job: u64,
    store: &Path,
    digest: Digest,
) -> Res<String> {
    let m = manifest(job);
    let socket_dir = runtime.join(format!("j{job}"));
    std::fs::create_dir_all(&socket_dir)?;
    broker.send(&ToBroker::StartJob {
        job: m.job,
        role: m.role,
        manifest: key.sign(&m)?.to_bytes()?,
        socket: socket_dir.join("b.sock").to_string_lossy().into_owned(),
        forge_token: None,
    })?;
    assert_eq!(broker.recv()?, Some(ToEngine::Admitted { job: m.job }));
    broker.send(&ToBroker::RunPlugin {
        job: m.job,
        call: PluginCall {
            store: store.to_string_lossy().into_owned(),
            name: PluginName::new("probe").ok_or("name")?,
            digest,
            input: "{}".into(),
            output: BTreeMap::from([("ok".to_string(), Field::Bool)]),
        },
    })?;
    match broker.recv()? {
        Some(ToEngine::Refused { reason, .. }) => Ok(reason.as_str().to_string()),
        other => Err(format!("the load was not refused: {other:?}").into()),
    }
}

#[test]
fn a_broker_process_cannot_open_engine_key_or_github_app_pem() -> Res {
    if !hyalos::confine::available() {
        if std::env::var_os("HYALOS_REQUIRE_LANDLOCK").is_some() {
            return Err("HYALOS_REQUIRE_LANDLOCK is set and this kernel has no Landlock".into());
        }
        // Without Landlock no broker starts at all (ADR-0121), so none can read the keys.
        let d = dir("no-landlock")?;
        let started =
            BrokerProcess::launch(Path::new(EXE), &d, &fresh_manifest_key()?.public(), WAIT);
        assert!(matches!(
            started,
            Err(hyalos::launch::LaunchError::NoLandlock)
        ));
        return Ok(());
    }
    let d = dir("keys")?;
    let runtime = d.join("run");
    let plugins = d.join("plugins");
    let config = d.join("config");
    for dir in [&runtime, &plugins, &config] {
        std::fs::create_dir_all(dir)?;
    }
    let digest = Digest::from_bytes([7; 32]);
    keys_as_a_skill(&plugins, &digest)?;
    keys_as_a_skill(&config, &digest)?;

    let key = fresh_manifest_key()?;
    let mut broker = BrokerProcess::launch_confined(
        Path::new(EXE),
        &runtime,
        &key.public(),
        None,
        None,
        Some(&plugins),
        WAIT,
    )?;
    // In the store it was granted, the broker reads every file, the stand-in keys too.
    assert_eq!(
        load(&mut broker, &key, &runtime, 5, &plugins, digest)?,
        "plugin.altered"
    );
    // In the config directory, it can open none of them.
    assert_eq!(
        load(&mut broker, &key, &runtime, 6, &config, digest)?,
        "plugin.unreadable"
    );
    Ok(())
}

#[test]
fn the_engine_will_not_start_a_broker_that_could_read_the_config_directory() -> Res {
    let d = dir("guard")?;
    let runtime = d.join("run");
    let config = d.join("data/config");
    std::fs::create_dir_all(&runtime)?;
    std::fs::create_dir_all(&config)?;
    let keys = config.join("provider.keys");
    let key = fresh_manifest_key()?;
    // A plugin store that contains the directory of the provider key file, and so the engine's keys.
    let started = BrokerProcess::launch_confined(
        Path::new(EXE),
        &runtime,
        &key.public(),
        Some(&keys),
        None,
        Some(&d.join("data")),
        WAIT,
    );
    assert!(matches!(
        started,
        Err(hyalos::launch::LaunchError::Confinement)
    ));
    // The same through a link: Landlock would follow it, so the guard does too.
    let link = d.join("store-link");
    std::os::unix::fs::symlink(d.join("data"), &link)?;
    let started = BrokerProcess::launch_confined(
        Path::new(EXE),
        &runtime,
        &key.public(),
        Some(&keys),
        None,
        Some(&link),
        WAIT,
    );
    assert!(matches!(
        started,
        Err(hyalos::launch::LaunchError::Confinement)
    ));
    Ok(())
}
