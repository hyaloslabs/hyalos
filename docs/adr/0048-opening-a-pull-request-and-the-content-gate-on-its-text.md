# ADR-0048: Opening a pull request, and the content gate on its text

**Status:** Proposed

## Context

A reference job ends with a pull request: the actor pushes its branch through the git proxy ([ADR-0046](0046-the-git-proxy.md)) and then asks for that branch to be merged into the base. The reference-job walk found nothing that opens one (gap G4). That is story S3.12 (#79).

Three rules already bear on it:
- **[ADR-0004](0004-one-signed-manifest-per-job.md):** everything a job may do on the forge is in its signed manifest, the broker checks every call against it, and the broker content-gates a tainted job's outbound text.
- **[ADR-0008](0008-cedar-for-policy-code-for-content-gates.md):** byte-level content gates are code in the broker, not Cedar.
- **[ADR-0047](0047-forge-content-as-typed-values-and-derived-taint.md):** an actor's manifest is always tainted, so every pull request the reference job opens comes from a tainted job.

A pull request's title and body are the first outbound text a job writes to the forge in clear. A person reads them before deciding to merge. Two things in that text would do harm:
- a credential the broker holds, published where anyone with access to the repository can read it;
- characters that make the text show the reader something other than what it says. These are bidirectional overrides, zero-width characters and Unicode tags, which can carry a whole hidden message.

No ADR said what the content gate checks. #183 (S3.14b) was filed from #85 to decide it.

## Decision

**A job opens a pull request on its own socket, as it would ask GitHub's REST API** (`hyalos_broker::pulls`).
- The call is `POST /forge/repos/{owner}/{repo}/pulls` with `title`, `body`, `head`, `base` and `draft`, GitHub's field names, so a harness or tool that already speaks the API needs only a different base URL.
- Nothing else under `/forge/` is served. Merging, issues, comments, GraphQL and every other method or path get a 404 (`forge.route_refused`).
- The body is held to 512 KiB and parsed against a closed type. Any other field, `maintainer_can_modify` included, is a 400 (`forge.bad_request`). So is a title that is empty or over GitHub's 256 characters, or a body over its 65,536 characters.

**The manifest says where a pull request may go.**
- The `open_pr` action must be in the manifest (`forge.action_not_allowed`).
- The repository in the path must be the manifest's, ignoring ASCII case (`forge.repo_not_allowed`).
- The head must be a branch of that repository that the job may push: inside `branch_prefix` by the git proxy's own rule, and not `owner:branch`, which names another fork (`forge.head_not_allowed`).
- The base must be exactly the manifest's new `pr_base` and a well-formed branch name (`forge.base_not_allowed`).
- `Manifest.pr_base` is optional. A manifest without it opens no pull request. When absent it is left out of the signed bytes, so every manifest signed before it, and its pinned form, is unchanged. The manifest version stays 1.
- The job's forge token is needed (`forge.no_token`), and the job must be live and within its deadline.

**The content gate checks the title and the body before anything reaches the forge** (`hyalos_broker::content`). Text is refused when it holds either of these:
- **A credential the broker holds** (`content.credential`): the job's forge token, or a provider key. Anything shorter than 12 bytes is not looked for, since it would match ordinary words.
- **Text that hides from its reader** (`content.hidden_text`):
  - control characters other than tab and line breaks;
  - the soft hyphen and other invisible fillers;
  - zero-width spaces and joiners, and the left-to-right and right-to-left marks;
  - line and paragraph separators;
  - bidirectional embeddings, overrides and isolates;
  - invisible operators and deprecated format characters;
  - the byte-order mark;
  - interlinear annotation characters;
  - Unicode tags.

  Ordinary text in any script, emoji included, passes. The list is a closed match in code, so changing it means changing this ADR.

The gate applies to every job's pull request, not only a tainted one's. Every actor is tainted already, and a check that refuses only what nobody should publish costs an untainted job nothing.

**A refusal is reported.**
- Every refusal by the manifest's rules is a 403 to the harness and the manifest gate's `deny` to the engine.
- A content refusal is a 403 and the content gate's `deny` (`Gate::Content`).
- Each report carries a SHA-256 of what was refused: the repository, head, base or action, or the title and body together.
- They share the per-job caps of the git proxy's reports (`Reports::for_gate`), and the content gate gets its own (`content.too_many_refusals`).
- Nothing is sent to the forge for a refused request.

**The broker opens the pull request and passes back only what the job needs.**
- It calls `POST /repos/{repo}/pulls` with the job's token, the checked fields, and `maintainer_can_modify: false`, so nobody but the job's branch writes to its head.
- A 201 answer becomes `{number, html_url}`, and the URL must be an `http(s)` address of at most 512 bytes without control characters. Nothing else the forge said reaches the harness.
- A failure is mapped and never relayed: 401 or 403 is `forge.upstream_refused`, 404 is `forge.upstream_not_found`, 422 is `forge.pull_rejected` (invalid, or one is already open for that head), anything else, or an answer without a number and a usable URL, is a 502 with `forge.upstream_status` or `forge.malformed`.

## Consequences

The reference job can now end with a pull request the operator chose the base of. An injected instruction cannot use one to reach another repository, another fork's branch, a base the operator did not name, or a merge, which has no route.

The content gate is a byte check, and it cannot catch:
- a secret the job has encoded, split or paraphrased;
- a credential the broker does not hold, such as one in the repository;
- a misleading pull request written in plain characters.

Review is the defence against those. #183 keeps the rest of the question open: whether push contents, which are compressed packs, are inflated and scanned, and the gate for comments once they exist.

The engine does not yet judge `pr_base`. `mint_manifest` asks the policy about `open_pr` as one action, and the broker checks the base. Holding a pull request into a protected base for approval, or into one the operator's policy names, needs `pr_base` among the policy's facts, which is left for when the policy has a reason to know it.

GitHub's REST answer is not checked for unknown fields as ADR-0047's issue reads are. Only two fields are taken from it, and nothing of it reaches the engine.

Tests (`core/broker/tests/pulls.rs`) open a pull request through a real job socket against a stand-in GitHub that checks the token. They cover every refusal above, reported with its gate and digest and never reaching GitHub; each class of character and both kinds of credential; and every answer mapping. GitLab merge requests and GitHub Enterprise Server are not built.
