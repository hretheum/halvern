# 01 — Design

## The question

`recommend_summary_model` picks the built-in summarization model from system RAM
alone. Across 0–128 GB and all five language groups it returns exactly two
answers: `qwen3.5:2b` below 14 GB, `qwen3.5:4b` at or above it. Neither Gemma is
ever recommended, on any machine.

That is not because Gemma lost a comparison. It is because the inherited
priority table ranks Qwen above Gemma, and each Gemma shares a RAM floor with
the Qwen that outranks it:

| Model | Size | RAM floor | Priority |
|---|---|---|---|
| `qwen3.5:4b` | 2614 MB | 14 GB | 4 |
| `gemma3:4b` | 2374 MB | 14 GB | 2 |
| `qwen3.5:2b` | 1221 MB | 0 | 3 |
| `gemma3:1b` | 1019 MB | 0 | 1 |

So whenever a Gemma clears the RAM filter, a higher-ranked Qwen clears it too.
Both Gemmas ship, are listed by `builtin_ai_list_models`, and are installable
from the model settings — but nothing ever suggests them, and onboarding never
shows the list at all.

This matters because Halvern is multilingual by design and the families differ
exactly there. Gemma 3 shipped with broad multilingual coverage as a stated
goal; small Qwen models have historically been strongest in English and Chinese.
"Always Qwen" may well be right at these sizes. Today it is an inherited
default, not a finding.

**What this experiment produces:** the contents of `language_score(model_name,
group) -> u8` in `summary/summary_engine/commands.rs`. That function already
takes the language group and currently ignores it (`let _ = group;`), returning
the inherited order. Filling it in is a table change, not a signature change.

## The finding that shaped the design

The pipeline does not summarize into the meeting's language. It **always
summarizes into English and then translates**.

`ENGLISH_BASE_SUMMARY_INSTRUCTION` in `summary/processor.rs` is prepended to
every chunk prompt, every combine prompt, and the final structured prompt:

> Write the summary/report in English regardless of transcript language;
> non-English prose is invalid.

Then `resolve_final_language_action` decides what happens next: return the
English as-is, run an English normalization pass, or call `translate_markdown`
— **with the same model** — to render the finished document into the target
language.

A Polish meeting summarized into Polish therefore runs:

```
transcript (pl)
  → chunk summaries (en)        model role 1: cross-lingual comprehension
  → structured summary (en)     model role 2: structured English generation
  → translate to pl             model role 3: translation with structure preserved
```

One model, three jobs. A model can be strong at one and weak at another, so a
single end-to-end score cannot tell you which pass failed — and the fix for each
is different. **The experiment measures the three roles separately and also
end to end.**

This also corrects an assumption worth stating so nobody re-derives it: output
language is not a property of the model's inclination here. It is produced by an
explicit translation pass with an explicit instruction. The interesting failure
is not "answered in the wrong language" but "mangled the document while
translating it", which is a different and much more measurable thing.

## What gets measured

Three capabilities, isolated:

**Role 1 — cross-lingual comprehension.** Read a transcript in language L,
produce faithful English content. Scored on planted-fact recall and fabrication
against a known ground truth. This is where a model that does not really read
Japanese will show up.

**Role 2 — structured English generation.** Fill the template's declared
sections in their declared formats. Scored on template compliance. Language-
independent, so it also serves as the control: a model that fails here fails
everywhere, and its language scores mean nothing.

**Role 3 — translation with structure preservation.** The translation prompt
demands that every `#`, `**`, `-`, `|` and table pipe stay in position. A small
model rendering a five-column Markdown table into Japanese and losing the pipes
produces a visibly broken document. This is objectively checkable by stripping
all prose and diffing the structural skeleton — no judge required, and it is the
single cheapest high-signal metric in the whole design.

## Two contests, not one

The recommendation never chooses across RAM tiers, so comparing `gemma3:1b`
against `qwen3.5:4b` answers no question the code asks.

- **Tier A (14 GB floor):** `qwen3.5:4b` vs `gemma3:4b`
- **Tier B (no floor):** `qwen3.5:2b` vs `gemma3:1b`

Each tier is scored independently and fills its own half of the table.

## Confounds that must be controlled

### The sampling presets differ between families

This is the most dangerous one, and it is not obvious from the model list. The
two families ship with different `SamplingParams`:

| | temperature | top_k | top_p | presence | repeat |
|---|---|---|---|---|---|
| `qwen35_summary` (both Qwens) | 0.5 | 20 | 0.8 | 0.3 | 1.05 |
| `gemma3_instruct` (both Gemmas) | **1.0** | 64 | 0.95 | 0.0 | 1.0 |

Gemma runs at twice the temperature with no repetition or presence penalty. Any
result of the form "Gemma fabricates more" would be partly the sampler, not the
model — and the sampler is ours to change.

**Therefore the experiment has two arms:**

- **Arm M (matched):** both models at `temperature 0`, `top_k 1`, penalties off.
  This answers *which model is better*, which is what `language_score` should
  encode.
- **Arm S (shipped):** each model with its own preset, exactly as users get it.
  This answers *which configuration is better today*, which is what actually
  reaches people.

**How Arm M is actually forced, and why it is not the obvious way.** The first
run of this experiment produced an "Arm M" that was nothing of the sort. The
harness passes `temperature: Some(0.0)` into `generate_meeting_summary`, which
looks like it should do it — and for the cloud providers it does. For the local
provider it is silently dropped: `llm_client.rs` short-circuits `BuiltInAI` into
`generate_with_builtin`, which takes no sampling arguments at all, and
`client.rs` builds the request from `model_def.sampling` alone.

So the local sampler cannot be set per call. It is decided in exactly one place,
`SamplingParams::sanitize_for_llama_helper`, and that is where the override
lives: setting `HALVERN_BENCH_GREEDY` forces temperature 0, `top_k` 1 and
penalties off for every built-in model. Unset in normal builds, which is the
whole of its safety, and there is a test pinning that.

The lesson is worth keeping: **the confound was identified in this document
before the first run and still went uncontrolled**, because passing a parameter
was mistaken for the parameter having an effect. Any arm that claims to control
something should be checked against what the model actually received, not
against what the harness sent.

If the two arms disagree, that is itself the finding: it means the preset is
carrying the difference and the cheaper fix is to retune the preset rather than
switch models.

### The chat templates differ

Qwen uses `qwen3.5_nonthinking`; Gemma uses `gemma3` with `<start_of_turn>` /
`<end_of_turn>` markers. If either family's prompt assembly is subtly wrong, the
experiment measures our bug and blames the model. **Pre-flight P0 exists to rule
this out before anything else runs.**

### Determinism

`llama-helper` exposes no seed. Temperature 0 with `top_k 1` is greedy and
deterministic in principle, but llama.cpp output can still vary with thread
count and batch composition. The protocol therefore fixes the thread count and
runs every case **three times**, not to average but to detect residual
nondeterminism. If repeats disagree in Arm M, the run is invalid and must be
re-examined before any conclusion is drawn.

### Single-pass versus map/reduce

`generate_meeting_summary` takes a `token_threshold` (default 4000). Below it,
one pass; above it, chunk summaries then a combine pass. These are different
workloads — the map/reduce path asks the model to summarize its own summaries —
and a model can be fine at one and poor at the other. The corpus contains both
regimes so the result is not an artefact of transcript length.

## What this experiment does not measure

Stated so the absence is deliberate:

- **Speed.** Worth knowing, recorded as a secondary column, but not a
  tie-breaker. A 15-second difference on a background job nobody watches is not
  a reason to accept worse summaries.
- **The cloud providers.** OpenAI, Anthropic, Groq and OpenRouter are the user's
  own choice and their own key. Only the built-in local lineup is in scope.
- **Transcription quality.** That is Parakeet versus Whisper and a separate
  question, settled by `choose_engine` on coverage rather than by measurement.
- **Whether the RAM floors are right.** 14 GB is the threshold this code shipped
  with and no measurement supports a different number. Changing it is out of
  scope here.

## Decision rules, fixed before any number is seen

Pre-registered so results cannot be rationalized afterwards.

**A Gemma replaces the Qwen for a language group when, in Arm M, within its
tier, all three hold:**

1. Fabrication rate is not worse by more than 1 percentage point.
2. Planted-fact recall is at least 3 percentage points higher, **or** structure
   preservation is at least 5 points higher with recall not worse than 2 points.
3. Template compliance is at or above 0.95, i.e. it is not winning on content
   while producing a document the app cannot render.

**Ties break on download size.** `gemma3:1b` is 1019 MB against `qwen3.5:2b`'s
1221 MB, and first-run weight is a launch risk in its own right
(LAUNCH_READINESS §2.3). Equal quality, smaller model wins.

**If Arm M and Arm S disagree**, the outcome is not a model switch. It is a
ticket to retune the losing model's `SamplingParams`, then re-run.

**If no rule fires**, `language_score` keeps the inherited order for that group
and the result is recorded as "measured, unchanged" — which is a real outcome
and the most likely one for English.
