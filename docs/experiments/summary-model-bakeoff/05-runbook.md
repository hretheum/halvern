# 05 — Runbook

Do these in order. Each step names what would make it fail and what to do then.

## Prerequisites

- All four built-in models downloaded, from the app's model settings. Roughly
  7.2 GB total.
- A machine with at least 14 GB RAM, or Tier A cannot run at all.
- `python3`, a Rust toolchain, and the `llama-helper` sidecar built
  (`cd frontend && node scripts/build-sidecar.js`).
- `processor.rs` committed and clean. `run.sh` refuses to start otherwise,
  because the recorded SHA has to describe what actually ran.

## 1. Generate the corpus — about 2 hours, mostly waiting

Run P1 from [03-prompts.md](03-prompts.md) 36 times against Claude Opus 5, once
per row of the slot table. **Fresh context each time** — six Polish transcripts
generated in one session come out as six variations of the same meeting, and
the corpus needs independent samples.

Save to `corpus/<lang>/<id>.json`.

```bash
python3 scripts/validate_corpus.py corpus/
```

Expect failures on the first pass; the generator drifts on the fact-kind counts
and on cancellation cues, which is what the validator is for. Regenerate the
files it names rather than hand-patching the ground truth — a hand-edited fact
is exactly the kind of thing that turns out to be untrue of the transcript.

`scripts/example-corpus-file.json` is a worked example. It passes every check
except the token band, deliberately: it is short enough to read in one sitting,
which a real corpus file is not.

## 2. Extract the control set — 5 minutes

```bash
./scripts/extract-control-set.sh 6
```

Writes six real Polish meetings to `corpus/_control/`. Confirm they are
git-ignored before committing anything in this folder.

## 3. Pre-flight P0 — 15 minutes, and it can end the experiment early

Run both P0 probes against each of the four models. This is a human read, not a
script: you are checking that no chat-template markers leak into the output and
that the table comes back well-formed.

**If a model fails, stop and fix the template handling.** A model that cannot
follow a three-line instruction is not telling you anything about Japanese
meeting summaries, and every number you collect afterwards will be about our
bug.

## 4. The cheap first pass — under 2 hours

Before committing to a full run, find out whether any signal exists.

```bash
./scripts/run.sh smoke M          # then answer y at the P0 gate
```

with `--repeats 1` and the corpus cut to `ja` and `en` only. Two languages, the
extremes: if Gemma and Qwen are indistinguishable on Japanese, the multilingual
hypothesis is weak and the full run is a formality you may still want but can
schedule rather than rush.

Look at M1 first. Structure preservation on Japanese is the metric most likely
to separate the models and it needs no judge.

## 5. Full run, Arm M — 5 to 8 hours, overnight

```bash
./scripts/run.sh 2026-08-19 M
```

Tier A then Tier B, three repeats, all 36 transcripts plus the control set.
Records land in `results/raw/2026-08-19-M/`.

While it runs, nothing else heavy on the machine: the harness measures wall time
as a secondary column and a parallel build makes that column meaningless.

## 6. Full run, Arm S — same again

```bash
./scripts/run.sh 2026-08-19 S
```

Each model with its own shipped `SamplingParams` — Qwen at temperature 0.5 with
penalties, Gemma at 1.0 without. This is what users actually get.

## 7. Adjudicate the residue — 30 to 60 minutes

If `results/uncertain.json` exists, work through it with P3 and P3b from
[03-prompts.md](03-prompts.md), collecting answers into `adjudications.json`
keyed exactly as the uncertain file is.

```bash
python3 scripts/score.py --raw results/raw/2026-08-19-M --corpus corpus \
    --adjudications adjudications.json \
    --report results/REPORT-2026-08-19-M.md
```

If more than 25% of rows came back uncertain, the paraphrase lists are too thin.
The scorer warns about this. Widen them and re-score — the generations do not
need repeating, only the matching.

## 8. Check validity before reading anything

From [04-measurement.md](04-measurement.md). The run is void if any hold:

- a model failed P0
- the corpus did not pass the validator
- `processor_sha` differs between records (the scorer exits 2 on this)
- any Arm M cell is `UNSTABLE` — the three repeats disagree
- the Slavic ranking on synthetic disagrees with the control set
- fewer than 3 of 6 language slots produced usable data for a tier

An `UNSTABLE` cell is the one worth pausing on. Arm M is greedy decoding; if it
is not reproducible, something else varied, and finding out what matters more
than the table.

## 9. Apply the result

Only if all four decision-rule conditions fire for a cell. Edit
`language_score` in `summary/summary_engine/commands.rs`:

```rust
fn language_score(model_name: &str, group: LanguageGroup) -> u8 {
    match (model_name, group) {
        // filled from results/REPORT-*.md, with the run id in the comment
        ("gemma3:4b", LanguageGroup::Cjk) => 5,
        _ => summary_model_priority(model_name),
    }
}
```

Then:

- the existing test `available_summary_model_priority_prefers_qwen_over_gemma`
  asserts the inherited order and **will fail** — that is correct, and the fix
  is to rewrite it around the measured order, citing the run id
- add a test pinning each changed cell, so a future edit cannot silently undo a
  measured result
- write `results/REPORT.md` including the cells where nothing changed. "Measured,
  unchanged" is a result, and next year somebody will want to know it was
  checked rather than assumed

## 10. Update the launch plan

`LAUNCH_READINESS.md` §4 currently says nobody has evaluated the lineup across
languages. Replace that with what was measured, what changed, and what did not.

## If you are short of time

The minimum that still produces a defensible answer:

- Arm M only, 1 repeat, `ja` + `pl` + `en` — 18 transcripts, about 90 minutes
- score M1, M2_en and M4 only; skip recall adjudication entirely

That will not fill the whole table, but it will answer the specific question of
whether the small tier is broken for non-Latin scripts — which is the finding
most likely to change what ships, and the one a user would notice first.
