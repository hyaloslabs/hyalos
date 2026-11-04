# ADR-0039: TLS to model providers

**Status:** Proposed

## Context

The broker makes every model call with the provider's key ([ADR-0002](0002-one-binary-two-processes-sandboxes.md), [ADR-0038](0038-model-calls-from-the-sandbox-and-the-provider-key-file.md)). [ADR-0037](0037-model-protocol-adapters-and-their-line-budget.md) gave it a transport that speaks plain HTTP to a loopback server only, and refused an `https://` base. That reaches a local model, but not `api.anthropic.com` or `api.openai.com`. Adding TLS puts a large dependency in the trusted core. This ADR decides which library, which roots, and what that adds (story S3.15b, #159).

## Decision

**rustls, on the `ring` crypto provider.** `hyalos-broker` depends on `rustls` 0.23 with default features off and `std`, `ring` and `tls12` on. That means TLS 1.3, plus 1.2 for servers that lack it, with rustls's safe default cipher suites and no logging. rustls is memory-safe Rust and widely reviewed. Its default provider, `aws-lc-rs`, needs CMake and a larger C build. `ring` needs only the C compiler the bundled SQLite already requires. OpenSSL through `native-tls` would tie the broker to whichever version the host has.

**The roots are the Mozilla set, compiled in.** The broker trusts the certificate authorities in `webpki-roots`, a copy of Mozilla's root store that is part of the build. The roots are therefore the same on every host. They change only when the dependency is updated, which is a reviewed change, and a root added to a host's store (by malware, an employer or a proxy) is not trusted. In exchange, a network that intercepts TLS with its own certificate authority cannot be used. Supporting one would take an explicit operator setting and its own ADR.

**Certificate checks cannot be turned off.** No setting in `provider.keys`, the environment or the command line touches TLS. An unknown name in the key file is refused, as before. `HttpTransport::new` always uses the compiled-in roots. `HttpTransport::with_roots_der` takes other roots, and exists so tests can trust their own test authority. Only code calls it; nothing read at run time reaches it. The handshake, including the certificate and name check, completes before the request is written, so the key is never sent to a server that fails it.

**One transport, two schemes.** `HttpTransport` takes `https://host[:port]` (any host, port 443 by default) or `http://host[:port]` (loopback only, as in ADR-0037). Both share the same request writing, response reading, chunked decoding and timeouts. A failed handshake or certificate is `transport.tls_refused`, which the relay answers with 502 at once and does not retry, since retrying cannot fix a certificate. A host that cannot be reached is still `transport.unreachable` and is retried.

**What it adds.**
- Crates: `rustls`, `rustls-webpki`, `rustls-pki-types`, `ring`, `untrusted` and `webpki-roots`. `zeroize`, `subtle`, `once_cell` and `getrandom` were already in the build.
- Licenses in `deny.toml`: `ring`, `rustls-webpki` and `untrusted` bring `ISC`; `webpki-roots` brings `CDLA-Permissive-2.0`, a permissive license for data.
- The new code fits the existing 1,200-line budget for `core/broker/src/model`: it is at 1,009 lines.

## Consequences

The broker reaches `api.anthropic.com` and `api.openai.com` with the keys in `provider.keys` and the default bases. The tests in `core/broker/tests/model_tls.rs` run a TLS server on loopback with a `localhost` certificate from a test authority kept in `core/broker/tests/fixtures/tls`. That certificate is valid for 100 years, and the authority's private key was discarded when it was made, so nothing else can be issued from it. The tests show four things:
- a stream and a sized response pass through the relay as they do over plain HTTP;
- the Mozilla roots refuse the test certificate before any request is sent;
- a certificate for another name is refused;
- a server that does not speak TLS is refused, and one that is not there is unreachable.

The roots age with the dependency, so `webpki-roots` must be kept current like any other security dependency. Revocation is not checked: rustls with these roots does no OCSP or CRL lookup, which matches common practice for API clients. Not here: HTTP/2, connection reuse between calls, and a proxy for the broker's own outbound traffic.
