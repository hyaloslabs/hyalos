# Recording the two-minute demo

The demo shows two attacks being contained: a poisoned skill refused, and a steered job held. It is a test, `core/hyalos/tests/demo.rs`, so CI checks every run that what the video shows is what Hyalos does. It runs against keys, a GitHub and a model made for the test alone. It needs no secret, no network and no container runtime, and it touches nothing of yours.

## Run it

```
cargo test -p hyalos --test demo --locked -- --nocapture --test-threads=1
```

Both acts print what happens and pass in a few seconds. If either fails, the behaviour has changed. Do not record until it passes.

## Record it

The cast is short, so record one act at a time:

```
asciinema rec -c "cargo test -p hyalos --test demo --locked -q -- --nocapture --exact act_1_a_poisoned_skill_is_refused" act-1.cast
asciinema rec -c "cargo test -p hyalos --test demo --locked -q -- --nocapture --exact act_2_a_steered_job_is_held" act-2.cast
```

Build once beforehand (`cargo test -p hyalos --test demo --no-run`), so that no compile output is recorded. A terminal 100 columns wide fits every line.

## Storyboard (about two minutes)

| Time | On screen | Say |
| --- | --- | --- |
| 0:00–0:15 | The README's first lines | "Hyalos runs coding agents in a sandbox that holds no secrets, and records every decision in a signed log. Here are two attacks it stops." |
| 0:15–0:30 | Act 1's opening lines | "A skill from somewhere on the internet. Its description tells whoever reads it to grant it everything. It links to my SSH key, and it wants to reach a paste site." |
| 0:30–0:45 | The first refusal: `it holds a link` | "Hyalos reads the skill before anything runs it. A link out of the skill is refused." |
| 0:45–1:05 | The second refusal: `+ paste.example`, `run again with --grant` | "Without the link, it still asks to reach paste.example. Hyalos shows me exactly what it asks for and stops. The skill's words never reach my screen. Only my signed grant could install it." |
| 1:05–1:20 | Act 2's opening lines | "Now a job driven by an issue. My policy lets an issue into a job only when the people who wrote it can write to the repository. This one was written by an outsider." |
| 1:20–1:40 | `your policy did not release issue #7`, then `held as handle …` | "The reader steps see the issue, but the actor, the part that can push code, never does. The job waits for me, and its GitHub token is revoked." |
| 1:40–2:00 | `Failed (run.cancelled)`, then `hyalos log verify` | "I decline it. Nothing was pushed, no one released it, and the signed log records every step." |

## Keep it true

- Change the demo only together with its assertions. The test fails if the output it relies on changes.
- Don't edit the cast to show something the test did not print.
- Each act's story rests on these ADRs: 0088 and 0089 (reading and installing a skill), 0012 (a human grants capabilities), 0078 and 0103 (an issue is released to a job), 0063 (handles), and 0018 (the signed log).
