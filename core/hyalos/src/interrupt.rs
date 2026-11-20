//! Ctrl-C and `SIGTERM` during `hyalos run` (ADR-0053).
//!
//! A run killed by its first signal would leave the job's forge token live and the job with no end in
//! the log. So while a run is under way the two signals are taken on a thread of their own. The first
//! marks the run interrupted, and the run then ends the job as it ends any job: it stops the sandbox,
//! ends the job in the broker, revokes the token and records `job.ended`. Each signal after the first
//! calls `again`, which in the binary says what was left undone and exits at once.

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};
use std::thread::JoinHandle;

use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::iterator::{Handle, Signals};

/// Whether the run has been asked to stop, and by which signal.
pub struct Interrupt {
    /// The first signal taken, or 0 for none.
    first: Arc<AtomicI32>,
    taking: Option<(Handle, JoinHandle<()>)>,
    /// A file whose appearance cancels the run as the first signal would (ADR-0069): the MCP
    /// server's `tasks/cancel` makes it, which needs no signal, so asking twice is harmless.
    cancel: Option<PathBuf>,
}

/// What [`Interrupt::signal`] names a cancel by the driving agent.
pub const CANCELLED: &str = "tasks/cancel";

impl Interrupt {
    /// Takes no signal and is never set: the signals keep their default action.
    pub fn none() -> Self {
        Self {
            first: Arc::default(),
            taking: None,
            cancel: None,
        }
    }

    /// Takes `SIGINT` and `SIGTERM` until it is dropped. The first one sets it; each one after that
    /// calls `again` with the signal's number, on the thread that takes them.
    pub fn on_signals(again: impl Fn(i32) + Send + 'static) -> io::Result<Self> {
        let mut signals = Signals::new([SIGINT, SIGTERM])?;
        let handle = signals.handle();
        let first = Arc::new(AtomicI32::new(0));
        let seen = first.clone();
        let thread = std::thread::spawn(move || {
            for signal in signals.forever() {
                if seen
                    .compare_exchange(0, signal, Ordering::SeqCst, Ordering::SeqCst)
                    .is_err()
                {
                    again(signal);
                }
            }
        });
        Ok(Self {
            first,
            taking: Some((handle, thread)),
            cancel: None,
        })
    }

    /// Also stops the run once `cancel` exists.
    pub fn watch(&mut self, cancel: PathBuf) {
        self.cancel = Some(cancel);
    }

    /// The first signal taken, by name, or [`CANCELLED`] once the cancel file exists, or `None`.
    pub fn signal(&self) -> Option<&'static str> {
        name(self.first.load(Ordering::SeqCst)).or_else(|| {
            self.cancel
                .as_ref()
                .is_some_and(|file| file.exists())
                .then_some(CANCELLED)
        })
    }
}

/// The name of `SIGINT` or `SIGTERM`; `None` for anything else.
pub fn name(signal: i32) -> Option<&'static str> {
    match signal {
        SIGINT => Some("SIGINT"),
        SIGTERM => Some("SIGTERM"),
        _ => None,
    }
}

impl Drop for Interrupt {
    fn drop(&mut self) {
        if let Some((handle, thread)) = self.taking.take() {
            handle.close();
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_is_never_set() {
        let interrupt = Interrupt::none();
        assert!(interrupt.signal().is_none());
        assert_eq!(interrupt.signal(), None);
        assert_eq!(name(SIGINT), Some("SIGINT"));
        assert_eq!(name(SIGTERM), Some("SIGTERM"));
        assert_eq!(name(0), None);
    }
}
