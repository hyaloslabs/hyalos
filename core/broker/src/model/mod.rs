//! Model calls through the broker (ADR-0016, ADR-0037).
//!
//! A harness in a sandbox speaks Anthropic Messages or OpenAI Responses to the broker with a
//! placeholder key. The broker serves both through one [`Adapter`] interface and one [`Relay`]: it
//! refuses what the manifest does not allow (any tool definition from a reader, a call once the job's
//! budget is spent), removes provider-hosted tools from every request, adds the real key, absorbs
//! upstream rate limits and overload with backoff, and meters the tokens each response reports.
//!
//! An adapter is only what differs between the two protocols: paths, the credential header, which
//! tool definitions the harness runs itself, the retryable status and the shape of an error. The
//! [`Transport`] that carries a request upstream is separate, so the relay is tested without a network.

mod anthropic;
mod http;
mod openai;
mod relay;
mod usage;

pub use anthropic::Anthropic;
pub use http::HttpTransport;
pub(crate) use http::dechunk;
pub use openai::OpenAi;
pub use relay::{Backoff, Relay, Sleep, ThreadSleep};
pub use usage::Meter;

use std::fmt;
use std::io::Read;

use hyalos_types::ReasonCode;
use serde_json::{Map, Value};

/// The largest request body the broker reads, and the largest non-streamed response it meters.
pub const MAX_BODY: usize = 32 * 1024 * 1024;

/// A provider API key. It is shown by nothing: not `Debug`, not an error, not a response.
#[derive(Clone)]
pub struct ApiKey(String);

impl ApiKey {
    /// A key is printable ASCII with no spaces, so it cannot break a header line.
    pub fn new(key: &str) -> Option<Self> {
        let valid = !key.is_empty() && key.bytes().all(|b| (0x21..=0x7e).contains(&b));
        valid.then(|| Self(key.to_string()))
    }

    fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(..)")
    }
}

/// A model call as the harness made it.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    /// The path and query string, such as `/v1/messages?beta=true`.
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// What the harness is answered: the provider's response, or the broker's own refusal in the
/// provider's error shape.
pub struct Response {
    pub status: u16,
    pub content_type: String,
    pub body: Box<dyn Read + Send>,
    /// Set when the broker answered instead of the provider, saying why.
    pub reason: Option<ReasonCode>,
}

/// Which kind of call a path is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// A generation, which spends tokens and is metered.
    Generate,
    /// Counting the tokens of a request, which spends none.
    CountTokens,
}

/// What differs between the protocols the broker speaks.
pub trait Adapter: Send + Sync {
    /// The call a path is (without its query string), or `None` if the broker does not serve it.
    fn route(&self, path: &str) -> Option<Route>;
    /// The header that carries the key, and what goes before the key in it.
    fn credential_header(&self) -> (&'static str, &'static str);
    /// Request headers from the harness that are passed on, lowercase. Every other one is dropped,
    /// the placeholder key with them.
    fn passthrough(&self) -> &'static [&'static str];
    /// Headers added when the harness did not send them.
    fn defaults(&self) -> &'static [(&'static str, &'static str)] {
        &[]
    }
    /// Whether the harness runs a tool defined this way. Anything else is run by the provider, or is
    /// unknown, and is removed.
    fn harness_runs(&self, tool: &Value) -> bool;
    /// Removes request fields that only make sense with a provider-hosted tool.
    fn scrub(&self, _body: &mut Map<String, Value>) {}
    /// The request field that caps a generation's output tokens.
    fn output_limit(&self) -> &'static str;
    /// The status the harness retries when the broker gives up on an overloaded provider.
    fn overloaded(&self) -> u16;
    /// An error body in this protocol's shape.
    fn error_body(&self, status: u16, message: &str) -> Vec<u8>;
    /// Whether a 429 body says the key's quota is spent, which no retry fixes.
    fn quota_exhausted(&self, _body: &[u8]) -> bool {
        false
    }
}

/// A request on its way to the provider, with the real key in its headers.
pub struct Upstream {
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// The provider's answer. Header names are lowercase.
pub struct UpstreamResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Box<dyn Read + Send>,
}

/// Why a request did not reach the provider or its answer could not be read, as a short code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransportError(pub &'static str);

impl TransportError {
    /// Nothing was sent: the provider could not be reached. Only this error is retried, since after
    /// a request is sent the provider may already be generating, and a retry would pay twice.
    pub const UNREACHABLE: TransportError = TransportError("transport.unreachable");
    /// The provider's TLS failed: its certificate, its name or the protocol. Nothing was sent, but
    /// trying again cannot fix it, so it is not retried (ADR-0039).
    pub const TLS_REFUSED: TransportError = TransportError("transport.tls_refused");
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for TransportError {}

/// Carries a request to the provider and returns its answer. Requests are always `POST`.
pub trait Transport: Send + Sync {
    fn send(&self, request: &Upstream) -> Result<UpstreamResponse, TransportError>;
}

/// The broker's own answer in a protocol's error shape. `code` is a reason code.
pub fn refusal(adapter: &dyn Adapter, status: u16, code: &str, message: &str) -> Response {
    Response {
        status,
        content_type: "application/json".into(),
        body: Box::new(std::io::Cursor::new(adapter.error_body(status, message))),
        reason: ReasonCode::new(code),
    }
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}
