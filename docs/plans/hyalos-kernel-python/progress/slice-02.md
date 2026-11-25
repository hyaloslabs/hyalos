# Slice 2 — A contract yields its SDK

| | |
|---|---|
| **Status** | done |
| **Issue** | #495 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 (`claude-opus-5-5`) |
| **Date** | 2026-11-25 |

## Preflight

Checked on the branch's base, `f6a1605` (Slice 1 merged).

| Citation | Holds |
|---|---|
| `hyalos-os/guest/src/lib.rs:26-213` the digest guest's rules; `:164-208` its notification and result | holds |
| `hyalos-os/guest/src/lib.rs:28` the 16,384-byte record buffer; `:128-131` a `-2` record counts as other | holds |
| `hyalos-os/audit/src/record.rs:205` `task_v4` | holds |
| `hyalos-os/ABI.md:70-71` the issue fixture's `<number>\t<title>` lines | holds |
| `spike-run` and `spike-task` as Slice 1 built them (`hyalos-os/monitor/src/spike.rs`) | holds |
| `hyalos-os/monitor/tests/release_switches.rs:82` `built_kernel`, which `kernel.sh` builds as | holds |
| `spikes/73/README.md`, `spikes/73/run-python.sh` a spike rig's shape | holds |
| `.github/workflows/ci.yml:233-325` the `hyalos-os` job | differs, not a contract: Slice 1 moved the feature's tests into a job of its own, `hyalos-os (spike contract)`. The point it backs, that CI runs Rust tests and not `check.sh`, still holds |

## Acceptance

`spikes/438/generate/check.sh` output, as run:

```
generated twice, identical and committed: draft (c79ad7370429b681)
generated twice, identical and committed: no-notify (ac336640f1e8cab4)
generated twice, identical and committed: no-ask (69966c5443266bae)
refused: reordered (refused: not a canonical contract: it must begin {"asks":)
refused: padded (refused: not a canonical contract: a list must open with [)
refused: newline (refused: not a canonical contract: nothing may follow the closing })
names: draft: module: task; task: ask.classify finish notify.team read_inputs
names: no-notify: module: task; task: ask.classify finish read_inputs
names: no-ask: module: task; task: finish notify.team read_inputs
triage.py: imports only `from hyalos import task`; other names: its own and builtins,
  none of 16 that reach outside the script, and no attribute starting _
2.3: notification 'total 40 bug 0 feature 0 security 0 other 40\n', result 'ok 40'
fixture edge exact-last exact-not-last over-last over-not-last
2.2: fixture: same as the digest guest (3581 bytes, 40 security lines, not truncated, ok 40)
2.2: edge: same as the digest guest (4159 bytes, 1 security lines, truncated, ok 200)
2.2: exact-last: same as the digest guest (8000 bytes, 2 security lines, not truncated, ok 2)
2.2: over-last: same as the digest guest (3962 bytes, 1 security lines, truncated, ok 2)
2.2: exact-not-last: same as the digest guest (8000 bytes, 2 security lines, truncated, ok 3)
2.2: over-not-last: same as the digest guest (3962 bytes, 1 security lines, truncated, ok 3)
no-notify: AttributeError after 40 questions, no effect
check: ok
```

| ID | How it was shown |
|---|---|
| S-02.1 | the two rows below |
| S-02.1.e1 | `check.sh`: each contract generated twice, byte-identical, and equal to the committed `sdk/*.py` |
| S-02.1.e2 | `check.sh`: a reordered contract, a whitespace-padded one and one ending in a newline are each refused, with no output |
| S-02.2 | S-02.2.e1 |
| S-02.2.e1 | `check.sh`: the draft SDK's module has only `task`, whose public members are `ask.classify`, `finish`, `notify.team`, `read_inputs` |
| S-02.3 | the two rows below |
| S-02.3.e1 | `check.sh`: the `no-notify` SDK has no `notify` |
| S-02.3.e2 | `check.sh`: the `no-ask` SDK has no `ask` |
| S-02.4 | the two rows below |
| S-02.4.e1 | Verify: `spike-task` over the digest guest, the triage script and the fixture gives `58417a84…cf9b` for `draft` and `66cc0197…7a8c` for `no-notify` (`sort -u … \| wc -l` prints 2); `cmp` of `sdk/draft.py` and `sdk/no-notify.py` differs (`check.sh`'s hashes above) |
| S-02.4.e2 | Verify: the same loop run twice gives the same two identities (`cmp` passes), and the `draft` identity is the `task` field of the record `spike-run` wrote (`grep -c` prints 1) |
| 2.1 | `spikes/438/contracts/{draft,no-notify,no-ask}` in canonical bytes, no trailing newline; `spikes/438/sdk/{draft,no-notify,no-ask}.py` committed, checked by `check.sh` |
| 2.2 | `spikes/438/scripts/triage.py`; `check.sh` parses it and finds exactly one import, `from hyalos import task`, no name but its own variables, `task` and builtins, none of the builtins that reach outside the script (`__import__`, `exec`, `open`, `getattr` and others), and no attribute starting `_`. It keeps the digest guest's rules: with every answer security, `check.sh` compares its notification and result with a line-by-line port of `hyalos-os/guest/src/lib.rs` on the fixture, on inputs at its limits (titles of 4,095 to 4,097 bytes, a record over 16,384 bytes, leading zeros, `u32`'s bound, 213 records), and on digests landing exactly on 8,000 bytes and one byte past, with the long line last and not last; all are identical. Making the last-line rule `<` instead of `<=` fails it on `exact-last`. Also, once, not committed: with a Python port of the stand-in model at seed 0, its notification and result were byte-identical to the digest guest's profile 2 record on the fixture |
| 2.3 | `check.sh`: `total 40 bug 0 feature 0 security 0 other 40\n` and `ok 40` |
| 2.4 | `spikes/438/README.md`: the layout table and **Regenerate the SDKs** (`grep -c regenerate` prints 1) |
| 2.5 | `spikes/438/kernel.sh hyalos.profile/2` and `hyalos.spike/1` print the default image's path (`test -s` passes); `kernel.sh hyalos.spike/9` exits 1 |

### Verify, as run

From the repository root, every line exited as expected:

- `spikes/438/generate/check.sh`: `check: ok`.
- `test -s "$(spikes/438/kernel.sh hyalos.profile/2)"`: 0.
- `spike-run` of the digest guest with `draft`: `ok: 43 inputs, 40 inferences, 1 effect, fuel 85840`,
  then `timing: cold_start 0.580 ms, run_time 0.696 ms, backend process`.
- `spike-run` with `no-notify`: exits non-zero; `grep -c UnknownEffect` prints 1; no record written.
- `spike-task` loop twice: `cmp` passes; `sort -u | wc -l` prints 2; the `draft` identity
  `58417a84d3a8fa59ac569a5187403327ddb4195878d8f8851699dafb7891cf9b` appears once in the record.
- `grep -c regenerate spikes/438/README.md`: 1.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| `design.md` §2 Generator: refuses a non-canonical contract | It also refuses a contract granting a value outside the fixed name table, with the same rule as the monitor's parser (`hyalos-os/audit/src/contract.rs`) | The name table and the host's supported values are the same set today; one rule in both places means a contract the generator accepts is one the monitor accepts | n |

## Decisions

- **The SDK's shape.** Plain classes (`_Task`, `_Ask`, `_Notify`, `_Issue`) with methods defined in
  the class body, and `task.ask` and `task.notify` set only when granted. No decorators, closures,
  `lambda`, `types` or other imports, so the SDK asks little of a small interpreter.
- **Bytes, not text.** `read_inputs` gives each issue's `title` as the record's bytes and its
  `number` as an `int`, or `None` when the number is not strict decimal or over `u32`'s range (as
  the digest guest's `parse_number`). A record with no tab, or an empty one (`read`'s `-2`, too
  large), gives `number` and `title` both `None`. `classify`, `team` and `finish` take bytes, or a
  `str`, which they encode as UTF-8; anything else raises `TypeError`, so a script's mistake ends
  the run `error TypeError` rather than sending something. Titles are never decoded, so invalid
  UTF-8 travels as bytes.
- **The 200-record cap is the script's** (`task.read_inputs()[:200]`), as the design gives it to
  the triage script. `read_inputs` reads every record until `read` gives `None`.
- **`fake_hyalos.py`** runs the script as a runtime guest's `run` will: the SDK executed as module
  `hyalos` in `sys.modules`, then the script as `__main__`, and an uncaught exception ending the run
  with `error <TypeName>`, and a record over 16,384 bytes read as `b""`, as `read`'s `-2`. `check.sh` also runs the `no-notify` SDK this way and sees
  `error AttributeError` after the 40 questions, the shape S-04.2.e1 then checks in a guest.
- **`kernel.sh`** builds the default image for `hyalos.profile/2` and `hyalos.spike/1` only, with
  cargo's environment cleared as `built_kernel` does, so it works when called from a cargo run.
  Every other name exits 1 until a slice adds a kernel image feature for it.
- **Host Python:** 3.12.3, recorded in `spikes/438/README.md`.

## Discoveries

- For Slices 3 and 4: the SDK needs from a runtime only classes with methods, instance
  attributes, `isinstance`, `raise TypeError`, `bytes`, `str.encode`, `bytes.find`, slicing,
  iteration over `bytes` giving integers, lists and tuples. `triage.py` adds a dict, `range`, `len`,
  `str`, tuple unpacking and `+=` on bytes.
- The digest guest reads records 0 to 2 as issues too, so under a spike profile its notification
  says `total 43` and its fuel differs from the profile 2 run (85,840 against 29,845 at seed 0);
  3.3's comparison is against the digest guest on the fixture's lines alone, as the plan says.
