# ADR-0042: The egress gate and the allowlisting proxy

**Status:** Accepted

## Context

A harness in the sandbox has no network. Its model calls reach the broker through the forwarder and the job's socket ([ADR-0038](0038-model-calls-from-the-sandbox-and-the-provider-key-file.md)). Its other traffic, such as a web fetch or a `curl` in a shell command, is sent to the forwarder as a proxy request, because the image sets `HTTPS_PROXY` and `HTTP_PROXY` ([ADR-0040](0040-the-sandbox-image.md)).
- [ADR-0016](0016-agent-runtime-and-broker-protocols.md) decides that the broker is an allowlisting proxy for that traffic: anything to a host outside the manifest's egress hosts is refused, IP literals included.
- [ADR-0009](0009-gates-answer-allow-deny-unknown.md) makes egress one of the gates the broker evaluates per request.
- [ADR-0024](0024-event-origins-and-time-order.md) makes a gate's answer the one fact the broker reports for the engine to record.

Until now the broker refused every proxy request as a malformed call. This is story S3.5 (#20).

## Decision

**A request that is not a model call is a proxy request, and the egress gate decides it.** On a job's socket:
- `CONNECT host:port` asks for a TLS tunnel;
- a request whose target is an absolute `http://` URL asks for plain HTTP;
- a target that starts with `/` is a model call, as before.

The gate (`hyalos_broker::egress`) allows a proxy request only if all of these hold:
- **The job is live, and its deadline has not passed.**
- **The target is a host name.** Any IP literal is refused (`egress.ip_literal`), whether or not the manifest lists it. That includes numeric forms a resolver would read as an address, such as `127.1` or `0x7f000001`. A host must be letters, digits and hyphens in dot-separated labels, and is compared lowercase without a trailing dot. Anything else is `egress.bad_target`.
- **The port is the one for the request:** 443 for CONNECT, 80 for plain HTTP (`egress.port_not_allowed`).
- **The host is one of the manifest's egress hosts, and is not a model provider's API host** (`egress.host_not_allowed`). `api.anthropic.com` and `api.openai.com` are refused even when listed. ADR-0016 already says a prompt-injected agent could use one of them with an attacker's key; model calls go through the relay with the broker's key.
- **The host resolves to a public address** (`egress.not_public`). The broker resolves the absolute name (with a trailing dot), so the resolver's search domains cannot turn it into another host. It resolves once, and connects only to the addresses it kept. The resolver keeps only public unicast addresses. Loopback, private networks, link-local (which includes cloud metadata at `169.254.169.254`), shared, benchmarking, documentation, multicast and reserved ranges are dropped, as are IPv6 forms that carry an IPv4 address or stand for one. So a DNS answer cannot point an allowed name at the broker's own host or network. A test can supply another resolver from code (`Egress::with_resolver`), as it can other TLS roots (ADR-0039); nothing in configuration replaces the resolver.

**What the broker does with an allowed request.**
- **CONNECT:** the broker connects to the first public address that answers, replies `200 Connection established`, and relays bytes both ways. It first sends on any bytes the harness wrote after its CONNECT, such as a TLS hello. The harness's TLS runs end to end, so the broker sees neither content nor credentials.
- **Plain HTTP:** a request that the host could read as a different request is refused with 400 (`egress.bad_request`) before anything is sent. That means a method that is not capital letters, a path with a space or control byte, a header name that is not a token, or a header value with a control byte other than a tab. Otherwise the request is sent in origin form, with `host` set to the destination. The hop-by-hop and proxy headers are dropped: `proxy-authorization`, `proxy-connection`, `connection`, `keep-alive`, `te` and `upgrade`. The body must have a `content-length`, and the response is relayed until the host closes.
- **Ending:** a relay ends when both ends close, after 300 seconds with no bytes moving either way, or when the job ends. Writes have short timeouts and are retried until one of those happens, so a host or harness that stops reading cannot hold a relay, or its connection slot, after the job ends. The deadline is checked when a request arrives; a tunnel already open runs until one of those ends, as a model stream does.

A host that does not resolve or does not answer is a 502 (`egress.unresolved`, `egress.unreachable`): the request was allowed, and the gate did not refuse it.

**A refusal is a 403 to the harness and the egress gate's `deny` to the engine.** The broker sends `ToEngine::Gate { gate: egress, answer: deny, reason, inputs }`, the message ADR-0035 already defines. It goes out when the refusal happens rather than in answer to an engine message, so `serve_with` now writes every frame from one writer thread. Answers and reports therefore leave in the order they were made.

`inputs` is the SHA-256 of `host:port` as the broker read it, or of the target as written when it is not a host and port. The log holds the digest, not the name ([ADR-0003](0003-event-log-is-the-database.md)): anyone holding a candidate name can check it against the digest, and the channel still carries no free text.

A refusal made after the job has ended is not reported. Each job reports each distinct refusal once, and at most 32 of them. After that, one more report with `egress.too_many_refusals` says that further ones were not reported. A harness repeating a refused request therefore fills neither the channel nor the log. Allowed requests are not reported.

**The image test checks all of this with the real harnesses.** `crates/sandbox-tools/tests/egress.rs` runs the image as the container substrate would, against the real broker:
- `allowed.hyalos.test` is an HTTPS server on loopback. Its certificate comes from a test authority whose key was discarded, and only the image's `test` target trusts that authority.
- The broker's test resolver maps that one name to the server.
- A scripted model asks the harness to fetch from the server and from `elsewhere.hyalos.test`. Claude Code uses its web fetch tool and Codex runs `curl` with its shell tool. The test target installs curl from the Debian snapshot the base image was built from, with apt told to bypass the image's proxy variables.

For the allowed host, each harness must:
- reach the host;
- pass its page to the model;
- make no lookup;
- send nothing on the channel but model calls and the one CONNECT.

For the unlisted host, each harness must:
- reach nothing;
- produce exactly one report, the gate's `deny` for that host.

No request goes to `api.anthropic.com`, so `skipWebFetchPreflight` works. That was moved here from #166 ([ADR-0041](0041-an-init-in-the-sandbox-and-the-image-tests-further-checks.md)). A control empties Claude Code's managed settings, and its first request is then `CONNECT api.anthropic.com:443`. The `sandbox image` workflow runs this test along with the image test.

## Consequences

A job can reach exactly the hosts its manifest names, over TLS end to end, and nothing it refuses passes unrecorded. Any target that is not a path is now a proxy request, so a malformed model call such as `POST v1/messages` gets a 403 from the gate (`egress.bad_target`) and a report, where it used to get a 400. The engine will therefore receive `gate` messages between its answers, and its reading loop must take them in any order. The engine still has no loop that reads the broker's messages and records them (`Engine::submit` with `Origin::Broker`). That comes with the first command that starts the engine. Until then the reports are on the channel and tested there.

Not here:
- **Declared data flows**, which name what may move to a host rather than which hosts. Enforcing them needs the content of a request, and a TLS tunnel hides it.
- **Ports other than 443 and 80.**
- **Wildcard hosts.**
- **Other provider endpoints.** Only those two exact names are refused. Subdomains of them, and an operator's own `anthropic.base` or `openai.base` host such as a corporate gateway, are not.
- **Recording allowed requests.**
- **Rate limits on a tunnel.**
