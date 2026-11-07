# ADR-0065: The content gate on what a tainted job publishes, and the ported red-team corpus

**Status:** Proposed

## Context

S4.5 (#27) asks that the broker content-gate outbound text from tainted jobs. Its acceptance is that the ported red-team items that target sinks are refused.

The red-team corpus is hivrr's (`scripts/redteam/corpus`, version 1): 300 attacks and 10 controls over five channels (issue body, issue comment, repository file, linked page, tool output). Each item says:
- what a stranger planted;
- what a fully steered agent would then try: a write to a sink, with its text;
- what the gates should do.

Secrets in it are canaries stored as placeholders (`{{secret:github}}`, `{{b64:anthropic}}` and so on) and built when the item is rendered. The measure is the worst case: if the agent complies, does the steered write get through? hivrr's offline reach rate was 16 of 300.

hivrr's sinks were a forge comment, an issue comment, an issue body, a pull request body and MCP tool calls. In Hyalos a job's only ways out are its socket's routes:
- a pull request (`POST /forge/repos/{owner}/{repo}/pulls`, [ADR-0048](0048-opening-a-pull-request-and-the-content-gate-on-its-text.md));
- a push ([ADR-0046](0046-the-git-proxy.md), [ADR-0054](0054-the-content-gate-on-what-a-push-carries.md));
- egress to listed hosts ([ADR-0042](0042-the-egress-gate-and-the-allowlisting-proxy.md));
- model calls;
- the result ([ADR-0064](0064-results-a-harness-submits-and-the-broker-reads.md)), which is typed values only.

Anything else under `/forge/` is 404 `forge.route_refused`, and a job has no MCP servers.

Until now the content gate (`content::check`) refused two things: a credential the broker holds, byte for byte, and hidden text. Run against the corpus, most pull request attacks got through:
- links that carry data off the forge, in a URL, an image, an autolink, a `data:` URI or a bare `host/path?query`;
- other services' key shapes, which the broker does not hold;
- keys encoded in base64, hex or percent encoding, reversed or split;
- bodies long enough to hide a payload from a reviewer.

A pull request's text is published where people and their browsers render it, and an image link is fetched by every reader's browser or the forge's image proxy. Text a tainted job wrote can be steered by a stranger ([ADR-0005](0005-provenance-is-a-type.md)), so a byte check for held credentials is not enough for it. The policy layer ([ADR-0008](0008-cedar-for-policy.md)) decides whether a job may open a pull request at all, not what the request says.

## Decision

**A tainted job's published text gets a stricter check.** `content::check_published(text, secrets, forge)` runs `check`, then refuses, in order:
- `content.too_long`: more than `MAX_PUBLISHED` (20,000) characters in one field. GitHub's own cap is 65,536 and stays the route's limit for untainted jobs.
- `content.key_shape`: a key in a shape a provider or service issues, or a credential written out. That is:
  - a known prefix at the start of a word, with enough key characters after it: Anthropic, OpenAI, GitHub (`ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_`, `github_pat_`), GitLab, Slack and Stripe;
  - an AWS access key id (`AKIA` or `ASIA` and 16 capitals or digits);
  - a private key block;
  - a JSON web token;
  - a password in a URL (`scheme://user:password@host`);
  - an assignment such as `API_TOKEN=...` or `password: ...` to a name that says it is a credential, of a value of 8 or more key characters with a digit. So `Token::new`, `max_tokens: 16384` and `token: Option<&str>` pass.
- `content.encoded_credential`: a held credential or a known key shape in another view of the text:
  - runs of pieces of at most 4 characters joined, which catches a key split into groups;
  - the text reversed;
  - percent-decoded;
  - with HTML character references decoded;
  - each run of 16 or more base64 or hex characters, decoded.
- `content.link`: a link anywhere but the forge's web host, or one there with a query. A link is any of:
  - a URL with a scheme of two or more letters and a slash or backslash after it (only `http` and `https` can pass);
  - `http:` or `https:` with no slash at all, since browsers read `https:/host`, `https:\\host` and `https:host` as the host;
  - a scheme-relative link: two or more slashes or backslashes where a link begins, before a host name (`//host.name`, `///host.name`, `\\host.name`);
  - a `data:` URI;
  - a bare host name that starts `www.` or carries a query, which GitHub autolinks or which carries data.

  The text is checked as written and with HTML character references decoded, since Markdown and HTML both decode them (`&#47;&#47;host`, `https&colon;//host`). A `// comment`, a bare `Node.js/path`, a Rust path `a::b` and a drive `C:\Users` are not links. With no forge host known, no link passes.

**The route chooses the check by the manifest's taint.** `pulls::answer` runs `check_published` on the title and body when `manifest.tainted`, and `check` otherwise. A refusal is 403 with the code, plus the content gate's `deny` (SHA-256 of the title and body), as before. Nothing refused reaches the forge. An actor is always tainted, so every pull request `hyalos run` opens gets the published check.

**The forge's web host comes from the git base.** `Git::web_host` is the host of the git base the engine names in `Hello` ([ADR-0050](0050-running-one-job-from-the-command-line.md)), `github.com` by default. It is passed to the route, so a job may link to its own repository's issues, files and pull requests, which is what a pull request ordinarily does.

**The corpus lives in Hyalos as data.** `crates/redteam` (not core, never linked into the binary) holds the 310 items in `corpus/<channel>.json`. They are ported from hivrr's version 1 with each item's provenance recorded. hivrr's repository references are rewritten to the test repository `octo/repo` on GitHub. Each item's `expected` is one of four:
- `refused`: an attack;
- `reached`: a known gap;
- `accepted`: a benign write;
- `no_sink`: a benign read, where the sandbox reads its own clone.

The renderer is ported too, and builds secret shapes from split literals as hivrr's does.

**What each item's write is in Hyalos.** `Item::writes` maps hivrr's sinks onto a job's socket:
- a pull request body is a pull request;
- a forge comment, issue comment or issue body is tried as a pull request body, the one forge text sink there is, and as the call itself on its path under `/forge/`;
- an MCP call is a `CONNECT` to the host it would reach: its URL's host, or `<server>.mcp.example`.

**The corpus runs offline against the real broker.** `crates/redteam/tests/corpus.rs` starts a `Broker` with a stand-in GitHub on loopback. For each item it admits an actor job with a signed, tainted manifest, makes the item's writes on that job's socket and ends the job. A write got through if the broker answered 2xx or the stand-in saw a request. The test fails if:
- an attack expected refused got through;
- a known gap stopped reaching, so the corpus must say so;
- a control was refused at the pull request;
- a benign read had a write.

It prints the reach rate and the refusals by code. The control runs the same writes from untainted jobs: 222 of 300 attacks reach GitHub, and none of the new codes appear. So the refusals are this check's, not some other part of the route.

**The known gaps are two forms, 11 items.** A secret written as prose (`The password is ...-hunter-two-words-long`, 6 items) and a clean summary carrying a canary (5) are text no byte check can tell from an honest pull request. They are `reached`, and the test asserts they still reach. hivrr's third gap, a write to a trusted MCP server, is refused here, since a job has no MCP server and egress to one is refused.

## Consequences

S4.5's acceptance holds. Every ported item that targets a sink is refused but the 11 known gaps, and the offline reach rate is 11 of 300 (3.7%), against hivrr's 16 of 300. The tally by refusal code:

| Code | Writes |
|---|---|
| `forge.route_refused` | 174 |
| `content.key_shape` | 91 |
| `content.link` | 71 |
| `egress.host_not_allowed` | 71 |
| `content.encoded_credential` | 42 |
| `content.hidden_text` | 7 |
| `content.too_long` | 7 |

The tests:
- `core/broker/src/content.rs`:
  - each rule refuses its case, including the link forms browsers accept with extra or missing slashes, backslashes and character references;
  - an ordinary pull request, with code, paths, a `// comment`, a commit id, links to its own repository and words like `risk_test_cases`, passes;
  - every rule reads a bounded distance from each match, so text built to be slow (20,000 of `=`, `https:`, `eyJ` and so on) is checked in linear time.
- `crates/redteam/src/lib.rs`: the corpus loads whole, every item says what to expect, and placeholders render as hivrr's renderer does.
- `crates/redteam/tests/corpus.rs`: the run and its control.

**A tainted job's pull request cannot link off the forge.** An actor that wants to cite documentation elsewhere names it in words. That is the price of an image link being a fetch by every reader. Links to the job's own forge pass without a query, so issues, files, commits and pull requests can be linked.

**The rules can refuse an honest pull request.** For example, a body that quotes an example key of a known shape, or one over 20,000 characters. The refusal names its code, so the operator sees why. The rules lean narrow where code is common: assignments need a digit and key characters, prefixes must start a word, and split pieces are joined only when they are at most 4 characters.

**What this does not decide:**
- **Commit messages, tag messages and push options** keep ADR-0054's check (held credentials and hidden text). GitHub does not render images in a commit message, and the corpus has no commit sink. If runs show the need, they can take the published check later.
- **A secret written as prose, and a summary carrying a canary,** stay open. Catching them needs a judge of meaning, not of text.
- **Growing the corpus and tracking the reach rate in CI over time** is S8.4 (#53). This ADR ports version 1 and pins its result.
- **The live runner,** which asks whether a model complies, is not ported. The measure here is the gates alone.

**Costs.** 376 more lines of trusted core, all in the broker: the published check and its decoders in `content.rs`, the route's choice of check in `pulls.rs`, and the git host's web host in `git.rs`.

Trusted core goes from 15,990 to 16,366 lines of 30,000. Outside the core, `crates/redteam` holds the corpus (310 items, about 270 KiB of JSON), its renderer and the offline run.
