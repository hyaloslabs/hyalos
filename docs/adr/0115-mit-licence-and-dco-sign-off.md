# ADR-0115: MIT licence, DCO sign-off, and a secrets scan of the full history

**Status:** Proposed

## Context

The repository is to be made public at launch (ADR-0014, E10). Until now our crates declared no licence, `deny.toml` skipped them, and the README said "MIT (planned for v1)". People who contribute need terms that say what they grant. Making a repository public also publishes every commit it has ever had, so a secret in any old commit would leak at that moment.

The full history holds three private keys, and all three were made for the tests. One is the GitHub App test key (ADR-0044). The other two are the keys of TLS leaf certificates whose authorities trust nothing real (ADR-0039). A scan with no exceptions reports them and can never come back clean.

## Decision

Hyalos is licensed under MIT. `LICENSE` holds the text, with copyright held by "The Hyalos contributors". The workspace declares `license = "MIT"` and every crate inherits it.

Contributors sign off each commit under the Developer Certificate of Origin 1.1 (`git commit -s`). There is no contributor licence agreement. Contributors keep their copyright and license their work under MIT, as they would to any MIT project. `CONTRIBUTING.md` holds the DCO's text and the steps.

The secrets scan is gitleaks with its default rules, configured by `.gitleaks.toml` at the root. The config names, by exact path, every key that is allowed to be in the history and says why that key is harmless. Adding an exception is a reviewed change to that file. Before the repository is made public, the operator runs the scan over all refs, and it must come back clean.

## Consequences

Anyone can use, change and ship Hyalos under a licence that the dependency audit already accepts. Without a CLA, the project cannot relicense contributed code on its own, and we accept that.

For now the DCO is a commitment, not a mechanism. No check enforces the sign-off yet, so the maintainer confirms it before merging an outside contribution. Enforcing it (the DCO GitHub App, or a CI job) is the operator's choice when the repository opens.

The scan is repeatable by anyone, and its exceptions are visible. Any new test key needs an entry, which keeps a real key from passing as a fixture without review. Untracked files such as a local `.env` are outside the history, so the scan does not cover them.
