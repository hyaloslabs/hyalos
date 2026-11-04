//! Metering: the tokens each response reports, charged to its job.

use std::collections::BTreeMap;
use std::io::{self, Read};
use std::sync::{Arc, Mutex, PoisonError};

use hyalos_types::JobId;
use serde_json::Value;

use super::MAX_BODY;

/// The longest server-sent event line the scanner reads. A longer one is skipped.
const MAX_LINE: usize = 1024 * 1024;

/// Tokens spent per job, as the provider reported them.
#[derive(Debug, Default)]
pub struct Meter(Mutex<BTreeMap<JobId, u64>>);

impl Meter {
    pub fn spent(&self, job: JobId) -> u64 {
        self.lock().get(&job).copied().unwrap_or(0)
    }

    pub fn charge(&self, job: JobId, tokens: u64) {
        let mut spent = self.lock();
        let total = spent.entry(job).or_default();
        *total = total.saturating_add(tokens);
    }

    /// Drops a job that has ended.
    pub fn forget(&self, job: JobId) {
        self.lock().remove(&job);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<JobId, u64>> {
        // A panic while holding the lock cannot leave a half-written count.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
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

    fn absorb(&mut self, event: &Value) {
        let Some(usage) = ["/usage", "/message/usage", "/response/usage"]
            .iter()
            .find_map(|p| event.pointer(p).filter(|u| u.is_object()))
        else {
            return;
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
    }
}

/// Reads usage from a response body as it passes: a JSON document, or server-sent events.
pub(super) struct Scanner {
    events: bool,
    buf: Vec<u8>,
    skipping: bool,
    usage: Usage,
}

impl Scanner {
    pub(super) fn new(content_type: &str) -> Self {
        Self {
            events: content_type
                .trim()
                .to_ascii_lowercase()
                .starts_with("text/event-stream"),
            buf: Vec::new(),
            skipping: false,
            usage: Usage::default(),
        }
    }

    fn feed(&mut self, mut chunk: &[u8]) {
        if !self.events {
            // A response this large is beyond any model's output; its usage is not read.
            if self.buf.len() + chunk.len() <= MAX_BODY {
                self.buf.extend_from_slice(chunk);
            } else {
                self.skipping = true;
            }
            return;
        }
        while let Some(i) = chunk.iter().position(|&b| b == b'\n') {
            self.push(&chunk[..i]);
            self.line();
            chunk = &chunk[i + 1..];
        }
        self.push(chunk);
    }

    fn push(&mut self, part: &[u8]) {
        if self.buf.len() + part.len() > MAX_LINE {
            self.skipping = true;
            self.buf.clear();
        } else if !self.skipping {
            self.buf.extend_from_slice(part);
        }
    }

    fn line(&mut self) {
        if !self.skipping
            && let Some(data) = self.buf.strip_prefix(b"data:")
            && let Ok(event) = serde_json::from_slice::<Value>(data)
        {
            self.usage.absorb(&event);
        }
        self.skipping = false;
        self.buf.clear();
    }

    fn finish(mut self) -> u64 {
        if self.events {
            self.line();
        } else if !self.skipping
            && let Ok(doc) = serde_json::from_slice::<Value>(&self.buf)
        {
            self.usage.absorb(&doc);
        }
        self.usage.total()
    }
}

/// A response body that charges its job when it ends, or when it is dropped part way.
pub(super) struct Metered {
    inner: Box<dyn Read + Send>,
    scanner: Option<Scanner>,
    meter: Arc<Meter>,
    job: JobId,
}

impl Metered {
    pub(super) fn new(
        inner: Box<dyn Read + Send>,
        scanner: Scanner,
        meter: Arc<Meter>,
        job: JobId,
    ) -> Self {
        Self {
            inner,
            scanner: Some(scanner),
            meter,
            job,
        }
    }

    fn settle(&mut self) {
        if let Some(scanner) = self.scanner.take() {
            self.meter.charge(self.job, scanner.finish());
        }
    }
}

impl Read for Metered {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n == 0 {
            self.settle();
        } else if let Some(scanner) = self.scanner.as_mut() {
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
