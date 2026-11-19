# Security policy

Hyalos runs coding agents in sandboxes that hold no secrets. If you find a way around that, we want to hear about it.

## Reporting a vulnerability

Please do not open a public issue, discussion or pull request for a vulnerability.

Report it privately through GitHub instead: on this repository's **Security** tab, choose **Report a vulnerability**. Only the maintainers can see the report.

Please include:
- what an attacker can do, and from where: a steered agent in the sandbox, a skill or plugin, forge content, the driving agent, the network, or the host;
- the version or commit;
- steps or a test that reproduces it.

Never include a real secret. If a report needs one, use a key made for the purpose.

We aim to acknowledge a report within three working days, and to agree a fix and disclosure date with you. Once a fix is released, we credit reporters who want credit.

## Supported versions

Hyalos is before 1.0. Only the latest release gets security fixes.

## What is in scope

The [threat model](docs/security/threat-model.md) sets out what Hyalos defends, against whom, and its known gaps. Every control in it names its code and the test that proves it. [Data flows](docs/security/data-flows.md) shows where each secret lives.

These are especially in scope:
- any secret reaching the sandbox, a log line or an error message;
- the sandbox reaching the network, or the host, other than through the broker;
- forge or model text reaching an actor's prompt or the engine;
- a decision made without the operator's signature;
- a log change that verification does not catch;
- a skill, plugin or release that gets more than it was granted, or installs unverified.

These are out of scope: an attacker running as the operator's own OS user, or as root on the host; known gaps listed in the threat model (but a way to make one worse is in scope); and findings in `spikes/`, which is not product code.
