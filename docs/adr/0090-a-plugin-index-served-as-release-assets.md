# ADR-0090: A plugin index served as release assets

**Status:** Proposed

## Context

[ADR-0007](0007-capability-based-marketplace-trust.md) says how a release is trusted: the pipeline signs it keylessly, records it in a transparency log, and an install verifies a Sigstore bundle against a pinned identity, online or offline. It does not say where an install finds a plugin by name. [ADR-0089](0089-installing-an-agent-skill-and-running-it-in-the-job-s-sandbox.md) installs a skill from a directory only.

S7.9 (#81) asks for an ADR choosing the source (an index, an OCI registry or GitHub releases) before the code, for install to resolve a plugin by name there and verify its digest, and for the offline bundle (S7.6) to use the same format. The choices:

- **An OCI registry.** Skills would be artifacts by digest, and registries are everywhere. But an install would need an OCI client in the trusted core (manifests, media types, token auth), and the offline bundle would be an OCI layout, a second format for an air gap to carry.
- **GitHub releases, searched through the API.** No new format, but resolving a name would mean API calls, rate limits and GitHub's own release semantics, and an offline bundle would have nothing to mirror.
- **A signed index, served as files.** One JSON file maps each name to its versions, each with a digest and the files beside it. A release's assets serve it; a directory holding the same files is the offline bundle. An install needs only an HTTPS GET.

## Decision

**The source is an index.** `index.json` and the files it names sit side by side at one location: the assets of a GitHub release (served at `https://github.com/<owner>/<repo>/releases/download/<tag>/` or `.../releases/latest/download/`), any HTTPS location laid out the same way, or a directory. The index is:

```json
{"version": 1,
 "plugins": {"pdf-tools": [
   {"version": "1.1.0", "digest": "<sha-256, hex>",
    "artifact": "pdf-tools-1.1.0.skill", "bundle": "pdf-tools-1.1.0.sigstore.json"}]}}
```

Each name is a `PluginName`. Its versions are listed oldest first, and the last is the latest. A version is 1 to 64 letters, digits, `.`, `+` and `-`, compared only for equality. File names are plain names beside the index, never paths, and none is named twice or is `index.json`. Unknown fields are refused, the index is at most 1 MiB, and a version of the format other than 1 is refused by name (`hyalos_plugin::index`).

**The artifact is the skill's archive, and its SHA-256 is the skill's digest.** The archive is the bytes ADR-0089's digest was already taken over: the domain tag `hyalos.skill.v1\n`, then each file in path order as its path's length, the path, whether it is executable, its length and its bytes. So an install checks the digest with one hash of the file it fetched, before reading anything in it, and the digest an index gives, the one an install records and the one the store keeps a skill under are the same. Reading an archive applies a directory's rules: plain names, at most 256 files, 8 MiB and depth 8, paths in order and each once, and no file that is also a directory of another. `hyalos plugin pack <dir> <file>` writes one and prints its digest.

**Install resolves by name.** `hyalos plugin install --index <location> <name> [--version <v>]` fetches `index.json`, resolves the name to the version asked for or the latest, fetches the archive, and refuses it unless its SHA-256 is the index's digest (`plugin.digest_mismatch`) and the skill in it has that name (`plugin.wrong_name`). It then installs as ADR-0089 does: the capability diff, `--grant` for anything added, the operator's signed `plugin.installed`. The source it records is `{"index": {"location", "version"}}`, so an update check (S7.10) knows where to look. A location is an `https://` URL, an `http://` URL on loopback (for tests), or an absolute directory; a relative one given on the command line is made absolute. Over HTTPS, at most five redirects are followed, each only to HTTPS or loopback, and every file is read within its limit.

**The offline bundle is the same files in a directory.** An index location that is a directory reads `index.json` and the files it names from there, never following a link. S7.6 adds its revocation list and trusted-root snapshot to that directory, and the pipeline's signature over them, without changing the index.

**The bundle is named now and verified later.** Each entry names its Sigstore bundle, so an index written today already carries what ADR-0007's check needs. Verifying it is S7.12 (#106). Until then an install from an index is trusted exactly as a local one is: the digest proves the files are the ones the index named, not who published them, so every capability is still the operator's grant. When S7.12 lands, install verifies the bundle before the archive, and refuses without one.

This is schema version 16 (`plugin.installed` gains the index source; older payloads read as they were) and `StateImage` version 11 (the same source in `plugins`).

## Consequences

- An install needs only HTTPS GETs and a hash, with no registry client or forge API in the trusted core.
- A GitHub release is the simplest host, and a mirror is a copy of the files. Pinning a tag in the location pins the index; `latest` follows new releases.
- The digest an index gives is the one an install records and the store checks, so a file changed anywhere on the way is refused.
- Until S7.12, an index is not authenticated: someone who controls the location can offer other files with matching digests. That gains them nothing a local install would not, since the operator reads the diff and grants every capability. Install policy (S7.5) must not install automatically from an index on its own authority before then.
- A second kind of artifact (a WASM component, S6.1) needs its own archive or a field saying which kind an entry is; that is a version 2 of the index.
