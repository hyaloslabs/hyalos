# ADR-0045: Forge tokens up to 4 KiB, and revoking a token the engine cannot use

**Status:** Proposed

## Context

[ADR-0044](0044-minting-per-job-github-tokens-from-the-app.md) caps a `ForgeToken` at 255 printable ASCII characters. GitHub's installation tokens used to be 40, so 255 looked like room enough.

The first run against real GitHub (`hyalos github check` on a test repository, App `hyaloslabs-forge-test`) found otherwise:
- **Tokens are longer now.** GitHub minted a 390-character `ghs_` token. Everything else matched what ADR-0044 expects: the JWT, the installation lookup, the permissions granted (`contents: read`, `metadata: read`), the repository selection and the one repository.
- **A refused token stayed alive.** The engine refused the token as too long, and because it refused it before holding it as a `ForgeToken`, it could not revoke it. A token GitHub had issued stayed live until it expired, up to an hour.

## Decision

**The cap is 4,096 characters** (`MAX_FORGE_TOKEN`). That is room for GitHub's tokens to grow again, still fits an HTTP header, and is far under the channel's frame cap. The rest of the check is unchanged: printable ASCII, no space, at least one character. This replaces only the "1 to 255" in ADR-0044's *Handing it over*; everything else there stands.

**Once GitHub has issued a token, every refusal revokes it.** That covers a wider grant (as before), and now also an expiry the engine cannot read and a token it will not accept. The engine revokes using the token text GitHub returned. If that text could not go in a header, the engine cannot send it back, and the token expires on GitHub's clock.

The stand-in GitHub in `core/hyalos/tests/github_app.rs` now issues tokens about as long as GitHub's, and a test shows that a token with an expiry the engine cannot read is revoked.

## Consequences

`hyalos github check` and minting work against github.com as it is today. A token issued and then refused no longer outlives the attempt. A change in GitHub's token format beyond 4 KiB, or to characters a header cannot carry, would be refused and revoked, which fails closed. The real run is the evidence that the stand-in's request and response shapes match GitHub's. It is a manual check, not one CI runs.
