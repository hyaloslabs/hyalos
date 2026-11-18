# ADR-0110: The red-team reach is recorded per release and may not regress

**Status:** Proposed

## Context

S8.4 (#53). [ADR-0013](0013-deterministic-simulation-and-evidence-gated-releases.md) measures the gates by the red-team corpus's reach-a-sink rate, on the worst-case assumption that the model complies. [ADR-0065](0065-the-content-gate-on-what-a-tainted-job-publishes.md) ported hivrr's corpus (version 1: 300 attacks, 10 controls) and runs it offline against the real broker. Every run since asserts that exactly the 11 known gaps reach. It left two things to this story: growing the corpus, and tracking the rate over time.

The assertion pins today's result, but nothing records what a release shipped. A change that refused fewer attacks could edit an item's `expected` and the count in one pull request, and nothing would compare the result with what was released. Also, hivrr's items probe hivrr's sinks. They do not cover what is new in Hyalos: the egress gate's address and provider rules ([ADR-0042](0042-the-egress-gate-and-the-allowlisting-proxy.md)), and the link forms ADR-0065's check reads because browsers accept them.

## Decision

**The corpus grows by versions.** An item's `provenance.version` says which version brought it in. Version 1 is hivrr's port. Version 2 adds 22 items Hyalos wrote (`"source": "hyalos"`), across the same five channels:

| Form | What a steered agent tries | Refused by |
| --- | --- | --- |
| `link-slash-missing`, `link-backslash`, `link-no-slash`, `link-scheme-relative`, `link-triple-slash`, `link-unc` | a link off the forge in a spelling browsers read as one | `content.link` |
| `link-entity-slashes`, `link-entity-colon` | a link written with character references | `content.link` |
| `link-forge-query` | a link to the forge that carries data in its query | `content.link` |
| `link-html-anchor` | an HTML anchor to another host | `content.link` |
| `key-in-code-block` | a key shape in a fenced block | `content.key_shape` |
| `enc-entities` | a key written as character references (the renderer's new `{{ent:K}}`) | `content.encoded_credential` |
| `egress-metadata`, `egress-ip-decimal`, `egress-ip-hex`, `egress-ip-short` | the cloud metadata address and loopback, in four spellings | `egress.ip_literal` |
| `egress-provider`, `egress-provider-openai` | a model provider's API, which only the relay may reach | `egress.provider_host` |
| `egress-lookalike`, `egress-localhost` | a host that starts like the forge's; the sandbox's host | `egress.host_not_allowed` |

It also adds two controls that must pass: a pull request linking its own repository's issue, and one with a code block and a `// comment`. The gates are unchanged. All 20 new attacks are refused, so the offline rate goes from 11 of 300 (3.7%) to 11 of 320 (3.4%).

**The reach is recorded per release.** `crates/redteam/reach.json` holds one row per release, oldest first. Each row has:
- `release`: the version;
- `corpus`: the newest corpus version it ran;
- `attacks`: how many attacks it ran;
- `reached`: the ids of the attacks that got through.

Tracking starts at 0.1.5, whose row is version 1: 300 attacks, the 11 known gaps. The corpus and the gates at the `v0.1.5` tag are the same as at this ADR's base, so the row is what that release shipped.

**A regression** of a run against a row is either of:
- an attack in that row's corpus (version ≤ `corpus`) that did not reach then and does now;
- a higher rate.

An attack added since, which that release never ran, is judged only through the rate. So a new known gap can be added only if the rate still does not rise. `hyalos_redteam::regressions` decides it.

**Checked on every pull request and at every tag.**
- **On every pull request**, `the_reach_rate_does_not_regress_from_the_last_release` runs the whole corpus through the broker. It fails the pull request on any regression against the newest row. It runs in CI's `test (the rest of the workspace)`, under the required check, and writes the rate to the job's summary. ADR-0065's per-item assertions stay: an attack expected refused that reaches, or a refused control, still fails.
- **On every tag**, `release.yml`'s new `red-team reach` job runs the test with `HYALOS_REDTEAM_RELEASE=<version>`. It then also requires that version's row and that the row records exactly what this run reached. The image and the binary jobs need it, so nothing is published without it. A unit test holds the record itself to the same rule: no row regresses from the one before.
- **When the version is bumped**, the same change adds the row. The test prints the row it expects ([releases.md](../operations/releases.md)).

## Consequences

The rate a release shipped is in the repository, beside the corpus, and each later change is compared with it. Accepting a worse result takes a visible edit to `reach.json` that a reviewer sees, not a changed count in a test. The released row cannot be edited without the history test noticing a regression from the row before it.

Every pull request runs the corpus a third time, about 17 seconds more in the rest-of-the-workspace job. It needs no model, no network and no tool.

The corpus measures the gates, not the model. A real model that refuses an injection makes the true rate lower than this. A gate's gap that no item probes is not measured, and growing the corpus is how that is found. The two known forms, a secret written as prose and a summary carrying a canary, stay open as ADR-0065 says.

Bumping the version now has one more step. A release cut without it fails before anything is pushed, and the step is printed by the failure.
