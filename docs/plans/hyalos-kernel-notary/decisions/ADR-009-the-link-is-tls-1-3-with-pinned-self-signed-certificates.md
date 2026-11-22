# ADR-009: The link is TLS 1.3 with pinned self-signed certificates, and the notary names a release mismatch

**Status:** accepted (developer, 2026-11-21)

**Proposed by:** author (slice, cold-read); accepted by the developer on 2026-11-21, after the cold read passed

## Context

ADR-004 decided that the operator and the notary talk over a mutually authenticated, encrypted link with pinned transport keys. It did not fix the transport. The author fixed it while slicing, and in cold-read round 10 changed the first message's answer so the operator's tool can tell a release mismatch from a dropped connection. The developer confirmed both. This adds to ADR-004 and changes nothing in it.

## Options

1. **A handshake of our own over TCP, built on the pinned keys.** Rejected: a new protocol to get wrong, for nothing TLS does not already give.
2. **TLS with a certificate authority.** Rejected: the two sides already hold each other's keys, and an authority adds a party neither needs to trust.
3. **TLS 1.3 with self-signed certificates, each accepted only when its public key equals the pinned one.** Chosen.

For the first message's answer:

1. **The notary closes without a word on a release mismatch.** Rejected: the operator's tool could not tell it from a dropped connection, so the run's result link could not say which (S-02.4, and the prd's Failure modes).
2. **The notary answers `release` and then closes.** Chosen.

## Decision

The link is TLS 1.3 over TCP with the operator as the client. Each side presents a self-signed certificate for its own transport key and accepts exactly one peer, by public key. No certificate authority, name or expiry is consulted. After the handshake the operator sends the release identifier and the run's intent hash, and the notary answers `ok`, or answers `release` and closes when the release differs. `design.md` §2, The link between the two programs, holds the whole contract.

## Consequences

- The answer `release` tells a peer only that the releases differ. It is sent only to a peer whose transport key is enrolled.
- A changed transport key on either side needs the other side to be given the new public key.
