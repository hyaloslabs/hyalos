# Hyalos

*HY-a-los*: Greek for glass, a clear stone. Work is contained, and everything inside can be seen.

**One tool an agent can use without getting you into trouble.** Any agent (Claude Code, Hermes, an Archon workflow) hands work to Hyalos; Hyalos runs it contained, within a signed manifest, with every action audited, and runs any Agent Skill or MCP server safely.

> **Status:** pre-1.0, one maintainer, and **not independently security-reviewed** ([why that matters](#security)). It runs fix-issue jobs end to end with Claude Code or Codex, in a container, gVisor or a Firecracker microVM. This is a clean-room rewrite of hivrr; it carries over lessons, not modules.

## See it work

A two-minute demo shows two attacks contained: a poisoned skill refused, and a job steered by an outsider's issue held. It is a test, so CI checks on every change that it still does what it shows. It needs no secret, no network and no container:

```
cargo test -p hyalos --test demo --locked -- --nocapture --test-threads=1
```

[`docs/operations/demo.md`](docs/operations/demo.md) has the storyboard and how to record it.

## Start here

- [Getting started](docs/operations/getting-started.md): from a clone to a first job; or open Claude Code here and say "set up Hyalos"
- [Product requirements](docs/prd/hyalos.md): what we are building and why
- [Architecture decisions](docs/adr/README.md): why it is shaped this way
- [Documentation map](docs/README.md): where everything lives
- [Threat model](docs/security/threat-model.md) and [security policy](SECURITY.md): what Hyalos defends, and how to report a vulnerability

## Security

Hyalos has not had an independent security review. Its controls are designed and tested, not audited: the [threat model](docs/security/threat-model.md) names each control's code and test and lists the known gaps, and [`SECURITY.md`](SECURITY.md) says what has been checked and how to report a vulnerability privately. If you can review part of the trusted core, [reviewing the core](docs/security/reviewing-the-core.md) says where to start.

## License

MIT. See [`LICENSE`](LICENSE). Contributions are signed off under the DCO. See [`CONTRIBUTING.md`](CONTRIBUTING.md).
