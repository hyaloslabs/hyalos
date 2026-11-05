# ADR-0049: Git in the sandbox image

**Status:** Proposed

## Context

The broker's git proxy ([ADR-0046](0046-the-git-proxy.md)) serves git's smart HTTP on a job's socket under `/git/{owner}/{repo}.git`. Inside the sandbox, `hyalos-forward` relays `127.0.0.1:8089` to that socket.

The sandbox image ([ADR-0040](0040-the-sandbox-image.md)) has no `git`, so nothing in a sandbox can clone or push. The git proxy's own test runs the host's git, not the image's. This is S3.3b (#182), left from S3.3 (#18).

Three things could go wrong with a git in the image:
- **It could look for a credential.** A helper or a prompt would hang the job or write a credential somewhere.
- **It could send its requests to the proxy.** The image sets `HTTPS_PROXY` and `HTTP_PROXY` to the forwarder for the harnesses' web traffic. A clone sent there as a proxy request names `127.0.0.1`, which the egress gate refuses.
- **It could reach the network another way.** ssh, `git://`, `ext::` and `file://` are the obvious routes.

## Decision

**The image installs Debian's `git` (2.39.5 at the pinned date) with `--no-install-recommends`,** so there is no ssh client and no `less`.
- It installs from one snapshot of the three suites the base image uses: `bookworm`, `bookworm-updates` and `bookworm-security`, at the date already pinned for the test target's curl. A snapshot of `bookworm` alone does not install, because the base image's `perl-base` comes from the security suite.
- Every build installs the same versions, and they match what the base image already has.
- The `test` target reuses these sources for curl.

**`/etc/gitconfig` is shipped from `images/sandbox/etc/git/gitconfig`:**
- **No credential.** `credential.helper` is empty, which clears every helper, and `core.askPass` is empty. The image also sets `GIT_TERMINAL_PROMPT=0` and an empty `GIT_ASKPASS`, so git fails rather than asks. The broker adds the job's token itself (ADR-0046), so git never holds one.
- **No proxy for the forwarder.** `http."http://127.0.0.1:8089/".proxy` is empty. `NO_PROXY` already says the same to curl, and this setting does not depend on the environment a harness passes on.
- **Smart HTTP only.** `protocol.allow` is `never`, with only `http` and `https` allowed. ssh, `git://`, `ext::`, which runs a command, and `file://` are refused by git itself. An `https` remote would still have to pass the egress gate.
- **A commit identity.** `user.name` is `Hyalos` and `user.email` is an address under `.invalid`, so a job can commit without configuring one. The forge records who pushed from the token.
- `init.defaultBranch` is `main`, and the detached-head advice is off.

**Nothing in the image sets a remote or rewrites `github.com` URLs.** A job reaches its repository at `http://127.0.0.1:8089/git/{owner}/{repo}.git`. Giving it that URL, and the clone a job starts from, is the end-to-end job's part (#70).

## Consequences

The image's git clones and pushes through the broker under the substrate's isolation. `crates/sandbox-tools/tests/git.rs` runs it with the container substrate's flags against the real broker and a `git http-backend` on the host's loopback, which accepts only the job's token. The test checks:
- the clone and a push to `hyalos/fix` succeed, and that branch holds the sandbox's commit;
- a push to `main` is refused before anything reaches the server, `main` is unchanged, and the refusal is reported once as the manifest gate's `deny` (`git.ref_not_allowed`);
- the DNS log shows no lookup;
- every request on the channel is a git call in origin form.

A control removes the proxy setting and empties `NO_PROXY`. The same clone then goes out as a proxy request for `127.0.0.1`, so the check would see a git that went to the proxy. The `sandbox image` workflow runs this test too; it needs the host's `git` for the server.

The shipped image is larger: the git layer, with perl and git's libraries, is about 92 MB.

A repository's own hooks are not disabled. Any code that a cloned repository can make git run is the job's code, inside the same sandbox as everything else the job runs.

git over `https` to a host other than the forge, through the egress gate, would need CA certificates, which the image does not have. Nothing needs it yet.
