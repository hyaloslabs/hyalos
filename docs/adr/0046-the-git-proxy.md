# ADR-0046: The git proxy

**Status:** Proposed

## Context

A job clones its repository and pushes its work. [ADR-0004](0004-one-signed-manifest-per-job.md) gives the manifest a repository and a branch prefix. [ADR-0008](0008-cedar-for-policy.md) requires that no manifest grants a write outside them. [ADR-0002](0002-one-binary-two-processes-sandboxes.md) puts every credential outside the sandbox.

[ADR-0044](0044-minting-per-job-github-tokens-from-the-app.md) gives the broker a forge token for each job that needs one. It is scoped by GitHub to the one repository and the permissions the actions need, but GitHub knows nothing of the branch prefix. A token with `contents: write` can push any branch, tag or deletion. So the prefix has to be enforced by the broker, on the way to the forge.

This is story S3.3 (#18): a push outside the manifest's repository or branch prefix is refused and audited.

## Decision

**Git goes through the broker, over git's smart HTTP protocol, on the job's socket.** The harness's remote is `http://127.0.0.1:8089/git/{owner}/{repo}.git`, which the forwarder relays to the job's socket like a model call ([ADR-0038](0038-model-calls-from-the-sandbox-and-the-provider-key-file.md)). The image already keeps `127.0.0.1` out of the proxy ([ADR-0040](0040-the-sandbox-image.md)). A target starting with `/git/` is for the git proxy (`hyalos_broker::git`).

It serves exactly the four requests of the protocol:
- `GET info/refs?service=git-upload-pack`
- `POST git-upload-pack` (clone and fetch)
- `GET info/refs?service=git-receive-pack`
- `POST git-receive-pack` (push)

Anything else under `/git/` is a 404 (`git.route_refused`). The owner and name are letters, digits, `-`, `_` and `.`, but not `.` or `..` alone; a `.git` suffix is optional.

**The broker allows a request only if all of these hold, in this order:**
1. **The job is live and within its deadline** (`admission.ended`, `admission.expired`), as for every request on the socket.
2. **The repository is the manifest's.** It is compared ignoring ASCII case, as GitHub does (`git.repo_not_allowed`).
3. **The actions allow the service.** Fetching needs `read` and pushing needs `push` (`git.action_not_allowed`). These are the actions whose tokens carry `contents: read` and `contents: write` (ADR-0044).
4. **The job has a forge token** (`git.no_token`).
5. **For a push, every ref update is allowed.** Before anything is sent, the broker reads the push's commands: the pkt-lines before the first flush, with any `shallow` lines skipped and capabilities after the first NUL ignored. Each command's ref must:
   - be a branch (`refs/heads/..`);
   - be well formed by `git check-ref-format`'s rules, kept to printable ASCII;
   - lie inside the manifest's branch prefix (`git.ref_not_allowed`).

   A prefix ending in `/` covers the branches under it. Any other prefix covers itself and the branches under it, so `hyalos/3` does not cover `hyalos/30`. An empty prefix covers nothing. A command that deletes a branch is refused even inside the prefix (`git.delete_refused`), since a prefix may be shared by jobs. One refused command refuses the whole push. A signed push (`push-cert`) is refused (`git.push_cert`), and commands that do not parse are a 400 (`git.bad_request`).

Then the broker sends the request to the forge's git host. It uses the manifest's spelling of the repository and adds `authorization: Basic x-access-token:<token>`, as GitHub expects for an installation token. Only `accept`, `content-type`, `git-protocol` and `user-agent` are passed on, plus `content-encoding` for a fetch, whose request git may gzip. A push's pack is never compressed again, so `content-encoding` on a push is refused.

The forge's answer is relayed as it streams, and is cut off when the job ends. Its 401 and 403 become the broker's 403 (`git.upstream_refused`), and its 404 becomes `git.upstream_not_found`. Its challenge is never passed on: the harness holds no credential to answer with.

**Bodies.** Git sends a body larger than its post buffer (1 MiB) chunked, and may first ask for `100 Continue`. The broker reads both. A request with both `content-length` and `transfer-encoding`, or with any transfer coding but `chunked`, is a 400. The broker reads at most 64 MiB (`MAX_GIT_BODY`, `git.too_large`) and holds the whole of a push before sending it. That is what lets it refuse a push before the forge sees any of it.

**The git host is github.com** (`Git::github`), over TLS to the compiled-in roots ([ADR-0039](0039-tls-to-model-providers.md)). A test can name another host from code (`Git::with_base`, plain HTTP only to loopback); nothing in configuration does. GitHub Enterprise Server's git host is not built.

**A refusal is audited as the manifest gate's `deny`.** Refusals 2 and 3, and each ref that step 5 refuses as outside the prefix or as a deletion, are 403s to the harness. They are also `ToEngine::Gate { gate: manifest, answer: deny, reason, inputs }` to the engine, the message and gate that ADR-0035 and [ADR-0009](0009-gates-answer-allow-deny-unknown.md) already define. As for egress ([ADR-0042](0042-the-egress-gate-and-the-allowlisting-proxy.md)), `inputs` is a SHA-256, never the name:
- for a repository, of `owner/name` in lowercase;
- for a ref, of the ref name;
- for an action, of the service name.

Each refused ref of a push is reported. Each distinct report goes once per job, at most 32, and then one more with `git.too_many_refusals`. This cap is separate from the egress gate's. Refusals 1 and 4 are not reported: the first is the job's end, as for egress, and the second is the engine's own omission. Nor is a push that is signed, does not parse, or carries `content-encoding`: it is refused whole and names no ref to digest. A request the gate allowed is not reported.

**The tests use the real git on both sides.** `core/broker/tests/git.rs` runs the `git` client through the broker's job socket to `git http-backend`, which serves bare repositories over loopback and answers only the job's token. The tests check that:
- a job clones and pushes a branch inside its prefix;
- a push to `main`, a tag, a branch with a look-alike prefix, a deletion, and an allowed ref pushed together with a refused one are each refused, reported and never reach the server;
- a 3 MiB push arrives chunked and is checked the same way;
- another repository is refused and reported, while the same repository in another case is allowed;
- a job without `push` can clone but not push;
- a job without a token reaches nothing;
- every request the server saw carried the token.

CI sets `HYALOS_REQUIRE_GIT`, so a missing git fails rather than skips.

## Consequences

A job's writes to the forge are now exactly the branches inside its prefix in its repository, whatever its token would allow. Every write it tries outside them is in the log as the manifest gate's `deny`. The sandbox still holds no credential: git works with no credential helper, and a challenge never reaches it.

The broker holds up to 64 MiB per push in memory, with at most 16 connections per job. A larger push is refused. Streaming a pack after its commands are checked would lift the limit, and is left for when it is needed.

Not here:
- **Git in the image.** The sandbox image has no `git`, and its test does not run one; that is a follow-up to [ADR-0040](0040-the-sandbox-image.md).
- **Who sets the remote.** The end-to-end job (#70) does that.
- **Force pushes inside the prefix.** The broker cannot tell a fast-forward from a rewrite without the objects, and the forge accepts either on a branch it does not protect.
- **Taint.** A tainted manifest is already held to its prefix, and so is every other.
- **Merging and the forge's API.** Those are S3.12 (#79).
- **GitLab.**
