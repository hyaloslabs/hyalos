# Leaving TLSNotary's beta pin

hyalOS pins TLSNotary (`tlsn`, `tlsn-core`, `tlsn-attestation`) at tag `v0.1.0-beta.1`, locked at
commit `40a069565a2eaecbb5e691dbc634be20e8498daf` (`attest/Cargo.toml`, `attest-check/Cargo.toml`,
`Cargo.lock`). This policy is ADR-005 of `docs/plans/hyalos-kernel-core`, written out for whoever
proposes a move. It applies to any change of the tag or of the locked commit, to a stable release
or to another beta.

## What a move needs

1. **A pull request of its own.** It changes the pin and what the pin forces to change with it (the
   rows of `review/scope.toml`, `Cargo.lock`, and any assessment in `advisories/` whose versions move),
   and nothing else.
2. **The forgery corpus passes on the new version.** Every forged presentation in
   `verify/tests/corpus.rs` and `monitor/src/forgery_tests.rs`, and those `forging-notary` makes, is
   still refused, with the same reason.
3. **The presentation differential passes.** Presentations made under the old pin and the new one
   are judged alike by the verifier, and every committed record still audits unchanged. No such
   test exists at this pin; the first move adds it.
4. **A fresh provider canary passes** on the new version, run by the operator, with its result
   named in the pull request.
5. **A changelog read.** The pull request names every change between the two tags to the
   presentation format: the bytes a presentation or attestation holds, how they are encoded, and
   what they commit to. It says "none" when there is none.

## When the presentation format changes

A change to the presentation format needs a new record decision, an ADR, before the pin moves. The
move then waits for that ADR, whatever the evidence above shows.

## What this does not change

The review scope's position on TLSNotary stays as ADR-005 gives it: our use covered, its protocol
and cryptography excluded. A move re-reads the `use` rows; it does not bring the internals into the
review.
