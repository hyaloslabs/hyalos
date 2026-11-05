//! A fake substrate that starts nothing: how a sandbox ends is drawn from a seeded stream (ADR-0013).
//!
//! `LocalProcessSubstrate` in `hyalos-substrate-contract` runs real processes, so its timing is the
//! machine's. This one has no process at all, which is what a simulation of thousands of runs needs.
//! It makes no claim to isolation and does not pass the contract's isolation half.

use std::sync::{Arc, Mutex};

use hyalos_sim::FakeClock;
use hyalos_substrate::{
    Ending, Isolation, Outcome, Sandbox, SandboxSpec, StartError, StopHandle, Substrate,
    Unavailable,
};
use hyalos_types::SeededRng;

#[derive(Debug, Clone)]
pub struct SeededSubstrate {
    rng: Arc<Mutex<SeededRng>>,
    clock: FakeClock,
    live: Arc<Mutex<Vec<String>>>,
    started: Arc<Mutex<Vec<SandboxSpec>>>,
}

impl SeededSubstrate {
    pub fn new(rng: SeededRng, clock: FakeClock) -> Self {
        Self {
            rng: Arc::new(Mutex::new(rng)),
            clock,
            live: Arc::default(),
            started: Arc::default(),
        }
    }

    /// Every spec this substrate was asked to start, in order.
    pub fn started(&self) -> Vec<SandboxSpec> {
        self.started.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn draw(&self, bound: u64) -> u64 {
        self.rng.lock().map(|mut r| r.below(bound)).unwrap_or(0)
    }
}

impl Substrate for SeededSubstrate {
    fn name(&self) -> &'static str {
        "seeded"
    }

    fn isolation(&self) -> Isolation {
        Isolation::Container
    }

    fn available(&self) -> Result<(), Unavailable> {
        Ok(())
    }

    fn start(&self, spec: &SandboxSpec) -> Result<Box<dyn Sandbox>, StartError> {
        spec.validate().map_err(StartError::Spec)?;
        // About one start in forty is refused by the runtime.
        if self.draw(40) == 0 {
            return Err(StartError::Refused("the runtime refused".into()));
        }
        // Decided at the start, so the same seed gives the same ending however the run is driven.
        let ending = match self.draw(20) {
            0 => Ending::DeadlineExceeded,
            1 => Ending::Lost,
            2 | 3 => Ending::Exited(1),
            _ => Ending::Exited(0),
        };
        let id = format!("seeded-{}", spec.job);
        if let Ok(mut live) = self.live.lock() {
            live.push(id.clone());
        }
        if let Ok(mut started) = self.started.lock() {
            started.push(spec.clone());
        }
        // Running takes fake time, so a deadline can pass.
        self.clock
            .advance(std::time::Duration::from_millis(10 + self.draw(200)));
        Ok(Box::new(SeededSandbox {
            id,
            ending,
            live: self.live.clone(),
            done: false,
            stop_asked: StopHandle::default(),
        }))
    }

    fn existing(&self) -> Result<Vec<String>, Unavailable> {
        Ok(self.live.lock().map(|l| l.clone()).unwrap_or_default())
    }
}

struct SeededSandbox {
    id: String,
    ending: Ending,
    live: Arc<Mutex<Vec<String>>>,
    done: bool,
    stop_asked: StopHandle,
}

impl SeededSandbox {
    fn remove(&mut self) {
        self.done = true;
        if let Ok(mut live) = self.live.lock() {
            live.retain(|i| *i != self.id);
        }
    }
}

impl Sandbox for SeededSandbox {
    fn id(&self) -> &str {
        &self.id
    }

    fn wait(&mut self) -> Outcome {
        let ending = if self.done || self.stop_asked.is_stopped() {
            Ending::Stopped
        } else {
            self.ending
        };
        self.remove();
        Outcome {
            ending,
            stdout: Vec::new(),
            stderr: Vec::new(),
        }
    }

    fn stop(&mut self) {
        self.remove();
    }

    fn stop_handle(&self) -> StopHandle {
        self.stop_asked.clone()
    }
}

impl Drop for SeededSandbox {
    fn drop(&mut self) {
        self.remove();
    }
}
