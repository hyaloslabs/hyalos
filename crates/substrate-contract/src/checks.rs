//! The checks themselves. Each takes the driver and the fixture and says what is wrong, if anything.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hyalos_substrate::{
    BROKER_SOCKET_IN_SANDBOX, Ending, OUTPUT_LIMIT, Outcome, SandboxSpec, StartError, Substrate,
};
use hyalos_types::JobId;

use crate::{Fixture, Report};

type Check = fn(&dyn Substrate, &Ctx) -> Result<(), String>;

/// A stand-in for the broker: answers `ping` with `pong` on a socket anyone may connect to.
struct Broker {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    connections: Arc<AtomicUsize>,
    thread: Option<JoinHandle<()>>,
}

impl Broker {
    fn start(fx: &Fixture) -> Result<Self, String> {
        let path = fx.work_dir.join("broker.sock");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).map_err(|e| format!("bind {path:?}: {e}"))?;
        // The sandbox runs as an unprivileged user, so it must be able to reach the socket.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666))
            .and_then(|()| {
                std::fs::set_permissions(&fx.work_dir, std::fs::Permissions::from_mode(0o755))
            })
            .map_err(|e| format!("chmod: {e}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("nonblocking: {e}"))?;
        let stop = Arc::new(AtomicBool::new(false));
        let connections = Arc::new(AtomicUsize::new(0));
        let (s, c) = (stop.clone(), connections.clone());
        let thread = std::thread::spawn(move || {
            while !s.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        c.fetch_add(1, Ordering::SeqCst);
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                        let mut line = String::new();
                        if BufReader::new(&stream).read_line(&mut line).is_ok() && line == "ping\n"
                        {
                            let _ = stream.write_all(b"pong\n");
                        }
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        Ok(Self {
            path,
            stop,
            connections,
            thread: Some(thread),
        })
    }
}

impl Drop for Broker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

struct Ctx {
    fx: Fixture,
    broker: Broker,
}

impl Ctx {
    fn spec(&self, command: &[&str]) -> SandboxSpec {
        let mut spec = SandboxSpec::new(
            JobId::from_seq(1),
            &self.fx.image,
            command.iter().map(|s| s.to_string()).collect(),
            self.broker.path.clone(),
        );
        spec.deadline = Duration::from_secs(60);
        spec
    }

    fn sh(&self, script: &str) -> SandboxSpec {
        self.spec(&["sh", "-c", script])
    }
}

fn run_to_end(substrate: &dyn Substrate, spec: &SandboxSpec) -> Result<Outcome, String> {
    let mut sandbox = substrate.start(spec).map_err(|e| e.to_string())?;
    Ok(sandbox.wait())
}

fn expect(what: &str, got: &Outcome, ending: Ending, stdout: &str) -> Result<(), String> {
    if got.ending != ending || got.stdout_text() != stdout {
        return Err(format!(
            "{what}: expected {ending:?} with stdout {stdout:?}, got {:?} with stdout {:?} and stderr {:?}",
            got.ending,
            got.stdout_text(),
            String::from_utf8_lossy(&got.stderr)
        ));
    }
    Ok(())
}

// ---- behaviour ------------------------------------------------------------------------------

fn exit_code(s: &dyn Substrate, c: &Ctx) -> Result<(), String> {
    for code in [0, 1, 7, 255] {
        let out = run_to_end(s, &c.sh(&format!("exit {code}")))?;
        expect("exit code", &out, Ending::Exited(code), "")?;
    }
    Ok(())
}

fn output(s: &dyn Substrate, c: &Ctx) -> Result<(), String> {
    let out = run_to_end(s, &c.sh("echo out; echo err >&2"))?;
    expect("output", &out, Ending::Exited(0), "out\n")?;
    if out.stderr != b"err\n" {
        return Err(format!("stderr was {:?}", out.stderr));
    }
    Ok(())
}

fn output_is_bounded(s: &dyn Substrate, c: &Ctx) -> Result<(), String> {
    let out = run_to_end(
        s,
        &c.sh("yes x | head -c 400000; yes y | head -c 400000 >&2"),
    )?;
    if out.ending != Ending::Exited(0) {
        return Err(format!("it did not finish: {:?}", out.ending));
    }
    if out.stdout.len() != OUTPUT_LIMIT || out.stderr.len() != OUTPUT_LIMIT {
        return Err(format!(
            "kept {} and {} bytes, expected {OUTPUT_LIMIT} of each",
            out.stdout.len(),
            out.stderr.len()
        ));
    }
    Ok(())
}

/// Names a sandbox may see that are not the spec's: the ones a runtime or a shell sets itself.
const RUNTIME_SETS: &[&str] = &[
    "PATH",
    "HOME",
    "HOSTNAME",
    "PWD",
    "OLDPWD",
    "SHLVL",
    "_",
    "TERM",
    "LANG",
    "LC_ALL",
    "container",
];

fn environment(s: &dyn Substrate, c: &Ctx) -> Result<(), String> {
    let mut spec = c.spec(&["env"]);
    spec.env.insert("HYALOS_CONTRACT".into(), "a b=c".into());
    let out = run_to_end(s, &spec)?;
    let text = out.stdout_text();
    if !text.lines().any(|l| l == "HYALOS_CONTRACT=a b=c") {
        return Err(format!("the spec's variable is missing:\n{text}"));
    }
    let host: Vec<String> = std::env::vars().map(|(k, _)| k).collect();
    for line in text.lines() {
        let name = line.split('=').next().unwrap_or_default();
        if name != "HYALOS_CONTRACT"
            && !RUNTIME_SETS.contains(&name)
            && host.iter().any(|h| h == name)
        {
            return Err(format!("the host's {name} reached the sandbox"));
        }
    }
    Ok(())
}

fn no_shell(s: &dyn Substrate, c: &Ctx) -> Result<(), String> {
    let out = run_to_end(s, &c.spec(&["echo", "$HOME; touch /tmp/x `id` && echo"]))?;
    expect(
        "arguments",
        &out,
        Ending::Exited(0),
        "$HOME; touch /tmp/x `id` && echo\n",
    )
}

fn deadline(s: &dyn Substrate, c: &Ctx) -> Result<(), String> {
    let mut spec = c.sh("sleep 60");
    spec.deadline = Duration::from_secs(1);
    let began = Instant::now();
    let out = run_to_end(s, &spec)?;
    if out.ending != Ending::DeadlineExceeded {
        return Err(format!("ended {:?}", out.ending));
    }
    let took = began.elapsed();
    if took < Duration::from_millis(900) || took > Duration::from_secs(20) {
        return Err(format!("killed after {took:?}, expected about 1s"));
    }
    Ok(())
}

fn stop(s: &dyn Substrate, c: &Ctx) -> Result<(), String> {
    let mut sandbox = s.start(&c.sh("sleep 60")).map_err(|e| e.to_string())?;
    let began = Instant::now();
    sandbox.stop();
    sandbox.stop();
    let out = sandbox.wait();
    if out.ending != Ending::Stopped || began.elapsed() > Duration::from_secs(20) {
        return Err(format!("{:?} after {:?}", out.ending, began.elapsed()));
    }
    // Stopping one that has ended changes nothing, and waiting again says the same thing.
    sandbox.stop();
    if sandbox.wait() != out {
        return Err("a second wait said something different".into());
    }
    let mut done = s.start(&c.sh("exit 3")).map_err(|e| e.to_string())?;
    let first = done.wait();
    done.stop();
    if done.wait() != first || first.ending != Ending::Exited(3) {
        return Err("stopping a sandbox that had ended changed its outcome".into());
    }
    Ok(())
}

fn cleans_up(s: &dyn Substrate, c: &Ctx) -> Result<(), String> {
    let before = s.existing().map_err(|e| e.to_string())?;
    let mut ended = s.start(&c.sh("exit 0")).map_err(|e| e.to_string())?;
    let id = ended.id().to_string();
    ended.wait();
    if s.existing().map_err(|e| e.to_string())?.contains(&id) {
        return Err("a sandbox that ended is still listed".into());
    }
    drop(ended);

    let running = s.start(&c.sh("sleep 60")).map_err(|e| e.to_string())?;
    let id = running.id().to_string();
    if !s.existing().map_err(|e| e.to_string())?.contains(&id) {
        return Err("a running sandbox is not listed".into());
    }
    drop(running);
    if s.existing().map_err(|e| e.to_string())?.contains(&id) {
        return Err("a sandbox dropped without waiting is still there".into());
    }
    if s.existing().map_err(|e| e.to_string())? != before {
        return Err("the list is not what it was".into());
    }
    Ok(())
}

fn bad_specs(s: &dyn Substrate, c: &Ctx) -> Result<(), String> {
    let before = s.existing().map_err(|e| e.to_string())?;
    let mut cases: Vec<(&str, SandboxSpec)> = Vec::new();
    for image in ["", "-v", "--privileged", "a b", "img;rm", "img\n", "img\0"] {
        let mut spec = c.sh("exit 0");
        spec.image = image.into();
        cases.push(("image", spec));
    }
    let mut spec = c.sh("exit 0");
    spec.command.clear();
    cases.push(("empty command", spec));
    let mut spec = c.sh("exit 0");
    spec.command.push("a\0b".into());
    cases.push(("NUL in argument", spec));
    for name in ["", "1A", "A=B", "A B", "A-B", "é"] {
        let mut spec = c.sh("exit 0");
        spec.env.insert(name.into(), "v".into());
        cases.push(("environment name", spec));
    }
    let mut spec = c.sh("exit 0");
    spec.env.insert("A".into(), "x\0y".into());
    cases.push(("NUL in value", spec));
    for socket in ["", "relative.sock", "/tmp/a,b", "/tmp/a:b", "/tmp/a\nb"] {
        let mut spec = c.sh("exit 0");
        spec.broker_socket = socket.into();
        cases.push(("socket path", spec));
    }
    let mut spec = c.sh("exit 0");
    spec.deadline = Duration::ZERO;
    cases.push(("zero deadline", spec));
    let mut spec = c.sh("exit 0");
    spec.memory_bytes = 0;
    cases.push(("zero memory", spec));
    let mut spec = c.sh("exit 0");
    spec.max_processes = 0;
    cases.push(("zero processes", spec));
    for (what, spec) in cases {
        match s.start(&spec) {
            Err(StartError::Spec(_)) => {}
            Err(other) => return Err(format!("{what}: {other}, expected a refused spec")),
            Ok(_) => return Err(format!("{what}: a bad spec was started")),
        }
    }
    if s.existing().map_err(|e| e.to_string())? != before {
        return Err("a refused spec left something behind".into());
    }
    Ok(())
}

fn missing_image(s: &dyn Substrate, c: &Ctx) -> Result<(), String> {
    let before = s.existing().map_err(|e| e.to_string())?;
    let mut spec = c.sh("exit 0");
    spec.image = c.fx.missing_image.clone();
    match s.start(&spec) {
        Err(StartError::Refused(_)) => {}
        Err(other) => return Err(format!("{other}, expected the runtime to refuse it")),
        Ok(_) => return Err("an image that does not exist was started".into()),
    }
    if s.existing().map_err(|e| e.to_string())? != before {
        return Err("a refused start left something behind".into());
    }
    Ok(())
}

fn reports_itself(s: &dyn Substrate, _: &Ctx) -> Result<(), String> {
    if s.name().is_empty()
        || s.name().len() > 32
        || !s
            .name()
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b == b'-')
    {
        return Err(format!("name {:?} is not a short lowercase word", s.name()));
    }
    s.available().map_err(|e| e.to_string())
}

// ---- isolation ------------------------------------------------------------------------------

fn no_network(s: &dyn Substrate, c: &Ctx) -> Result<(), String> {
    let script = r#"ls /sys/class/net | tr '\n' ' '; echo;
perl -MIO::Socket::INET -e 'my $s = IO::Socket::INET->new(PeerAddr => "1.1.1.1:53", Timeout => 3); print $s ? "connected\n" : "unreachable\n"'
perl -MIO::Socket::INET -e 'my $s = IO::Socket::INET->new(PeerAddr => "127.0.0.1:1", Timeout => 3); print $s ? "connected\n" : "closed\n"'"#;
    let out = run_to_end(s, &c.sh(script))?;
    expect(
        "network",
        &out,
        Ending::Exited(0),
        "lo \nunreachable\nclosed\n",
    )
}

fn not_root(s: &dyn Substrate, c: &Ctx) -> Result<(), String> {
    let out = run_to_end(s, &c.sh("id -u"))?;
    let uid = out.stdout_text().trim().to_string();
    if uid.is_empty() || uid == "0" {
        return Err(format!("the sandbox runs as uid {uid:?}"));
    }
    Ok(())
}

fn read_only_root(s: &dyn Substrate, c: &Ctx) -> Result<(), String> {
    let out = run_to_end(
        s,
        // The mount flag, not only a failed write: an unprivileged user cannot write to a root-owned
        // `/` whether or not it is mounted read-only.
        &c.sh(r#"awk '$2 == "/" { n = split($4, o, ","); for (i = 1; i <= n; i++) if (o[i] == "ro") print "root-mounted-ro" }' /proc/mounts
touch /hyalos-test 2>/dev/null && echo root-writable || echo root-not-writable; touch /tmp/ok && echo tmp-writable"#),
    )?;
    expect(
        "filesystem",
        &out,
        Ending::Exited(0),
        "root-mounted-ro\nroot-not-writable\ntmp-writable\n",
    )
}

fn no_privileges(s: &dyn Substrate, c: &Ctx) -> Result<(), String> {
    let out = run_to_end(
        s,
        &c.sh("grep -E '^(CapEff|CapBnd|NoNewPrivs):' /proc/self/status"),
    )?;
    expect(
        "privileges",
        &out,
        Ending::Exited(0),
        "CapEff:\t0000000000000000\nCapBnd:\t0000000000000000\nNoNewPrivs:\t1\n",
    )
}

fn broker_socket(s: &dyn Substrate, c: &Ctx) -> Result<(), String> {
    let script = format!(
        r#"perl -MIO::Socket::UNIX -e 'my $s = IO::Socket::UNIX->new(Peer => "{BROKER_SOCKET_IN_SANDBOX}") or die "connect: $!\n"; print $s "ping\n"; print scalar <$s>;'"#
    );
    let seen = c.broker.connections.load(Ordering::SeqCst);
    let out = run_to_end(s, &c.sh(&script))?;
    expect("broker socket", &out, Ending::Exited(0), "pong\n")?;
    if c.broker.connections.load(Ordering::SeqCst) != seen + 1 {
        return Err("the broker did not see the connection".into());
    }
    Ok(())
}

fn limits(s: &dyn Substrate, c: &Ctx) -> Result<(), String> {
    let mut spec = c.sh("cat /sys/fs/cgroup/pids.max /sys/fs/cgroup/memory.max");
    spec.max_processes = 77;
    spec.memory_bytes = 300 * 1024 * 1024;
    let out = run_to_end(s, &spec)?;
    expect(
        "limits",
        &out,
        Ending::Exited(0),
        &format!("77\n{}\n", 300 * 1024 * 1024),
    )
}

fn init_reaps(s: &dyn Substrate, c: &Ctx) -> Result<(), String> {
    // The program is not PID 1, and a process orphaned by its parent's exit, as a harness's
    // backgrounded command is, is reaped rather than left a zombie counting against the limit.
    let script = r#"[ "$$" = 1 ] && echo the-program-is-pid-1
(sleep 0 &); sleep 1
z=0; for f in /proc/[0-9]*/status; do grep -qs '^State:[[:space:]]*Z' "$f" && z=$((z+1)); done
echo "zombies $z""#;
    let out = run_to_end(s, &c.sh(script))?;
    expect("init", &out, Ending::Exited(0), "zombies 0\n")
}

const BEHAVIOUR: &[(&str, Check)] = &[
    ("reports its name and that it is available", reports_itself),
    ("exit codes are reported", exit_code),
    ("stdout and stderr are captured", output),
    (
        "output is kept to a limit however much is written",
        output_is_bounded,
    ),
    (
        "the environment is the spec's and nothing from the host",
        environment,
    ),
    ("arguments are never read by a shell", no_shell),
    ("a sandbox is killed at its deadline", deadline),
    ("a sandbox can be stopped, and stopping is idempotent", stop),
    (
        "nothing is left after a sandbox ends or is dropped",
        cleans_up,
    ),
    ("a bad spec is refused and starts nothing", bad_specs),
    ("an image that does not exist is refused", missing_image),
];

const ISOLATION: &[(&str, Check)] = &[
    ("there is no network, only loopback", no_network),
    ("the sandbox does not run as root", not_root),
    ("the root filesystem is read-only", read_only_root),
    ("no capabilities and no new privileges", no_privileges),
    (
        "the broker's socket is reachable and is the way out",
        broker_socket,
    ),
    ("memory and process limits are applied", limits),
    ("an init is PID 1 and reaps orphaned processes", init_reaps),
];

/// Runs the contract against a driver.
pub fn run(substrate: &dyn Substrate, fx: &Fixture) -> Report {
    let mut report = Report::default();
    let ctx = match Broker::start(fx) {
        Ok(broker) => Ctx {
            fx: fx.clone(),
            broker,
        },
        Err(why) => {
            report
                .failed
                .push(("the contract could not start its broker", why));
            return report;
        }
    };
    for (name, check) in BEHAVIOUR {
        match check(substrate, &ctx) {
            Ok(()) => report.passed.push(name),
            Err(why) => report.failed.push((name, why)),
        }
    }
    for (name, check) in ISOLATION {
        if fx.isolation {
            match check(substrate, &ctx) {
                Ok(()) => report.passed.push(name),
                Err(why) => report.failed.push((name, why)),
            }
        } else {
            report
                .skipped
                .push((name, "this driver does not claim to isolate"));
        }
    }
    report
}
