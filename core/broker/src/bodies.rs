//! What the broker holds of request bodies from jobs' sockets, bounded per job and across the broker
//! (#336).
//!
//! A sandbox can open `MAX_CONNECTIONS` connections and send each a body up to its route's cap. A
//! body is held only as its bytes arrive, never allocated from its `content-length`. Every byte
//! held is counted against the job, so one job cannot hold more than [`PER_JOB`], and against the
//! broker, so all jobs together cannot hold more than [`IN_ALL`]. A body that would pass either is
//! refused as busy. Reading stops when the job ends.

use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// The most request-body bytes one job may hold at once: one git body and one model call.
pub const PER_JOB: usize = crate::git::MAX_GIT_BODY + crate::model::MAX_BODY;

/// The most request-body bytes the broker holds at once, for all jobs together.
pub const IN_ALL: usize = 512 * 1024 * 1024;

/// How much is read at a time.
const STEP: usize = 64 * 1024;

/// Bytes held against a limit.
#[derive(Debug)]
pub struct Pool {
    held: AtomicUsize,
    max: usize,
}

impl Pool {
    pub const fn new(max: usize) -> Self {
        Self {
            held: AtomicUsize::new(0),
            max,
        }
    }

    pub fn held(&self) -> usize {
        self.held.load(Ordering::SeqCst)
    }

    /// Takes `n` more bytes if they fit.
    fn take(&self, n: usize) -> bool {
        self.held
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |held| {
                held.checked_add(n).filter(|total| *total <= self.max)
            })
            .is_ok()
    }

    fn give(&self, n: usize) {
        let _ = self
            .held
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |held| {
                Some(held.saturating_sub(n))
            });
    }
}

/// The broker's own pool, shared by every job.
static BROKER: Pool = Pool::new(IN_ALL);

/// One job's bodies: its own pool, and the broker's.
#[derive(Debug)]
pub struct Bodies {
    job: Pool,
    all: &'static Pool,
}

impl Default for Bodies {
    fn default() -> Self {
        Self::new(PER_JOB, &BROKER)
    }
}

impl Bodies {
    /// A job's bodies, held against `per_job` and against `all`.
    pub const fn new(per_job: usize, all: &'static Pool) -> Self {
        Self {
            job: Pool::new(per_job),
            all,
        }
    }

    /// Bytes this job holds now.
    pub fn held(&self) -> usize {
        self.job.held()
    }

    /// An empty hold, which grows as a body is read and gives back what it held when dropped.
    pub fn hold(&self) -> Held<'_> {
        Held {
            bodies: self,
            bytes: 0,
        }
    }
}

/// Bytes one body holds. Dropping it gives them back.
#[derive(Debug)]
pub struct Held<'a> {
    bodies: &'a Bodies,
    bytes: usize,
}

impl Held<'_> {
    /// Holds `n` more bytes, or says why not.
    pub fn grow(&mut self, n: usize) -> Result<(), Refused> {
        if !self.bodies.job.take(n) {
            return Err(Refused::Busy);
        }
        if !self.bodies.all.take(n) {
            self.bodies.job.give(n);
            return Err(Refused::Busy);
        }
        self.bytes += n;
        Ok(())
    }
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        self.bodies.job.give(self.bytes);
        self.bodies.all.give(self.bytes);
    }
}

/// Why a body was not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// It is longer than its route reads.
    TooLarge,
    /// Holding it would pass the job's or the broker's limit.
    Busy,
    /// It ended, or failed, before it was whole.
    Cut,
    /// The job ended while it was read.
    Ended,
}

/// Reads exactly `length` bytes, or to the end of `reader` if `length` is `None`, refusing more than
/// `max`. Each step is held in `held` before it is kept, and reading stops if `ended` is set.
pub fn read(
    reader: &mut impl Read,
    length: Option<usize>,
    max: usize,
    held: &mut Held<'_>,
    ended: &AtomicBool,
) -> Result<Vec<u8>, Refused> {
    if length.is_some_and(|n| n > max) {
        return Err(Refused::TooLarge);
    }
    let mut body = Vec::new();
    let mut step = vec![0; STEP];
    loop {
        if ended.load(Ordering::SeqCst) {
            return Err(Refused::Ended);
        }
        let want = match length {
            Some(n) => n - body.len(),
            // One byte past the cap tells a long body from one exactly at it.
            None => (max + 1).saturating_sub(body.len()),
        };
        if want == 0 {
            break;
        }
        let n = match reader.read(&mut step[..want.min(STEP)]) {
            Ok(0) if length.is_none() => break,
            Ok(0) | Err(_) => return Err(Refused::Cut),
            Ok(n) => n,
        };
        held.grow(n)?;
        body.extend_from_slice(&step[..n]);
        if body.len() > max {
            return Err(Refused::TooLarge);
        }
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(max: usize) -> &'static Pool {
        Box::leak(Box::new(Pool::new(max)))
    }

    #[test]
    fn a_body_is_held_only_as_its_bytes_arrive_and_given_back_when_dropped() {
        let bodies = Bodies::new(1000, pool(1000));
        let ended = AtomicBool::new(false);
        let mut held = bodies.hold();
        // A length far past what is sent allocates nothing up front: the read is cut short.
        let sent = [7u8; 10];
        assert_eq!(
            read(&mut &sent[..], Some(900), 1000, &mut held, &ended),
            Err(Refused::Cut)
        );
        assert_eq!(bodies.held(), 10, "only what arrived is held");
        drop(held);
        assert_eq!(bodies.held(), 0);
    }

    #[test]
    fn one_job_cannot_pass_its_own_limit_or_the_brokers() {
        let all = pool(150);
        let one = Bodies::new(100, all);
        let other = Bodies::new(100, all);
        let ended = AtomicBool::new(false);
        let bytes = [1u8; 100];

        let mut first = one.hold();
        assert!(read(&mut &bytes[..60], Some(60), 1000, &mut first, &ended).is_ok());
        let mut second = one.hold();
        assert_eq!(
            read(&mut &bytes[..60], Some(60), 1000, &mut second, &ended),
            Err(Refused::Busy),
            "the job's own limit"
        );
        drop(second);
        assert_eq!(one.held(), 60, "a refused body gives back what it held");

        let mut third = other.hold();
        assert!(read(&mut &bytes[..80], Some(80), 1000, &mut third, &ended).is_ok());
        let mut fourth = other.hold();
        assert_eq!(
            read(&mut &bytes[..20], Some(20), 1000, &mut fourth, &ended),
            Err(Refused::Busy),
            "the broker's limit, though the job has room"
        );
        drop((first, third, fourth));
        assert_eq!(all.held(), 0);
    }

    #[test]
    fn a_body_past_its_cap_is_refused_with_or_without_a_length() {
        let bodies = Bodies::new(1000, pool(1000));
        let ended = AtomicBool::new(false);
        let bytes = [1u8; 50];
        let mut held = bodies.hold();
        assert_eq!(
            read(&mut &bytes[..], Some(50), 40, &mut held, &ended),
            Err(Refused::TooLarge)
        );
        assert_eq!(
            read(&mut &bytes[..], None, 40, &mut held, &ended),
            Err(Refused::TooLarge)
        );
        assert_eq!(
            read(&mut &bytes[..], None, 50, &mut held, &ended).map(|b| b.len()),
            Ok(50)
        );
    }

    #[test]
    fn reading_stops_when_the_job_ends() {
        let bodies = Bodies::new(1000, pool(1000));
        let ended = AtomicBool::new(true);
        let bytes = [1u8; 10];
        let mut held = bodies.hold();
        assert_eq!(
            read(&mut &bytes[..], Some(10), 100, &mut held, &ended),
            Err(Refused::Ended)
        );
    }
}
