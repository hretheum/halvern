# Summary model bake-off

**Which local summarization model should Halvern recommend, for which
languages — and is the inherited answer right?**

Status: designed, not yet run. Everything needed to run it is in this folder.

---

## Contents

| Document | What it settles |
|---|---|
| [01 — Design](01-design.md) | The question, the architecture finding that shaped it, the confounds, and the decision rules fixed in advance |
| [02 — Corpus](02-corpus.md) | What material to generate, in which languages, with what planted inside it |
| [03 — Prompts](03-prompts.md) | Every prompt, the model each runs against, and the rule about what may be shown to a cloud model |
| [04 — Measurement](04-measurement.md) | The six metrics, how each is computed, and what makes a run void |
| [05 — Runbook](05-runbook.md) | Step by step, with times, failure modes, and a short version |

### Scripts

| File | Purpose | State |
|---|---|---|
| [`scripts/validate_corpus.py`](scripts/validate_corpus.py) | Gate the corpus before a run costs an overnight | tested |
| [`scripts/score.py`](scripts/score.py) | All six metrics, locally; emits uncertain rows for adjudication | tested on fixtures |
| [`scripts/summary_bench.rs`](scripts/summary_bench.rs) | Drives the real summary path; `run.sh` drops it into `examples/`, never `src/bin/` — see below | written, unrun |
| [`scripts/run.sh`](scripts/run.sh) | Orchestrates a full run and records the conditions | written, unrun |
| [`scripts/extract-control-set.sh`](scripts/extract-control-set.sh) | Pulls six real meetings as a reality check | written, unrun |
| [`scripts/example-corpus-file.json`](scripts/example-corpus-file.json) | Worked example of a corpus file | validates |

---

## Why this exists

`recommend_summary_model` picks the built-in summarization model from system RAM
alone. Swept across 0–128 GB and all five language groups, it returns exactly
two answers: `qwen3.5:2b` below 14 GB, `qwen3.5:4b` at or above it. **Neither
Gemma is ever recommended, on any machine.**

Not because Gemma lost a comparison — because each Gemma shares a RAM floor with
a Qwen that outranks it in an inherited priority table. Both Gemmas ship, are
listed, and are installable from settings, but nothing suggests them and
onboarding never shows the list.

That would be unremarkable except that Halvern is multilingual by design and the
families differ exactly there: Gemma 3 shipped with broad multilingual coverage
as a stated goal, while small Qwen models have historically been strongest in
English and Chinese. "Always Qwen" may well be correct at these sizes. Today it
is an inherited default rather than a finding, and the model almost every user
ends up running was chosen by a RAM check that has nothing to say about
language.

## The finding that reshaped the design

**The pipeline never summarizes into the meeting's language. It always
summarizes into English, then translates.**

`ENGLISH_BASE_SUMMARY_INSTRUCTION` is prepended to every chunk prompt, every
combine prompt and the final structured prompt. Afterwards
`resolve_final_language_action` either returns the English, normalizes it, or
calls `translate_markdown` — **with the same model** — to render the document
into the target language.

So a Polish meeting summarized into Polish runs three jobs through one model:

```
transcript (pl)
  → chunk summaries (en)      cross-lingual comprehension
  → structured summary (en)   structured English generation
  → translate to pl           translation, structure preserved
```

A model can be strong at one and weak at another, and the fixes differ. A single
end-to-end score cannot tell you which pass failed, so **the experiment measures
the three separately**, capturing both the English intermediate and the final
document for every generation.

It also relocates the interesting failure. Output language is not a matter of
the model's inclination here — it is produced by an explicit instruction. The
question worth measuring is not "did it answer in the right language" but "did
it mangle the document while translating it", which is cheap and objective:
strip the prose, compare the Markdown skeletons.

## What the experiment produces

The contents of `language_score(model_name, group) -> u8` in
`summary/summary_engine/commands.rs`. That function already takes the language
group and currently ignores it, returning the inherited order. Filling it in is
a table change, not a signature change — which is why the plumbing was separated
that way in the first place.

| Group | Tier A winner | Tier B winner |
|---|---|---|
| English | | |
| WesternEuropean | | |
| Slavic | | |
| Cjk | | |
| Other | | |

Tier A is `qwen3.5:4b` vs `gemma3:4b` (both 14 GB floor); Tier B is
`qwen3.5:2b` vs `gemma3:1b` (both unfloored). Comparing across tiers answers no
question the code asks.

## The confound most likely to produce a wrong answer

The two families ship with **different sampling presets**:

| | temperature | top_k | top_p | presence | repeat |
|---|---|---|---|---|---|
| Qwen (`qwen35_summary`) | 0.5 | 20 | 0.8 | 0.3 | 1.05 |
| Gemma (`gemma3_instruct`) | **1.0** | 64 | 0.95 | 0.0 | 1.0 |

Gemma runs at twice the temperature with no penalties. Any "Gemma fabricates
more" result would be partly the sampler, and the sampler is ours to change. So
the experiment runs two arms: **matched** (both at temperature 0 — which model
is better) and **shipped** (each with its own preset — which configuration is
better today). If they disagree, the outcome is a retuning ticket, not a model
switch.

## Cost

864 generations for the full protocol — 36 transcripts × 4 models × 2 arms × 3
repeats — roughly 5 to 14 hours on Apple Silicon. One or two overnight runs.

There is a two-hour first pass in [05 — Runbook](05-runbook.md) §4 that answers
the narrower question of whether the small tier is broken for non-Latin scripts,
which is the finding most likely to change what ships.

## The rule about real meetings

The control set holds six real Polish meetings. **They are never sent to a cloud
judge**, under any circumstances, including to adjudicate a scoring edge case.
They are scored only on the metrics that need no ground truth. `score.py`
enforces this by excluding them from the adjudication file rather than relying
on anyone remembering.

Measuring a privacy product by mailing its users' meetings to a third party is
not a trade worth making for a table of integers.

---

## Related

- [`LAUNCH_READINESS.md`](../../../LAUNCH_READINESS.md) §4 — the open item this
  experiment closes
- [`docs/ONBOARDING_LANGUAGE_MODEL.md`](../../ONBOARDING_LANGUAGE_MODEL.md) —
  the correctness half of the language problem, which did not wait for
  measurement
- `frontend/src-tauri/src/summary/summary_engine/commands.rs` — the function
  this experiment fills in
- `frontend/src-tauri/src/summary/processor.rs` — the three-pass pipeline under
  test
