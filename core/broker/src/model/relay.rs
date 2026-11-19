//! The one path every model call takes, whichever protocol it speaks.

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use hyalos_types::{Manifest, Role};
use serde_json::{Map, Value};

use super::usage::{Hold, Metered, Scanner};
use super::{
    Adapter, ApiKey, MAX_BODY, Meter, Request, Response, Route, Transport, TransportError,
    Upstream, UpstreamResponse, header, refusal,
};

/// Upstream statuses that mean "busy, try later": rate limits and overload.
const TRANSIENT: &[u16] = &[429, 502, 503, 504, 529];

/// The output a generation that asks for no limit may produce, and so holds.
const DEFAULT_HOLD: u64 = 16_384;

/// How much of a 429 body is read to tell a spent quota from a rate limit.
const MAX_ERROR_BODY: u64 = 64 * 1024;

/// Waits between attempts. The broker's is the thread's sleep; a test's records the waits.
pub trait Sleep: Send + Sync {
    fn sleep(&self, duration: Duration);
}

/// Sleeps the calling thread.
#[derive(Debug, Clone, Copy, Default)]
pub struct ThreadSleep;

impl Sleep for ThreadSleep {
    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// How the relay retries a busy provider: up to `attempts` calls, waiting the provider's
/// `retry-after` or else `base` doubled each time, never more than `cap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    pub attempts: u32,
    pub base: Duration,
    pub cap: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            attempts: 4,
            base: Duration::from_secs(1),
            cap: Duration::from_secs(30),
        }
    }
}

impl Backoff {
    fn wait(&self, attempt: u32, retry_after: Option<u64>) -> Duration {
        let wait = match retry_after {
            Some(seconds) => Duration::from_secs(seconds),
            None => self
                .base
                .saturating_mul(1u32 << attempt.saturating_sub(1).min(16)),
        };
        wait.min(self.cap)
    }
}

/// The broker answering instead of the provider.
struct Refused {
    status: u16,
    code: &'static str,
    message: String,
}

fn refused(status: u16, code: &'static str, message: impl Into<String>) -> Refused {
    Refused {
        status,
        code,
        message: message.into(),
    }
}

/// Serves model calls for admitted jobs through one adapter, one transport and one key.
#[derive(Clone)]
pub struct Relay {
    adapter: Arc<dyn Adapter>,
    transport: Arc<dyn Transport>,
    key: ApiKey,
    meter: Arc<Meter>,
    sleep: Arc<dyn Sleep>,
    backoff: Backoff,
}

impl Relay {
    /// The provider key, for the content gate to look for in outbound text (ADR-0048).
    pub(crate) fn key(&self) -> &ApiKey {
        &self.key
    }

    pub fn new(
        adapter: Arc<dyn Adapter>,
        transport: Arc<dyn Transport>,
        key: ApiKey,
        meter: Arc<Meter>,
        sleep: Arc<dyn Sleep>,
    ) -> Self {
        Self {
            adapter,
            transport,
            key,
            meter,
            sleep,
            backoff: Backoff::default(),
        }
    }

    pub fn with_backoff(mut self, backoff: Backoff) -> Self {
        self.backoff = backoff;
        self
    }

    /// Answers one call from the job whose admitted manifest is `manifest`. A refusal is a 402,
    /// 403, 404, 413 or 400 in the protocol's error shape, never a 429 (ADR-0016).
    pub fn handle(&self, manifest: &Manifest, request: Request) -> Response {
        self.serve(manifest, &request)
            .unwrap_or_else(|r| refusal(self.adapter.as_ref(), r.status, r.code, &r.message))
    }

    fn serve(&self, manifest: &Manifest, request: &Request) -> Result<Response, Refused> {
        let path = request
            .target
            .split_once('?')
            .map_or(request.target.as_str(), |(p, _)| p);
        let route = self
            .adapter
            .route(path)
            .filter(|_| request.method.eq_ignore_ascii_case("POST"))
            .ok_or_else(|| {
                refused(
                    404,
                    "model.route_refused",
                    "the broker serves no model call at this path",
                )
            })?;
        // A space or control character would split the request line upstream.
        if !request.target.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(refused(
                400,
                "model.bad_request",
                "the request path is not one the broker passes on",
            ));
        }
        if request.body.len() > MAX_BODY {
            return Err(refused(
                413,
                "model.too_large",
                "the request is larger than the broker reads",
            ));
        }
        let Ok(Value::Object(mut body)) = serde_json::from_slice::<Value>(&request.body) else {
            return Err(refused(
                400,
                "model.bad_request",
                "the request body is not a JSON object",
            ));
        };
        // A background response keeps generating after this call returns, where no meter sees it.
        if body
            .get("background")
            .is_some_and(|b| b != &Value::Bool(false) && !b.is_null())
        {
            return Err(refused(
                400,
                "model.background_refused",
                "the broker does not serve background responses",
            ));
        }
        // Only fields the adapter names, or that the relay handles itself, are served (ADR-0120). The
        // field is not named back: it is the sandbox's text.
        let known = self.adapter.fields();
        if body.keys().any(|field| {
            !known.contains(&field.as_str()) && !RELAY_HANDLES.contains(&field.as_str())
        }) {
            return Err(refused(
                400,
                "model.field_refused",
                "the request has a field the broker does not pass on",
            ));
        }
        body.remove("background");
        if manifest.role == Role::Reader && declares_tools(&body) {
            return Err(refused(
                403,
                "model.reader_tools",
                "refused by the job's manifest: a reader may not send tool definitions",
            ));
        }
        // A generation holds the output it may produce, and asks for no more than the budget has left,
        // so calls side by side cannot all pass the budget together (ADR-0038).
        let hold = if route == Route::Generate {
            let budget = manifest.budget.tokens;
            let field = self.adapter.output_limit();
            let asked = body.get(field).and_then(Value::as_u64);
            let want = asked.unwrap_or(DEFAULT_HOLD);
            let hold = Arc::clone(&self.meter)
                .hold(manifest.job, budget, want)
                .map_err(|spent| {
                    if spent {
                        refused(
                            402,
                            "budget.spent",
                            format!("this job's budget of {budget} tokens is spent"),
                        )
                    } else {
                        // Calls in flight hold the rest and will release what they do not use.
                        refused(
                            self.adapter.overloaded(),
                            "budget.held",
                            "the rest of this job's budget is held by calls in flight; try again",
                        )
                    }
                })?;
            if asked != Some(hold.amount()) {
                body.insert(field.into(), hold.amount().into());
            }
            Some(hold)
        } else {
            None
        };
        strip_hosted(self.adapter.as_ref(), &mut body);
        let upstream = Upstream {
            target: request.target.clone(),
            headers: self.headers(request)?,
            body: serde_json::to_vec(&body)
                .map_err(|_| refused(400, "model.bad_request", "the request cannot be written"))?,
        };
        self.deliver(manifest, hold, &upstream)
    }

    fn headers(&self, request: &Request) -> Result<Vec<(String, String)>, Refused> {
        let pass = self.adapter.passthrough();
        let mut out: Vec<(String, String)> = request
            .headers
            .iter()
            .map(|(n, v)| (n.to_ascii_lowercase(), v.clone()))
            .filter(|(n, _)| pass.contains(&n.as_str()))
            .collect();
        if out
            .iter()
            .any(|(_, v)| v.bytes().any(|b| b.is_ascii_control()))
        {
            return Err(refused(
                400,
                "model.bad_request",
                "a request header has a control character",
            ));
        }
        for (name, value) in self.adapter.defaults() {
            if header(&out, name).is_none() {
                out.push(((*name).into(), (*value).into()));
            }
        }
        let (name, prefix) = self.adapter.credential_header();
        out.push((name.into(), format!("{prefix}{}", self.key.expose())));
        out.push(("content-type".into(), "application/json".into()));
        Ok(out)
    }

    fn deliver(
        &self,
        manifest: &Manifest,
        hold: Option<Hold>,
        upstream: &Upstream,
    ) -> Result<Response, Refused> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let retry_after = match self.transport.send(upstream) {
                Ok(r) if !TRANSIENT.contains(&r.status) => {
                    return Ok(self.finish(manifest, hold, r));
                }
                Ok(r) => {
                    let retry_after = header(&r.headers, "retry-after")
                        .and_then(|v| v.trim().parse::<u64>().ok());
                    if r.status == 429 {
                        let mut body = Vec::new();
                        // An unreadable body is treated as a rate limit, which is retried.
                        let _ = r.body.take(MAX_ERROR_BODY).read_to_end(&mut body);
                        if self.adapter.quota_exhausted(&body) {
                            return Err(refused(
                                402,
                                "provider.quota_spent",
                                "the model provider says this key's quota is spent",
                            ));
                        }
                    }
                    retry_after
                }
                // Nothing was sent, so trying again cannot pay for a generation twice.
                Err(e) if e == TransportError::UNREACHABLE => None,
                Err(e) if e == TransportError::TLS_REFUSED => {
                    return Err(refused(
                        502,
                        "provider.tls_refused",
                        "the model provider's TLS certificate or handshake was refused",
                    ));
                }
                // The provider may have generated already; the harness decides whether to ask again.
                Err(_) => {
                    return Err(refused(
                        502,
                        "provider.bad_response",
                        "the model provider's answer could not be read",
                    ));
                }
            };
            if attempt >= self.backoff.attempts {
                return Err(refused(
                    self.adapter.overloaded(),
                    "provider.overloaded",
                    "the model provider is busy or unreachable; try again",
                ));
            }
            self.sleep.sleep(self.backoff.wait(attempt, retry_after));
        }
    }

    /// The provider's answer. A successful generation is metered; anything else releases its hold.
    fn finish(&self, manifest: &Manifest, hold: Option<Hold>, r: UpstreamResponse) -> Response {
        let content_type = header(&r.headers, "content-type")
            .unwrap_or("application/json")
            .to_string();
        let body: Box<dyn Read + Send> = match hold {
            Some(hold) if (200..300).contains(&r.status) => Box::new(Metered::new(
                r.body,
                Scanner::new(&content_type, self.adapter.stream_ends()),
                hold,
                manifest.budget.tokens,
            )),
            _ => r.body,
        };
        Response {
            status: r.status,
            content_type,
            body,
            reason: None,
        }
    }
}

/// Top-level fields the relay handles itself, whatever the protocol: a background response, refused
/// unless it is `false`, and hosted MCP servers, removed (ADR-0120).
const RELAY_HANDLES: &[&str] = &["background", "mcp_servers"];

/// Whether a request defines any tool, of any kind, including MCP servers for the provider to call.
fn declares_tools(body: &Map<String, Value>) -> bool {
    ["tools", "mcp_servers"].iter().any(|k| match body.get(*k) {
        None | Some(Value::Null) => false,
        Some(Value::Array(a)) => !a.is_empty(),
        Some(_) => true,
    })
}

/// Removes every tool the provider would run, and whatever would make the request invalid without
/// them, so the request stays one the provider accepts (ADR-0016, ADR-0004).
fn strip_hosted(adapter: &dyn Adapter, body: &mut Map<String, Value>) {
    body.remove("mcp_servers");
    let mut removed: Vec<String> = Vec::new();
    let mut left = 0;
    if let Some(Value::Array(tools)) = body.get_mut("tools") {
        tools.retain(|tool| {
            let keep = adapter.harness_runs(tool);
            if !keep {
                removed.extend(
                    ["type", "name"]
                        .iter()
                        .filter_map(|k| tool.get(*k).and_then(Value::as_str))
                        .map(str::to_string),
                );
            }
            keep
        });
        left = tools.len();
    }
    if left == 0 {
        for key in ["tools", "tool_choice", "parallel_tool_calls"] {
            body.remove(key);
        }
    } else {
        let names_removed = body
            .get("tool_choice")
            .and_then(Value::as_object)
            .is_some_and(|choice| {
                ["type", "name"].iter().any(|k| {
                    choice
                        .get(*k)
                        .and_then(Value::as_str)
                        .is_some_and(|s| removed.iter().any(|r| r == s))
                })
            });
        if names_removed {
            body.remove("tool_choice");
        }
    }
    adapter.scrub(body);
}
