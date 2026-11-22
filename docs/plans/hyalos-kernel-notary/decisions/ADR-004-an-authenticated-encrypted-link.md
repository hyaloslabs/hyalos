# ADR-004: The operator and the notary talk over a mutually authenticated, encrypted link with pinned keys

**Status:** accepted (developer, 2026-11-21)

**Proposed by:** author (spec); accepted by the developer at the spec ADR checkpoint

## Context

The two sides were joined by an in-memory pipe. They will now be on different machines. An open port would let anyone on the network have the customer's notary sign attestations, and use its memory and CPU. The operator must also know it is talking to the customer's notary before spending a model call. Whether the protocol's messages are safe for an eavesdropper to read was not established in the spike.

## Options

1. **A plain connection on a network both sides trust.** Rejected: anyone who can reach the port can obtain attestations under the customer's key, and nothing was shown about what an eavesdropper learns.
2. **Leave it to a tunnel the customer sets up.** Rejected: the security of the link would depend on installation notes, and a wrong setup fails open.
3. **Certificates issued by an authority.** Rejected: an authority to run and renew, for a link between exactly two parties who already exchange public keys.
4. **The operator proves it holds its ledger key by signing a challenge, with no encryption.** Rejected: it authenticates the operator and leaves the eavesdropper question open.
5. **A standard encrypted transport in which each side pins the other's public key.** Chosen.

## Decision

Each side has a transport key of its own, separate from any signing key. The customer gives the notary the operator's transport public key, and the operator is configured with the notary's transport public key and its signing public key. A session starts only when both pinned keys match. A connection from anyone else is closed before the protocol starts.

## Consequences

- The customer enrols an operator by one public key, and can remove it.
- Each side has one more key to keep. The operator's is a secret an agent working on this repository must never read.
- The notary's signing key and its transport key are different keys, so the link can be re-keyed without changing what auditors trust.
- A stranger who reaches the port learns that something is listening and nothing else.
