//! A driver that runs the program as a host process. It is a fake: it isolates nothing, and exists so
//! the behavioural half of the contract can be checked without a container runtime.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hyalos_substrate::{
    Ending, Isolation, OUTPUT_LIMIT, Outcome, Sandbox, SandboxSpec, StartError, StopHandle,
    Substrate, Unavailable,
};

/// Runs sandboxes as child processes of the engine with an empty environment. Only the image named
/// at construction exists.
#[derive(Debug, Clone)]
pub struct LocalProcessSubstrate {
    image: String,
    live: Arc<Mutex<Vec<String>>>,
    next: Arc<AtomicU64>,
}

impl LocalProcessSubstrate {
    pub fn new(image: &str) -> Self {
        Self {
            image: image.to_string(),
            live: Arc::default(),
            next: Arc::default(),
        }
    }
}

impl Substrate for LocalProcessSubstrate {
    fn name(&self) -> &'static str {
        "local-process"
    }

    fn isolation(&self) -> Isolation {
        Isolation::Container
    }

    /// None: it runs programs on the host, with nothing to attest.
    fn attestation(&self) -> hyalos_types::Attestation {
        hyalos_types::Attestation::no_hardware_root()
    }

    fn available(&self) -> Result<(), Unavailable> {
        Ok(())
    }

    fn start(&self, spec: &SandboxSpec) -> Result<Box<dyn Sandbox>, StartError> {
        spec.validate().map_err(StartError::Spec)?;
        if spec.image != self.image {
            return Err(StartError::Refused(format!(
                "no such image: {}",
                spec.image
            )));
        }
        let (program, args) = spec
            .command
            .split_first()
            .ok_or(StartError::Spec(hyalos_substrate::SpecError::Command))?;
        let mut child = Command::new(program)
            .args(args)
            .env_clear()
            // Its own process group, so everything it starts can be killed with it.
            .process_group(0)
            .envs(&spec.env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| StartError::Refused(e.to_string()))?;
        let id = format!("local-{}", self.next.fetch_add(1, Ordering::SeqCst));
        if let Ok(mut live) = self.live.lock() {
            live.push(id.clone());
        }
        let stdout = child.stdout.take().map(drain);
        let stderr = child.stderr.take().map(drain);
        Ok(Box::new(LocalSandbox {
            id,
            live: self.live.clone(),
            child,
            stdout,
            stderr,
            deadline_at: Instant::now() + spec.deadline,
            stopped: false,
            stop_asked: StopHandle::default(),
            outcome: None,
        }))
    }

    fn existing(&self) -> Result<Vec<String>, Unavailable> {
        Ok(self.live.lock().map(|l| l.clone()).unwrap_or_default())
    }
}

fn drain<R: Read + Send + 'static>(mut stream: R) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let room = OUTPUT_LIMIT.saturating_sub(kept.len());
                    kept.extend_from_slice(&buffer[..n.min(room)]);
                }
            }
        }
        kept
    })
}

struct LocalSandbox {
    id: String,
    live: Arc<Mutex<Vec<String>>>,
    child: Child,
    stdout: Option<JoinHandle<Vec<u8>>>,
    stderr: Option<JoinHandle<Vec<u8>>>,
    deadline_at: Instant,
    stopped: bool,
    stop_asked: StopHandle,
    outcome: Option<Outcome>,
}

impl LocalSandbox {
    /// Kills the program and everything it started, which would otherwise keep the output open.
    fn kill(&mut self) {
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{}", self.child.id())])
            .stderr(Stdio::null())
            .status();
        let _ = self.child.kill();
    }

    fn forget(&self) {
        if let Ok(mut live) = self.live.lock() {
            live.retain(|i| i != &self.id);
        }
    }
}

impl Sandbox for LocalSandbox {
    fn id(&self) -> &str {
        &self.id
    }

    fn wait(&mut self) -> Outcome {
        if let Some(done) = &self.outcome {
            return done.clone();
        }
        let mut deadline_hit = false;
        let status = loop {
            match self.child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {}
                Err(_) => break None,
            }
            if !self.stopped && self.stop_asked.is_stopped() {
                self.stopped = true;
                self.kill();
            }
            if !self.stopped && !deadline_hit && Instant::now() >= self.deadline_at {
                deadline_hit = true;
                self.kill();
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let ending = if self.stopped {
            Ending::Stopped
        } else if deadline_hit {
            Ending::DeadlineExceeded
        } else {
            status
                .and_then(|s| s.code())
                .map_or(Ending::Lost, Ending::Exited)
        };
        let outcome = Outcome {
            ending,
            stdout: self
                .stdout
                .take()
                .and_then(|h| h.join().ok())
                .unwrap_or_default(),
            stderr: self
                .stderr
                .take()
                .and_then(|h| h.join().ok())
                .unwrap_or_default(),
        };
        self.forget();
        self.outcome = Some(outcome.clone());
        outcome
    }

    fn stop(&mut self) {
        if self.outcome.is_none() {
            self.stopped = true;
            self.kill();
        }
    }

    fn stop_handle(&self) -> StopHandle {
        self.stop_asked.clone()
    }
}

impl Drop for LocalSandbox {
    fn drop(&mut self) {
        if self.outcome.is_none() {
            self.kill();
            let _ = self.child.wait();
            self.forget();
        }
    }
}
