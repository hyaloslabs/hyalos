//! The substrate interface and its drivers (ADR-0011).
//!
//! A substrate starts a sandbox: a process with no network and no secrets, whose only way out is a
//! socket to the broker. The engine picks the best substrate the host has and records which one ran
//! each job; every driver must pass the same contract suite (`crates/contract`).
//!
//! Layout rule (ADR-0017): this crate depends only on hyalos-types.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use hyalos_types::SubstrateName;

mod choose;
mod container;
mod spec;

pub use choose::{Choice, HOST_OS, Tried, Unsupported, check_host, choose, detect};
pub use container::{ContainerSubstrate, GVISOR_RUNTIME};
pub use spec::{
    BROKER_SOCKET_IN_SANDBOX, Ending, Isolation, OUTPUT_LIMIT, Outcome, SandboxSpec, SpecError,
    StartError, Unavailable,
};

/// Something that can start sandboxes. It is `Send` so the engine can detect substrates on a thread
/// with a timeout (ADR-0050).
pub trait Substrate: Send {
    /// A short name for the driver, recorded with each job so the audit shows what ran it.
    fn name(&self) -> &'static str;

    /// What each job this driver runs records as its substrate on `job.started` (ADR-0043): the name,
    /// unless the driver can say more, such as which runtime it drives.
    fn recorded_as(&self) -> SubstrateName {
        SubstrateName::new(self.name()).unwrap_or_else(SubstrateName::unrecorded)
    }

    /// How strong the boundary is, so the engine can prefer the best one the host has.
    fn isolation(&self) -> Isolation;

    /// Whether this driver can run on this host now, and if not why not. Probing must not start a
    /// sandbox.
    fn available(&self) -> Result<(), Unavailable>;

    /// Whether this driver can run a job from `image` now: it is available, and anything every
    /// sandbox needs works, such as the container runtime's init (ADR-0041, ADR-0043). Unlike
    /// [`Substrate::available`] this may start and remove a short probe, never a job's sandbox. The
    /// engine chooses only a driver that is ready.
    fn ready(&self, image: &str) -> Result<(), Unavailable> {
        let _ = image;
        self.available()
    }

    /// Starts a sandbox for `spec`. A spec that is invalid, or an image or runtime that cannot start,
    /// is an error and leaves nothing running. A sandbox that starts and then fails is not an error
    /// here: it ends with an [`Outcome`].
    fn start(&self, spec: &SandboxSpec) -> Result<Box<dyn Sandbox>, StartError>;

    /// The sandboxes this driver has started that still exist, running or not yet cleaned up. After
    /// a sandbox's [`Sandbox::wait`] returns it is not in this list.
    fn existing(&self) -> Result<Vec<String>, Unavailable>;
}

/// One running sandbox. It is `Send` so the engine can wait for it while it serves the broker.
pub trait Sandbox: Send {
    /// An identifier for the sandbox, for logs and for [`Substrate::existing`].
    fn id(&self) -> &str;

    /// Waits for the sandbox to end, by its own exit, by [`Sandbox::stop`], or when its deadline
    /// passes (the deadline is measured from the start, and the sandbox is killed at it). The
    /// sandbox is gone when this returns.
    fn wait(&mut self) -> Outcome;

    /// Ends the sandbox now, without waiting for it to agree. A later [`Sandbox::wait`] returns
    /// [`Ending::Stopped`]. Stopping one that already ended does nothing.
    fn stop(&mut self);

    /// A handle that stops the sandbox from another thread while [`Sandbox::wait`] runs on this one
    /// (ADR-0053). `wait` sees it within its polling interval and ends the sandbox as
    /// [`Sandbox::stop`] would, returning [`Ending::Stopped`]; a handle used before `wait` is called
    /// stops the sandbox as soon as it is.
    fn stop_handle(&self) -> StopHandle;
}

/// Asks a sandbox to stop, from any thread. Cloning it gives another handle on the same sandbox.
#[derive(Debug, Clone, Default)]
pub struct StopHandle(Arc<AtomicBool>);

impl StopHandle {
    pub fn stop(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Whether [`StopHandle::stop`] has been called on this handle or a clone of it.
    pub fn is_stopped(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}
