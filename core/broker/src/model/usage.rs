//! Metering: the tokens each response reports, charged to its job.

use std::collections::BTreeMap;
use std::io::{self, Read};
use std::sync::{Arc, Mutex, PoisonError};

use hyalos_types::JobId;
use serde_json::Value;

use super::MAX_BODY;

/// One job's account: tokens spent, as the provider reported them, and tokens held for calls still
/// in flight.
#[derive(Debug, Default, Clone, Copy)]
struct Account {
    spent: u64,
    held: u64,
}

/// Tokens spent per job, and held for calls in flight, so calls made side by side cannot together
/// pass the budget by more than their input.
#[derive(Debug, Default)]
pub struct Meter(Mutex<BTreeMap<JobId, Account>>);

impl Meter {
    pub fn spent(&self, job: JobId) -> u64 {
        self.lock().get(&job).map_or(0, |a| a.spent)
    }

    /// Tokens held for calls in flight.
    pub fn held(&self, job: JobId) -> u64 {
        self.lock().get(&job).map_or(0, |a| a.held)
    }

    pub fn charge(&self, job: JobId, tokens: u64) {
        let mut accounts = self.lock();
        let account = accounts.entry(job).or_default();
        account.spent = account.spent.saturating_add(tokens);
    }

    /// Holds up to `want` tokens of what is left of `budget`. `Err(true)` means the budget is spent;
    /// `Err(false)` that what is left is held by calls in flight, which will release it.
    pub(super) fn hold(self: &Arc<Self>, job: JobId, budget: u64, want: u64) -> Result<Hold, bool> {
        let mut accounts = self.lock();
        let account = accounts.entry(job).or_default();
        if account.spent >= budget {
            return Err(true);
        }
        let left = budget.saturating_sub(account.spent.saturating_add(account.held));
        let amount = want.min(left);
        if amount == 0 {
            return Err(false);
        }
        account.held += amount;
        Ok(Hold {
            meter: Arc::clone(self),
            job,
            amount,
        })
    }

    /// Drops a job that has ended.
    pub fn forget(&self, job: JobId) {
        self.lock().remove(&job);
    }

    /// Ends a hold. A job already forgotten stays forgotten: a call that outlived its job adds
    /// nothing back.
    fn release(&self, job: JobId, amount: u64, spent: u64) {
        if let Some(account) = self.lock().get_mut(&job) {
            account.held = account.held.saturating_sub(amount);
            account.spent = account.spent.saturating_add(spent);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<JobId, Account>> {
        // A panic while holding the lock cannot leave a half-written count.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Tokens held for one call. Settling charges what it spent; dropping it unsettled charges nothing
/// (the call never reached a successful response).
pub(super) struct Hold {
    meter: Arc<Meter>,
    job: JobId,
    amount: u64,
}

impl Hold {
    /// The tokens held, which is the most output the call may ask for.
    pub(super) fn amount(&self) -> u64 {
        self.amount
    }

    fn settle(mut self, spent: u64) {
        self.meter.release(self.job, self.amount, spent);
        self.amount = 0;
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        if self.amount > 0 {
            self.meter.release(self.job, self.amount, 0);
        }
    }
}

/// Input and output tokens of one response. Streamed events repeat running totals, so each field
/// keeps the largest value seen.
#[derive(Debug, Default, Clone, Copy)]
struct Usage {
    input: u64,
    output: u64,
}

impl Usage {
    fn total(self) -> u64 {
        self.input.saturating_add(self.output)
    }

    /// Takes the usage an event or document reports, and says whether it reported any.
    fn absorb(&mut self, event: &Value) -> bool {
        let Some(usage) = ["/usage", "/message/usage", "/response/usage"]
            .iter()
            .find_map(|p| event.pointer(p).filter(|u| u.is_object()))
        else {
            return false;
        };
        let n = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
        let input = [
            "input_tokens",
            "cache_creation_input_tokens",
            "cache_read_input_tokens",
            "prompt_tokens",
        ]
        .iter()
        .fold(0u64, |sum, k| sum.saturating_add(n(k)));
        let output = n("output_tokens").saturating_add(n("completion_tokens"));
        self.input = self.input.max(input);
        self.output = self.output.max(output);
        true
    }
}

/// Reads usage from a response body as it passes: a JSON document, or server-sent events.
pub(super) struct Scanner {
    events: bool,
    buf: Vec<u8>,
    /// Some of the body was not read for usage, so what was read may be short.
    lost: bool,
    /// The provider reported usage at least once.
    seen: bool,
    /// The event `type`s that end a stream, and whether one has arrived. A stream's usage before
    /// its end is provisional: Anthropic's `message_start` reports one output token.
    ends: &'static [&'static str],
    ended: bool,
    usage: Usage,
}

impl Scanner {
    pub(super) fn new(content_type: &str, ends: &'static [&'static str]) -> Self {
        Self {
            events: content_type
                .trim()
                .to_ascii_lowercase()
                .starts_with("text/event-stream"),
            buf: Vec::new(),
            lost: false,
            seen: false,
            ends,
            ended: false,
            usage: Usage::default(),
        }
    }

    fn feed(&mut self, mut chunk: &[u8]) {
        if !self.events {
            self.push(chunk);
            return;
        }
        while let Some(i) = chunk.iter().position(|&b| b == b'\n') {
            self.push(&chunk[..i]);
            self.line();
            chunk = &chunk[i + 1..];
        }
        self.push(chunk);
    }

    /// Keeps up to `MAX_BODY` of the current line (or document); past that it is lost.
    fn push(&mut self, part: &[u8]) {
        if self.buf.len() + part.len() > MAX_BODY {
            self.lost = true;
            self.buf = Vec::new();
        } else {
            self.buf.extend_from_slice(part);
        }
    }

    fn line(&mut self) {
        let line = std::mem::take(&mut self.buf);
        if let Some(data) = line.strip_prefix(b"data:") {
            self.absorb(data);
        }
    }

    fn absorb(&mut self, json: &[u8]) {
        match serde_json::from_slice::<Value>(json) {
            Ok(value) => {
                self.seen |= self.usage.absorb(&value);
                let kind = value.get("type").and_then(Value::as_str);
                self.ended |= kind.is_some_and(|k| self.ends.contains(&k));
            }
            Err(_) => self.lost = true,
        }
    }

    /// The tokens the body reported, or `None` if it cannot be trusted to have reported them all:
    /// part of it could not be read, it reported no usage at all, or it is a stream that stopped
    /// before the event that ends it, whose usage is provisional.
    fn finish(mut self) -> Option<u64> {
        if self.events {
            self.line();
        } else if !self.lost {
            let doc = std::mem::take(&mut self.buf);
            self.absorb(&doc);
        }
        let complete = !self.events || self.ended;
        (self.seen && !self.lost && complete).then(|| self.usage.total())
    }
}

/// A response body that charges its job when it ends, or when it is dropped part way. A body whose
/// usage cannot be read in full is charged the job's whole budget: metering fails closed.
pub(super) struct Metered {
    inner: Box<dyn Read + Send>,
    scanner: Option<(Scanner, Hold)>,
    budget: u64,
}

impl Metered {
    pub(super) fn new(
        inner: Box<dyn Read + Send>,
        scanner: Scanner,
        hold: Hold,
        budget: u64,
    ) -> Self {
        Self {
            inner,
            scanner: Some((scanner, hold)),
            budget,
        }
    }

    fn settle(&mut self) {
        if let Some((scanner, hold)) = self.scanner.take() {
            hold.settle(scanner.finish().unwrap_or(self.budget));
        }
    }
}

impl Read for Metered {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n == 0 {
            self.settle();
        } else if let Some((scanner, _)) = self.scanner.as_mut() {
            scanner.feed(&buf[..n]);
        }
        Ok(n)
    }
}

impl Drop for Metered {
    fn drop(&mut self) {
        self.settle();
    }
}
