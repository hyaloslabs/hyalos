//! S3.2: a job calls a model through the broker, which adds the key, and no provider key appears in
//! the sandbox's environment, files or memory dump (ADR-0002, ADR-0016, ADR-0038).
//!
//! The real `hyalos broker` is launched with an owner-only key file pointing at a local
//! OpenAI-compatible server. The job's sandbox is this test executable, started by the local-process
//! substrate with only a placeholder key; it calls the model through the job's socket, then writes
//! out its environment, everything it received and a dump of its own memory. The key is drawn at
//! run time, so it is in no binary, and the test looks for it in all of that.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use hyalos::launch::{BrokerProcess, JobDir, fresh_manifest_key};
use hyalos_substrate::{SandboxSpec, Substrate};
use hyalos_substrate_contract::LocalProcessSubstrate;
use hyalos_types::wire::{ToBroker, ToEngine};
use hyalos_types::{Budget, JobId, MANIFEST_VERSION, Manifest, Role, Timestamp};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

const EXE: &str = env!("CARGO_BIN_EXE_hyalos");
const WAIT: Duration = Duration::from_secs(20);
const PLACEHOLDER: &str = "placeholder-not-a-key";

fn dir(name: &str) -> Res<PathBuf> {
    // Short, so the job's socket path stays under the Unix limit.
    let dir = std::env::temp_dir().join(format!("hymc-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn random_key() -> Res<String> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    Ok(format!(
        "sk-test-{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ))
}

/// A local model server: answers one call with usage and sends back the credential it was given.
fn provider() -> Res<(u16, mpsc::Receiver<String>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut reader = BufReader::new(&stream);
        let (mut auth, mut length) = (String::new(), 0usize);
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                break;
            }
            if let Some((n, v)) = line.split_once(':') {
                match n.trim().to_ascii_lowercase().as_str() {
                    "authorization" => auth = v.trim().to_string(),
                    "content-length" => length = v.trim().parse().unwrap_or(0),
                    _ => {}
                }
            }
        }
        let mut body = vec![0; length];
        let _ = reader.read_exact(&mut body);
        let answer =
            r#"{"object":"response","output":[],"usage":{"input_tokens":12,"output_tokens":3}}"#;
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{answer}",
            answer.len()
        );
        let _ = tx.send(auth);
    });
    Ok((port, rx))
}

fn manifest(job: u64) -> Manifest {
    Manifest {
        version: MANIFEST_VERSION,
        job: JobId::from_seq(job),
        role: Role::Actor,
        tainted: false,
        repo: "hyaloslabs/hyalos".into(),
        branch_prefix: "hyalos/".into(),
        pr_base: None,
        actions: BTreeSet::new(),
        egress_hosts: BTreeSet::new(),
        tools: BTreeSet::new(),
        data_flows: hyalos_types::flow::ALL.map(String::from).into(),
        budget: Budget { tokens: 10_000 },
        deadline: Timestamp::from_millis(u64::MAX / 2),
        approval: None,
        handles: Default::default(),
        result: None,
    }
}

#[test]
fn a_runtime_directory_too_long_for_a_socket_is_refused_at_job_start() -> Res {
    let root = dir("long")?;
    // The job directory adds 44 bytes and the socket 12, so this leaves the socket over 100.
    let long = root.join("r".repeat(60));
    std::fs::create_dir(&long)?;
    let error = JobDir::create(&long)
        .err()
        .ok_or("a too-long runtime directory was taken")?;
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    let said = error.to_string();
    assert!(
        said.contains("too long for a job's socket") && said.contains("at most 100"),
        "{said}"
    );
    assert_eq!(std::fs::read_dir(&long)?.count(), 0, "nothing was made");
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Every byte of every file under `dir`.
fn files(dir: &Path) -> Res<Vec<(PathBuf, Vec<u8>)>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            out.extend(files(&path)?);
        } else {
            out.push((path.clone(), std::fs::read(&path)?));
        }
    }
    Ok(out)
}

#[test]
fn no_provider_key_reaches_the_sandbox() -> Res {
    let key = random_key()?;
    let (port, credential) = provider()?;
    let root = dir("leak")?;
    let config = root.join("config");
    std::fs::create_dir(&config)?;
    let keys = config.join("provider.keys");
    std::fs::write(
        &keys,
        format!("openai.key = {key}\nopenai.base = http://127.0.0.1:{port}\n"),
    )?;
    std::fs::set_permissions(&keys, std::fs::Permissions::from_mode(0o600))?;
    let run = root.join("run");
    std::fs::create_dir(&run)?;
    let out = root.join("out");
    std::fs::create_dir(&out)?;
    // The sandbox runs as another user in a real substrate; here it only writes its dumps.
    std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o777))?;

    let manifest_key = fresh_manifest_key()?;
    let mut broker = BrokerProcess::launch_with(
        Path::new(EXE),
        &run,
        &manifest_key.public(),
        Some(&keys),
        WAIT,
    )?;
    let job_dir = JobDir::create(&run)?;
    assert_eq!(
        std::fs::metadata(job_dir.path())?.permissions().mode() & 0o777,
        0o711
    );
    let m = manifest(7);
    broker.send(&ToBroker::StartJob {
        job: m.job,
        role: m.role,
        manifest: manifest_key.sign(&m)?.to_bytes()?,
        socket: job_dir.socket().to_string_lossy().into_owned(),
        forge_token: None,
    })?;
    assert_eq!(broker.recv()?, Some(ToEngine::Admitted { job: m.job }));

    let me = std::env::current_exe()?;
    let mut spec = SandboxSpec::new(
        m.job,
        "harness",
        vec![
            me.to_string_lossy().into_owned(),
            "harness_entry".into(),
            "--exact".into(),
            "--nocapture".into(),
        ],
        job_dir.socket(),
    );
    spec.env.insert(
        "HARNESS_SOCKET".into(),
        job_dir.socket().to_string_lossy().into_owned(),
    );
    spec.env
        .insert("HARNESS_OUT".into(), out.to_string_lossy().into_owned());
    // What a real sandbox image sets: the harness's own placeholder, never the key.
    spec.env.insert("OPENAI_API_KEY".into(), PLACEHOLDER.into());
    spec.env
        .insert("OPENAI_BASE_URL".into(), "http://127.0.0.1:8089/v1".into());
    spec.deadline = Duration::from_secs(60);
    let outcome = LocalProcessSubstrate::new("harness").start(&spec)?.wait();

    let status = std::fs::read_to_string(out.join("status")).unwrap_or_default();
    assert_eq!(
        status,
        "200",
        "the call went through: {}",
        String::from_utf8_lossy(&outcome.stderr)
    );
    assert_eq!(
        credential.recv_timeout(WAIT)?,
        format!("Bearer {key}"),
        "the provider was sent the real key"
    );

    let dumped = files(&out)?;
    let names: BTreeSet<_> = dumped
        .iter()
        .filter_map(|(p, _)| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    assert_eq!(
        names,
        ["environ", "memory", "response", "status"]
            .map(String::from)
            .into(),
        "the sandbox wrote everything it was asked to"
    );
    for (path, bytes) in &dumped {
        assert!(
            !contains(bytes, key.as_bytes()),
            "the provider key is in the sandbox's {}",
            path.display()
        );
    }
    // The scan can see what it looks for: the placeholder the harness held is in its memory.
    let memory = dumped
        .iter()
        .find(|(p, _)| p.ends_with("memory"))
        .ok_or("memory")?;
    assert!(
        memory.1.len() > 1024 * 1024,
        "a real dump, not an empty file"
    );
    assert!(contains(&memory.1, PLACEHOLDER.as_bytes()));
    assert!(
        !contains(&outcome.stdout, key.as_bytes()) && !contains(&outcome.stderr, key.as_bytes())
    );

    broker.send(&ToBroker::EndJob { job: m.job })?;
    drop(broker);
    drop(job_dir);
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

/// The sandbox's program. It does nothing unless the test above started it.
#[test]
fn harness_entry() -> Res {
    let (Ok(socket), Ok(out)) = (
        std::env::var("HARNESS_SOCKET"),
        std::env::var("HARNESS_OUT"),
    ) else {
        return Ok(());
    };
    let out = PathBuf::from(out);
    let placeholder = std::env::var("OPENAI_API_KEY")?;

    // The forwarder's job: one model call over the broker's socket, with the placeholder.
    let body = br#"{"model":"local","input":"hello","tools":[{"type":"web_search"}]}"#;
    let mut stream = UnixStream::connect(&socket)?;
    write!(
        stream,
        "POST /v1/responses HTTP/1.1\r\nhost: localhost\r\nauthorization: Bearer {placeholder}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    let status = String::from_utf8_lossy(&response)
        .split(' ')
        .nth(1)
        .unwrap_or_default()
        .to_string();
    std::fs::write(out.join("response"), &response)?;
    std::fs::write(out.join("status"), status)?;

    let environ: String = std::env::vars()
        .map(|(k, v)| format!("{k}={v}\n"))
        .collect();
    std::fs::write(out.join("environ"), environ)?;

    // A dump of every readable mapping of this process.
    let mut dump = std::fs::File::create(out.join("memory"))?;
    let mut mem = std::fs::File::open("/proc/self/mem")?;
    let maps = std::fs::read_to_string("/proc/self/maps")?;
    for line in maps.lines() {
        let mut fields = line.split_whitespace();
        let (Some(range), Some(perms)) = (fields.next(), fields.next()) else {
            continue;
        };
        if !perms.starts_with('r') || line.contains("[vvar") || line.contains("[vsyscall]") {
            continue;
        }
        let Some((start, end)) = range.split_once('-') else {
            continue;
        };
        let (start, end) = (
            u64::from_str_radix(start, 16)?,
            u64::from_str_radix(end, 16)?,
        );
        let mut region = vec![0u8; usize::try_from(end - start)?];
        if mem.seek(SeekFrom::Start(start)).is_ok() && mem.read_exact(&mut region).is_ok() {
            dump.write_all(&region)?;
        }
    }
    Ok(())
}
