//! Starts the built programs as an operator and a customer would. Every key and credential here
//! is made up, in a directory of the test's own.

#![allow(dead_code)]

use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Output, Stdio},
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};

pub const NAME: &str = "provider.test";
pub const INTENT: &str = "5f1d0c7a9b3e4f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8";
/// The task the rigs' notaries authorise. The notary does not compare it with the run.
pub const TASK: &str = "7f1d0c7a9b3e4f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8";
pub const OTHER_RUN: &str = "6f1d0c7a9b3e4f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8";

/// A session costs seconds and several hundred megabytes, so tests take them one at a time.
pub fn one_at_a_time() -> MutexGuard<'static, ()> {
    static ONE: Mutex<()> = Mutex::new(());
    ONE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A fresh directory under the build's scratch space, removed when dropped.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(label: &str) -> Self {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Ends a program however the test ends.
pub struct Running(pub Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub struct Standin {
    pub root: PathBuf,
    pub address: String,
    _child: Running,
}

/// Starts a stand-in provider for `NAME`, with the given controls.
pub fn standin(dir: &Path, controls: &[&str]) -> Standin {
    let root = dir.join("root.der");
    let mut child = Command::new(env!("CARGO_BIN_EXE_standin"))
        .arg(&root)
        .arg(NAME)
        .args(controls)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let address = line
        .trim()
        .strip_prefix("listening ")
        .expect("the stand-in did not start")
        .to_string();
    Standin {
        root,
        address,
        _child: Running(child),
    }
}

pub struct Notary {
    pub dir: PathBuf,
    pub signing: String,
    pub transport: String,
    pub address: String,
    /// The authorisation `notary` made or found. Empty from `try_notary`.
    pub authorisation: String,
    _stdin: Option<ChildStdin>,
    _child: Running,
}

impl Notary {
    pub fn lines(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.join("sessions.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// Waits for the session line that follows the `seen` already there.
    pub fn line_after(&self, seen: usize) -> String {
        let started = Instant::now();
        loop {
            if let Some(line) = self.lines().get(seen) {
                return line.clone();
            }
            assert!(
                started.elapsed() < Duration::from_secs(60),
                "no session line was written"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// Starts a notary, or returns what it said when it refused to start.
pub fn try_notary(dir: &Path, operator: &str, root: &Path) -> Result<Notary, String> {
    canary_pass(dir);
    start_notary(
        Command::new(env!("CARGO_BIN_EXE_notary")),
        dir,
        operator,
        root,
    )
}

/// Writes the customer's provider check state, as `canary` would.
pub fn write_canary(dir: &Path, word: &str, checked: u64) {
    write_canary_text(
        dir,
        &format!("hyalos.canary/1\nstate {word}\nchecked {checked}\nend\n"),
    );
}

pub fn write_canary_text(dir: &Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("canary");
    std::fs::write(&path, text).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// A fresh pass, unless the directory already holds a state. A path that cannot be a directory is
/// left for the notary to refuse.
pub fn canary_pass(dir: &Path) {
    if std::fs::create_dir_all(dir).is_ok() && !dir.join("canary").exists() {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        write_canary(dir, "passed", unix_now());
    }
}

/// Starts the given notary program on a directory as it stands.
pub fn start_notary(
    mut program: Command,
    dir: &Path,
    operator: &str,
    root: &Path,
) -> Result<Notary, String> {
    let mut child = program
        .args([
            "--listen",
            "127.0.0.1:0",
            "--approved-server",
            NAME,
            "--stop-on-stdin-close",
            "--operator-transport-public",
            operator,
            "--dir",
        ])
        .arg(dir)
        .arg("--root")
        .arg(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take();
    let mut printed = BufReader::new(child.stdout.take().unwrap());
    let mut field = |name: &str| {
        let mut line = String::new();
        printed.read_line(&mut line).unwrap();
        line.trim()
            .strip_prefix(name)
            .map(|value| value.trim().to_string())
    };
    let (Some(signing), Some(transport), Some(address)) =
        (field("signing"), field("transport"), field("listening"))
    else {
        drop(stdin);
        let mut said = String::new();
        std::io::Read::read_to_string(child.stderr.as_mut().unwrap(), &mut said).unwrap();
        let _ = child.wait();
        return Err(said);
    };
    Ok(Notary {
        dir: dir.to_path_buf(),
        signing,
        transport,
        address,
        authorisation: String::new(),
        _stdin: stdin,
        _child: Running(child),
    })
}

pub fn notary(dir: &Path, operator: &str, root: &Path) -> Notary {
    let mut started = try_notary(dir, operator, root).expect("the notary did not start");
    started.authorisation = authorised(dir);
    started
}

/// The test-only notary, from the release build the slice's Verify makes first, with a passing state.
pub fn forging_notary(
    dir: &Path,
    operator: &str,
    root: &Path,
    settings: &[(&str, &str)],
) -> Notary {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/release/forging-notary");
    assert!(
        path.exists(),
        "forging-notary is not built: cargo build --release --manifest-path hyalos-os/Cargo.toml -p attest --features forge-ticket --bin forging-notary"
    );
    canary_pass(dir);
    let mut program = Command::new(path);
    program.envs(settings.iter().copied());
    let mut started = start_notary(program, dir, operator, root).expect("the notary did not start");
    started.authorisation = authorised(dir);
    started
}

/// The directory's one authorisation, made with a generous budget on first use.
fn authorised(dir: &Path) -> String {
    if let Ok(mut held) = std::fs::read_dir(dir.join("authorisations")) {
        if let Some(entry) =
            held.find(|entry| entry.as_ref().is_ok_and(|e| e.file_name().len() == 64))
        {
            return entry.unwrap().file_name().into_string().unwrap();
        }
    }
    let made = Command::new(env!("CARGO_BIN_EXE_notary"))
        .args(["authorize", "--dir"])
        .arg(dir)
        .args(["--task", TASK, "--attempts", "100"])
        .output()
        .unwrap();
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    String::from_utf8(made.stdout).unwrap().trim().to_string()
}

/// An operator's home: a made-up credential at the pinned length, and a transport key.
pub struct Operator {
    pub home: PathBuf,
    pub key: PathBuf,
    pub public: String,
}

pub fn operator(home: &Path) -> Operator {
    let config = home.join(".config/hyalos-spike");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("anthropic.key"),
        format!("{}\n", "x".repeat(106)),
    )
    .unwrap();
    let key = home.join("operator-transport.key");
    let made = Command::new(env!("CARGO_BIN_EXE_attest"))
        .arg("link-key")
        .arg(&key)
        .output()
        .unwrap();
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    Operator {
        home: home.to_path_buf(),
        key,
        public: String::from_utf8(made.stdout).unwrap().trim().to_string(),
    }
}

/// The operator's tool, with test trust settings and the notary's address and public keys.
pub fn tool(
    who: &Operator,
    root: &Path,
    provider: &str,
    address: &str,
    transport: &str,
    signing: &str,
    authorisation: &str,
) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_attest"));
    command
        .env("HOME", &who.home)
        .env("HYALOS_TRUST_SERVER_NAME", NAME)
        .env("HYALOS_TRUST_ROOT", root)
        .env("HYALOS_TRUST_PROVIDER_ADDRESS", provider)
        .env("HYALOS_NOTARY_ADDRESS", address)
        .env("HYALOS_NOTARY_TRANSPORT_PUBLIC", transport)
        .env("HYALOS_NOTARY_SIGNER_PUBLIC", signing)
        .env("HYALOS_NOTARY_AUTHORISATION", authorisation)
        .env("HYALOS_OPERATOR_TRANSPORT_KEY", &who.key);
    command
}

/// Runs one `ask`, and fails the test rather than hang when it does not end.
pub fn run_ask(mut command: Command, file: &Path, intent: &str, position: u64) -> Output {
    let mut child = command
        .arg("ask")
        .arg(file)
        .args([intent, &position.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"The build fails on a clean checkout")
        .unwrap();
    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > Duration::from_secs(120) {
            let _ = child.kill();
            panic!("the attested call did not finish in 120 seconds");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    child.wait_with_output().unwrap()
}

/// The one word ask printed on a refusal.
pub fn word(output: &Output) -> String {
    assert!(!output.status.success(), "ask did not refuse");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

pub fn said(output: &Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "ask failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

pub fn verify(
    who: &Operator,
    root: &Path,
    file: &Path,
    signing: &str,
) -> Result<serde_json::Value, String> {
    let output = tool(who, root, "127.0.0.1:9", "127.0.0.1:9", "00", signing, "")
        .arg("verify")
        .arg(file)
        .arg(signing)
        .output()
        .unwrap();
    if output.status.success() {
        Ok(serde_json::from_slice(&output.stdout).unwrap())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}
