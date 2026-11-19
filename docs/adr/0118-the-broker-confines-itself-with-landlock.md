# ADR-0118: The broker confines itself with Landlock

**Status:** Accepted

## Context

The broker is a separate process with a cleared environment and a closed, typed wire (ADR-0035). The engine's keys never enter its memory: `engine.key` and `github-app.pem` stay in the engine, and `operator.key` stays sealed under the passphrase. But the broker runs as the operator's own OS user, and all three are files that user can read. A broker compromised by code execution could read `engine.key` and `github-app.pem` from disk, and with them forge the log and mint GitHub tokens. That compromise could come through an escape from the Wasmtime plugin host (ADR-0087) or a bug in one of its parsers. The threat model recorded this as Known gap 1, and the pre-read of the broker raised it again (#270, #337).

All of those files are in the config directory. Of that directory, the broker needs only `provider.keys`, and only once, at startup (ADR-0038). After startup it reads only three kinds of file:
- the system's own files, for name resolution and the C library;
- the plugin store, for a deterministic step's plugin (ADR-0102);
- the job sockets it binds in the runtime directory.

It runs no subprocess. Its TLS roots are compiled in.

The options were:
- **A separate uid.** This needs root or a setuid helper to set up, and the operator's machine has neither.
- **A mount namespace.** This needs unprivileged user namespaces, which some distributions turn off.
- **Landlock.** An unprivileged process can restrict itself with it, on Linux 5.13 and later.

## Decision

The broker confines itself with Landlock once it has done everything it does at startup: read `provider.keys`, connect to the engine and make the plugin runtime. From then on it may only:
- read `/etc`, `/usr`, `/lib`, `/lib32`, `/lib64`, `/proc`, `/sys`, `/run` and `/dev`;
- read the plugin store the engine named with `--read`;
- make and remove sockets in the runtime directory the engine named with `--sockets`. It may read nothing there.

Every other filesystem right that Landlock ABI 3 knows is refused, executing a file among them. `restrict_self` also sets `no_new_privs`.

- **The engine checks the grants before starting the broker.** The engine starts the broker with the runtime directory and the plugin store as arguments (`BrokerProcess::launch_confined`). It refuses to start a broker whose read grants contain the config directory (`LaunchError::Confinement`). The config directory is the directory of the provider key file, and the one the engine itself would use.
- **Landlock refusing the rules stops the broker.** A kernel without Landlock lets the broker run unconfined, and the broker says so on standard error. A Landlock older than ABI 3 restricts reads and writes still: the first version restricts both.
- **Network access is not restricted.** The broker's network access is what the gates decide (ADR-0042).
- **The test.** `core/hyalos/tests/broker_confined.rs` makes the real broker try to read stand-in key files. In the granted store it reads them. In the config directory it cannot open them. CI requires Landlock (`HYALOS_REQUIRE_LANDLOCK`), so the test cannot be skipped there.

## Consequences

- **A compromised broker cannot read the engine's keys** on a kernel with Landlock. It can still read the system's files, the plugin store and `provider.keys`' contents in its own memory. The provider keys are the broker's own secret anyway.
- **A kernel without Landlock, or a system other than Linux, gives no confinement.** The gap is narrowed to that case, and the broker reports it. Refusing to run there would rule out kernels that have Landlock built but not enabled.
- **A job socket must be in the runtime directory.** The engine already makes every job's directory there (`JobDir`). Tests that put sockets elsewhere were moved.
- **The plugin store is fixed when the broker starts.** A `RunPlugin` naming another store is refused as unreadable.
- **A new path the broker needs after startup means a grant here.** That grant is a change to this decision.
- **New dependencies.** The `landlock` crate, with `enumflags2`, is a dependency of the `hyalos` crate on Linux. The workspace's rule against unsafe code still holds: the crate wraps the system calls.
