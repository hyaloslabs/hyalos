# ADR-008: Findings: capsules should adopt MicroPython, and Monty is not ready

**Status:** proposed

**Proposed by:** executor, Slice 5 (#498), from the evidence of Slices 1 to 5

## Context

#438 asks whether a hyalOS task can be a signed Python script, run by an interpreter compiled into
the guest, on hyalOS's four calls, with a small SDK generated from its contract, and which runtime
capsules should adopt. Slices 1 to 4 built the monitor's contract check, the SDK generator and both
runtimes as guests; Slice 5 ran each on hostile inputs and measured them. The evidence is committed
under `spikes/438/`: the records and `evidence/expected.tsv`, which
`python_committed_evidence_replays_on_both_backends` replays, `MEASUREMENTS.md`, and the recorded
results in `README.md` (its `profile:`, `process-only:` and `typecheck:` lines). Every claim below
cites one of them or a slice's progress file.

## Options

1. **Adopt MicroPython.** It builds for `wasm32-unknown-unknown` with no floats and runs under
   `hyalos.spike/1`, which is `hyalos.profile/2`'s settings and limits with the contract enforced.
2. **Adopt Monty.** It runs the same script and SDK to the same records, and its type checker can
   refuse an ungranted call before signing, but it needs floats, more memory and many times the
   fuel.
3. **Adopt neither yet.** Neither runtime has run on KVM in this spike.

## Decision

**Capsules should adopt MicroPython. Monty is not ready.** Option 3 is rejected because the KVM
blocker that stops MicroPython belongs to the kernel image, not to MicroPython: the same fix that
lets any long guest loop run would let it run (below).

### What both runtimes showed

- **The same task gives the same answers, effects and results.** The triage script and its generated SDK run unchanged
  under both. At seeds 0 and 7, and under the no-notification contract, Monty's records hold the
  same answers, effects and results as MicroPython's (`evidence/micropython-triage-0.json` and
  `monty-triage-0.json`, and their `-7` and `-no-notify` pairs; progress `slice-04.md`,
  Discoveries), and both give the digest guest's notification and result
  (`python_3_3_micropython_triage_matches_the_digest_guest`). Every hostile case that ends with a
  result under both runtimes has the same notification and result under each (`MEASUREMENTS.md`,
  the hostile table: `utf8`, `record-16384`, `record-16385`, `numbers`, `repr`).
- **The monitor, not the SDK, is the boundary (pre-mortem P3).** A script that skips the SDK and
  calls `_hyalos.effect` under a contract with no effect is refused `UnknownEffect` by the monitor
  under both runtimes, and no record is written (`expected.tsv`: `micropython-raw-effect`,
  `monty-raw-effect`). A script that calls `task.notify` under that contract fails with
  `AttributeError` inside the guest only because the SDK leaves the name out
  (`micropython-no-notify`, `monty-no-notify`); the SDK makes the allowed calls easy, and a
  hand-edited SDK or a raw call gains nothing the contract does not grant. Capsules must keep the
  monitor's check whatever the SDK or a type checker does.
- **The contract changes the task.** A different contract gives different SDK bytes and a
  different task identity (S-02.4, Slice 2).

### MicroPython

- **It fits the default limits.** The guest is 249,874 bytes against `GUEST_MAX`'s 1 MiB, runs in
  `hyalos.spike/1`'s 4 MiB, and needs no floats (`MEASUREMENTS.md`; `README.md`, `process-only:
  micropython`). On the 40 issues it uses 33,127,745 fuel, and on 200 issues 81,687,923, both well
  within profile 2's 200,000,000 (`MEASUREMENTS.md`: the issue fixture row, and `distinct`).
- **Its blocker on KVM is the kernel image's (issue #503).** The default kernel image runs off its
  2 MiB stack once a guest loop runs about 10,000 iterations, because its `wasmi` dispatch does not
  become tail calls in the kernel build, and MicroPython's start-up alone runs past that
  (`README.md`, `process-only: micropython`). Built with `portable-dispatch` and nothing else
  changed, the image ran the MicroPython triage case on KVM to the process backend's result in
  about 280 ms (`slice-03.md`, Discoveries). That run was local and is not a committed record, so
  MicroPython's byte-identical replay on KVM is not shown by this spike's evidence.
- **Its heap is never collected.** The port's 2 MiB heap is not garbage collected, because a
  conservative scan of WebAssembly's shadow stack would miss live pointers (`slice-03.md`,
  Decisions). 200 titles of 4,096 bytes fill it after 93 questions: the run ends with the result
  `error MemoryError` and sends no notification (`MEASUREMENTS.md`, `heap`). A task's memory is
  therefore a bound on what its inputs may hold, not only on its working set.
- **Its build needs Asyncify** for MicroPython's `setjmp` and `longjmp`, run in a pinned container
  (`slice-03.md`, Deviations). The two clean builds compared equal in one environment; a rebuild
  elsewhere that differs is a reproducibility finding (pre-mortem P1) that this spike did not test.

### Monty

- **It does not fit KVM.** The guest is 6,945,056 bytes against `GUEST_MAX`'s 1 MiB, refused
  `TooLarge` before boot, and even a layout that held it would meet #503 (`README.md`,
  `process-only: monty`).
- **It needs a new profile.** It needs floats, with `wasmi`'s deterministic NaN, and 5 MiB of
  memory: `hyalos.spike/2` (`README.md`, `monty profile:`; ADR-0137; 4.3 of `slice-04.md`).
- **It cannot triage 200 issues within that profile.** The 40 issues take 413,707,242 fuel, 12.5
  times MicroPython's, and 200 short titles run out of the profile's 830,000,000 (`MEASUREMENTS.md`:
  the issue fixture row; `distinct` and `collide` under Monty end `OutOfFuel`). Fuel and time are
  not in proportion: its run time is about five times MicroPython's (`MEASUREMENTS.md`).
- **It lacks builtins a CPython author uses.** It has no `bytearray` and no `globals()`, so its
  guest defines a `bytearray` stand-in in module `hyalos` (`slice-04.md`, Deviations and
  Discoveries).
- **Its type checker works on a spike task.** Given the no-notification stub, it reports exactly
  the ungranted `notify` call and nothing else (`README.md`, `typecheck: rejected`). That is the one
  thing Monty offers that MicroPython does not: refusing an ungranted call before the task is
  signed. It does not replace the monitor's check.

What Monty still needs before capsules could adopt it: a guest within the KVM machine's size, or a
larger machine; fuel for 200 issues within a profile capsules accepts; floats and 5 MiB in an
accepted profile; and `bytearray`.

### The hostile-input limit an audit report must carry (pre-mortem P5)

A runtime cannot reach anything outside the guest: no hostile case crashed the process backend,
the only one they ran on since both runtimes are process-only (KVM was not reached), no record holds an ungranted schema or effect, and every case ran to a result or a refusal
(`spikes/438/hostile.sh` stops otherwise; `MEASUREMENTS.md`, the hostile table). Inside its grants,
hostile data does steer the task:

- a title is copied into the notification byte for byte when it is labelled security, including
  bytes that are not UTF-8 and a carriage return that draws a forged line on a terminal (`utf8`,
  `repr`);
- inputs that exhaust a runtime's memory or fuel change the result or suppress the notification
  (`heap` under MicroPython ends `error MemoryError` with no notification; `heap`, `distinct` and
  `collide` under Monty end `OutOfFuel`);
- the SDK's own parsing decides which records are asked about (`numbers`, `record-16385`).

Replay reproduces each of these faithfully. So an audit report for any Python task must say:
**the record proves what the runtime computed from these inputs under this contract; it does not
prove the script ran as its source reads, since hostile data may, through the runtime or its
limits, have chosen the labels asked, the notification's body or the result, within the
contract's grants.** In a live run a title that is not UTF-8 is refused `ModelRefused`
(`hyalos-os/ABI.md`), so `utf8`'s outcome holds for recorded runs only.

Collisions in the runtimes' string hashes were tested for both and had no effect, because the
triage script never hashes a title: under MicroPython the colliding titles' fuel is within 1,000
of the distinct control's (`MEASUREMENTS.md`, `collide` and `distinct`; `README.md`, Hostile inputs
and measurements). A task that keys a dict by its inputs would be exposed, since both hashes are
fixed by their source and seed and `fixtures/collide.py` aims at them.

### What capsules must decide

These belong to capsules, not to this spike, since each would change a default build, the guest
interface or the record format:

- **The kernel image's dispatch (#503).** Whether the default image is built with
  `portable-dispatch`, or the kernel stack grows, so that long guest loops run on KVM. Without it no
  interpreter runs on KVM.
- **Floats.** MicroPython needs none; Monty needs them with deterministic NaN, a new execution
  profile (ADR-002, ADR-0137).
- **The memory limit.** MicroPython runs in 4 MiB with a heap that is never collected, so the limit
  bounds a task's inputs; Monty needs 5 MiB.
- **Fuel.** Profile 2's 200,000,000 holds MicroPython on 200 issues; Monty would need several
  times its 830,000,000.
- **The script field.** The input loader splits files on newlines, so the spike delivers the SDK
  and the script as records 1 and 2 under its own framing (ADR-003); the contract needs a script
  field or a framed input.
- **Where type checking sits,** if at all, and that it never stands in for the monitor's check.

Signing the task identity was out of scope: the spike binds the contract, SDK and script into the
task identity (S-02.4, S-03), and signing that identity, and authorising one identity and not
another, is capsules' concern.

## Consequences

- `docs/plans/hyalos-kernel/pack-briefs.md`'s capsules section cites this ADR as the Python spike's
  result.
- Capsules starts from MicroPython's port in `spikes/438/micropython/`, its committed guest and its
  records, and from the decisions above.
- The spike's code stays behind `spike-contract` and in `spikes/438/`; nothing here changes a
  default build, the guest interface or the record format.
