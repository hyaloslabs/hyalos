# The engine's key: setup, backup, recovery and what to record

How to look after the key that signs the event log. The reasons are in [ADR-0022](../adr/0022-engine-keys-generation-storage-backup-and-loss.md), the log format in [ADR-0018](../adr/0018-event-log-format-and-verification.md) and backups in [ADR-0021](../adr/0021-backup-and-restore-of-the-event-log.md). This page is the procedure.

Two files matter, both in the config directory (`$HYALOS_CONFIG_DIR`, else `$XDG_CONFIG_HOME/hyalos`, else `~/.config/hyalos`):

| File | What | Mode | Keep a copy? |
| --- | --- | --- | --- |
| `engine.key` | The secret that signs every event. One line, `hyalos-engine-key-v1:` and 64 hex characters. | 0600, owned by the user running the engine | **Yes, somewhere safe.** |
| `engine.pub` | The public half. `hyalos log verify` reads it. | 0444 | Yes. It is not secret. |

## First start

```
hyalos init
```

If there is no key and no log, this creates the directories (mode 0700), the key and `engine.pub`, and prints the public key. It is safe to run again: it checks what is there and changes nothing.

**Then back up `engine.key` straight away**, to somewhere a compromise of this machine cannot reach, such as a password manager or offline media. Without it your log can be verified but never extended. Also keep a copy of the public key.

## Record the log's head

An attacker with the log file cannot forge events without the key, but they can cut events off the end, and what is left is still a valid chain. Only a head you recorded elsewhere exposes that.

After anything important, run `hyalos log verify`. It prints a line like `ok: 412 events, head 412:9f2c…`. Write that `seq:hash` somewhere other than this machine. Later, check against it:

```
hyalos log verify --expect-head 412:9f2c…
```

This fails if the log ends anywhere else. `hyalos log backup` and `hyalos log restore` take `--expect-head` too.

## Back up the log

```
hyalos log backup /safe/place/backup.db
```

This is safe while the engine runs, checks the copy before it exists under that name, and never overwrites a file. It does **not** include the key. Back that up separately, as above.

## Move to another machine

1. On the new machine, restore the log: `hyalos log restore backup.db --public-key <hex> --expect-head <seq:hash>`. Pass the public key yourself, since `engine.pub` is not there yet.
2. Put `engine.key` in the new config directory (create it with mode 0700) and give the file mode 0600.
3. Run `hyalos init`. It writes `engine.pub` from the key, checks the key against the log, and refuses if they do not match.
4. Check: `hyalos log verify --expect-head <seq:hash>`.

The engine can then carry on adding to the log. If you skip step 2, `hyalos init` refuses and tells you why: a restored log with no key can be verified, not extended.

## If the key is lost

The log still verifies, from the public key you kept. The engine will not continue it and will not make a new key beside it. To start over:

1. Move `events.db` and `engine.pub` aside together, and keep them. They are how the old log is verified.
2. Run `hyalos init`.

You get a new key and a new, empty log. **Tasks, jobs and holds do not carry across**, and the old log and the new one are not linked. Key rotation is not supported yet (ADR-0022).

## If the key may have leaked

Whoever holds the key can forge history from any point in time, and the log cannot show which events are forged. Treat events from the earliest time the key could have been exposed as untrusted, archive the log as above, and start a new one. If another user could read the file, `hyalos init` says so and refuses to continue until you fix its mode.

## If the engine says the key or directory is owned by another user

The engine only uses `engine.key` and the config directory when they belong to the user it runs as, whatever their mode (ADR-0030). The message gives both user ids. If the files are yours (for example, you made them as root and now run as yourself), run `chown` as the message says. If you did not put them there, treat the key as compromised: see above.

## Core dumps

A crash dump could contain the key. The engine turns dumps off for itself when it starts (ADR-0030): it sets the core file size limit to zero and, on Linux, marks the process not dumpable, so neither a core file nor a crash handler such as systemd-coredump receives its memory. **You no longer need `LimitCORE=0` or `ulimit -c 0`**; they do no harm. If the engine cannot do this it stops with an error and does not run.

One thing follows from it: while running, the process's `/proc/<pid>` files belong to root, so a debugger or monitoring tool running as your user cannot read its memory or environment. Diagnose with the log (`hyalos log verify`) and the engine's own output.

## What this does not cover

- The key sits in a file, readable by programs running as you. The sandboxes and the broker cannot read it. Keyring and hardware storage are not used yet (ADR-0022).
- The manifest signing key is not kept: it is made fresh each time the engine starts.
- Your approval key, `operator.key`, is a different key, sealed under a passphrase only you know, and it signs every decision recorded as yours. The engine uses it only inside a command you run and type the passphrase into. See [approvals.md](approvals.md).
