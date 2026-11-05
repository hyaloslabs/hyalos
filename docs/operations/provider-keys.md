# Provider keys: setting up the key file

How to give the broker the API keys it calls models with. The reasons are in [ADR-0002](../adr/0002-one-binary-two-processes-sandboxes.md) and [ADR-0038](../adr/0038-model-calls-from-the-sandbox-and-the-provider-key-file.md). This page is the procedure.

The broker reads `provider.keys` from the config directory (`$HYALOS_CONFIG_DIR`, else `$XDG_CONFIG_HOME/hyalos`, else `~/.config/hyalos`). No sandbox, and not the engine, ever sees the keys.

## Before you start

- Make a **dedicated** API key for Hyalos at each provider you use, not one you use anywhere else.
- Set a **spend cap** on that key at the provider. Each job's budget is enforced by Hyalos, but a provider key cannot be scoped to one job, so the cap is what bounds a loss. `hyalos doctor` reminds you.

## The file

```
# One name = value per line. Lines starting with # are ignored.
anthropic.key = sk-ant-...
openai.key = sk-...

# Optional: where to reach the provider. Leave it out for the provider's public API.
# An https:// base can be any host; its certificate must chain to the Mozilla roots built into
# Hyalos, and there is no setting to skip that check. A plain http:// base must be on this machine,
# such as a local OpenAI-compatible model:
# openai.base = http://127.0.0.1:11434
```

A job can never reach a base's host, or any name under it, through the egress proxy, even if its manifest lists it ([ADR-0051](../adr/0051-provider-hosts-the-egress-gate-never-reaches.md)). The same holds for `api.anthropic.com` and `api.openai.com`. Model calls reach the provider only through the broker, with the broker's key.

Then make it yours alone:

```
chmod 600 ~/.config/hyalos/provider.keys
```

The broker refuses to start if the file belongs to another user, can be read by group or others, or has a line it does not understand. Its error names the line number, never the line's contents. If the file is missing, the broker still starts, and model calls are refused with a message that says there is no key.

## Changing or revoking a key

Revoke the old key at the provider first, then edit the file. The broker reads the file when it starts, so restart Hyalos to pick up the change.
