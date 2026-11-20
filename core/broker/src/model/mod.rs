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

pub use anthropic::ANTHROPIC;
pub use http::HttpTransport;
pub(crate) use http::dechunk;
pub use openai::OPENAI;
pub use relay::{Backoff, QUOTA_SPENT, Relay, Sleep, ThreadSleep};
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

    pub(crate) fn expose(&self) -> &str {
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

/// What differs between the protocols the broker speaks: [`ANTHROPIC`] and [`OPENAI`].
pub struct Adapter {
    /// The path of a generation (without its query string). The broker serves no path but this and
    /// `count_tokens`.
    pub generate: &'static str,
    /// The path of counting a request's tokens, which spends none.
    pub count_tokens: &'static str,
    /// The header that carries the key, and what goes before the key in it.
    pub credential_header: (&'static str, &'static str),
    /// Request headers from the harness that are passed on, lowercase. Every other one is dropped,
    /// the placeholder key with them.
    pub passthrough: &'static [&'static str],
    /// Headers added when the harness did not send them.
    pub defaults: &'static [(&'static str, &'static str)],
    /// Whether the harness runs a tool defined this way. Anything else is run by the provider, or is
    /// unknown, and is removed.
    pub harness_runs: fn(&Value) -> bool,
    /// Removes request fields that only make sense with a provider-hosted tool.
    pub scrub: fn(&mut Map<String, Value>),
    /// The top-level request fields passed on. A request with any other field is refused (ADR-0120):
    /// a field the broker does not know may name state the provider holds or work it runs where no
    /// gate or meter sees it.
    pub fields: &'static [&'static str],
    /// The request field that caps a generation's output tokens.
    pub output_limit: &'static str,
    /// The status the harness retries when the broker gives up on an overloaded provider.
    pub overloaded: u16,
    /// The `type`s of the stream events that end a generation. Usage seen in a stream is final only
    /// once one of these arrives: earlier events carry provisional counts (ADR-0037).
    pub stream_ends: &'static [&'static str],
    /// An error body in this protocol's shape.
    pub error_body: fn(u16, &str) -> Vec<u8>,
    /// Whether a 429 body says the key's quota is spent, which no retry fixes.
    pub quota_exhausted: fn(&[u8]) -> bool,
    /// Whether a stream event says the key's quota or credit is spent: the provider started
    /// answering, then could not go on for this key (#355).
    pub quota_event: fn(&Value) -> bool,
}

impl Adapter {
    /// The call a path is (without its query string), or `None` if the broker does not serve it.
    pub fn route(&self, path: &str) -> Option<Route> {
        if path == self.generate {
            Some(Route::Generate)
        } else {
            (path == self.count_tokens).then_some(Route::CountTokens)
        }
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct TransportError(pub &'static str);

impl TransportError {
    /// Nothing was sent: the provider could not be reached. Only this error is retried, since after
    /// a request is sent the provider may already be generating, and a retry would pay twice.
    pub const UNREACHABLE: TransportError = TransportError("transport.unreachable");
    /// The provider's TLS failed: its certificate, its name or the protocol. Nothing was sent, but
    /// trying again cannot fix it, so it is not retried (ADR-0039).
    pub const TLS_REFUSED: TransportError = TransportError("transport.tls_refused");
}

/// Carries a request to the provider and returns its answer. Requests are always `POST`.
pub trait Transport: Send + Sync {
    fn send(&self, request: &Upstream) -> Result<UpstreamResponse, TransportError>;
}

/// The broker's own answer in a protocol's error shape. `code` is a reason code.
pub fn refusal(adapter: &Adapter, status: u16, code: &str, message: &str) -> Response {
    Response {
        reason: ReasonCode::new(code),
        ..Response::bytes(
            status,
            "application/json",
            (adapter.error_body)(status, message),
        )
    }
}

pub(crate) fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}
