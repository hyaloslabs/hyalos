# Leaving TLSNotary's beta pin

hyalOS pins TLSNotary (`tlsn`, `tlsn-core`, `tlsn-attestation`) at tag `v0.1.0-beta.1`, locked at
commit `40a069565a2eaecbb5e691dbc634be20e8498daf` (`attest/Cargo.toml`, `attest-check/Cargo.toml`,
`Cargo.lock`). This policy is ADR-005 of `docs/plans/hyalos-kernel-core`, "Leaving the beta pin",
written out for whoever proposes a move. It applies to any change of the tag or of the locked
commit: to a stable release, to another beta, or to a new commit under the same tag.

`cargo xtask review-scope` fails as soon as the tag or the commit in `Cargo.lock` differs from the
TLSNotary rows of `review/scope.toml`, so a move cannot land without its rows being read again.

## What a move needs

The move proceeds only when every item holds, and the pull request shows each one.

1. **A pull request of its own.** It changes the pin and what the pin forces to change with it:
   the TLSNotary rows of `review/scope.toml` (tag, commit, and the date each `upstream_audit` was
   searched again), `Cargo.lock`, any other row whose locked version moves with it, and any
   assessment under `advisories/` whose versions move. Nothing else.
2. **The forgery corpus passes on the new version.** Every forged presentation in
   `verify/tests/corpus.rs` and `monitor/src/forgery_tests.rs`, and every one `forging-notary`
   (`attest/src/bin/forging-notary.rs`) makes, is still refused, each with the same reason.
3. **The presentation differential passes.** Presentations made under the old pin and the new one
   are judged alike by the verifier, and every committed record still audits unchanged. No such
   test exists at this pin; the first move adds it, in the same pull request, and it must pass
   there before the move does.
4. **A fresh provider canary passes** on the new version (`attest/src/bin/canary.rs`), run by the
   operator in a terminal of their own, with its result named in the pull request.
5. **A changelog read.** The pull request names every change between the two tags to the
   presentation format: the bytes a presentation or attestation holds, how they are encoded, and
   what they commit to. It says "none" when there is none.

## When the presentation format changes

A change to the presentation format needs a new record decision, an ADR, before the pin moves.
Records made under the old format must still audit, so the decision says how. The move then waits
for that ADR, whatever the evidence above shows.

## Applying it

| A proposed move | Evidence | Outcome |
|---|---|---|
| A release with the forgery corpus, the presentation differential and a fresh canary passing, and a changelog read that names no format change | Items 1 to 5 | It may proceed |
| A release whose changelog read names a change to the presentation format | Item 5 names it | It needs a new record decision first, then items 1 to 5 |
| A release where any item fails or is missing | | It does not proceed |

## What this does not change

The review scope's position on TLSNotary stays as ADR-005 gives it: our use covered, its protocol
and cryptography excluded. A move re-reads the `use` rows; it does not bring the internals into the
review.

An upstream advisory against the pinned tag is a different path: the daily advisory job
(`.github/workflows/hyalos-advisories.yml`) fails, and whether to move or to record an assessment is
the developer's decision. A move made for an advisory still needs every item above.
