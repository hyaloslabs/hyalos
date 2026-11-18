# ADR-0109: Property-based contract suites at every boundary

**Status:** Accepted

## Context

S8.3 (#52). [ADR-0013](0013-deterministic-simulation-and-evidence-gated-releases.md) decides that "each boundary (broker enforcement, substrate drivers, plugin host) has a property-based contract suite", because the simulation fakes those boundaries and cannot prove the real ones. What existed was example-based. The substrate contract ([ADR-0032](0032-the-substrate-interface-and-its-contract.md)) checks every driver on cases written out by hand. The broker's admission and egress tests, and the plugin host's tests, check the cases their authors thought of. A property says what must hold for every input. It is only as good as the inputs it is checked on, so those inputs have to be drawn, not chosen.

## Decision

**A property is a function of a seeded stream, run on a range of seeds.** `crates/props` (`hyalos-props`) holds the runner and the generators the suites share. `forall(label, seeds, property)` runs the property once per seed, on a stream forked from the seed under `label`. The first failing case stops the run and names its seed and the command that replays it: `HYALOS_PROP_SEED=<seed>`. `HYALOS_PROP_CASES` runs more cases than a property's default. Nothing shrinks a failing input: the seed is the reproduction, and each property's message says what it drew. The stream is `hyalos_types::SeededRng`, the simulation's own, so no dependency is added and a case is the same on every machine.

**Each boundary has its suite, beside its other contract:**

| Boundary | Suite | Properties | Cases |
| --- | --- | --- | --- |
| Substrate drivers | `hyalos-substrate-contract`, the generated half of behaviour | A spec's arguments, environment and exit code reach the sandbox exactly: every argument verbatim and unread by any shell, every variable with its value, the code back. Values and arguments are drawn from quotes, expansions, separators, whitespace and non-ASCII. A spec with one field made bad, in a drawn way and at a drawn place, is refused as a spec and leaves nothing behind. | 12 round trips, 200 bad specs |
| Broker enforcement | `hyalos-broker-contract` | **Admission:** a manifest is admitted exactly when nothing is wrong with it. Each way it can be wrong is drawn independently (another key, an altered byte, another job or role, a job not live, a clock past the deadline). The refusal is the first check's code, in the documented order, and never repeats the document. **Egress**, through a running broker and a job's socket: a proxy request reaches its host exactly when the manifest lists the host, in any spelling a resolver reads the same, on its one port, and the host is not a provider's. Otherwise it gets 403 and one `deny` report with the code that says why, of `host:port` when the gate read one, and it connects to nothing. | 1,000 admissions, 200 requests |
| Plugin host | `hyalos-plugin-contract` | A component instantiates exactly when the grant covers every capability interface it imports. A call reaches the broker exactly when its name is in the grant's field, compared exactly, near misses included. The broker is asked at most `CALLS` times per run. | 150, 300 and 12 |

**A suite must reach every outcome it is for.** A property that passes because the generator never drew a refusal checks nothing. So each suite also requires its cases to have reached every outcome: admitted and each of the five refusals; each of the five egress refusals and a pass on each port; linked and refused; reached and refused.

**They run in CI on every pull request,** in `test (the rest of the workspace)`. That job sets `HYALOS_REQUIRE_CONTAINER`, `_GVISOR` and `_MICROVM`, so the substrate properties run on the container, gVisor and Firecracker drivers as well as the local fake, and a driver that is missing fails rather than skips. The broker and plugin suites need no tool. The new crates are under `crates/`, so none of them is in the binary.

## Consequences

A boundary's contract is now held on inputs nobody chose. A failure replays from one number. What a suite finds on a driver is that driver's bug, with a seed.

The cases are few where each costs a sandbox, and many where each is cheap. A property is checked on as many inputs as CI time allows, which is not all of them. Raising a default, or running with `HYALOS_PROP_CASES`, searches further.

The generators are written per boundary and encode what each boundary promises: which spellings of a host are the same host, which ports belong to which method. A promise the generator does not know about is not checked. The example-based tests stay beside the suites for the cases worth naming.
