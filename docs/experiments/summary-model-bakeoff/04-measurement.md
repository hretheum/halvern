# 04 — Measurement

Six metrics. Five are computed without a model; one needs a judge for a minority
of rows. Every metric is defined so that a disagreement about a number can be
settled by looking at the artefact rather than by argument.

## What is captured per generation

`summary_bench` writes one JSON record per generation into
`results/raw/<run-id>/`:

```json
{
  "transcript_id": "ja-long-02",
  "model": "gemma3:4b",
  "arm": "M",
  "repeat": 2,
  "language": "ja",
  "group": "Cjk",
  "length_class": "long",
  "chunks_used": 3,
  "english_intermediate": "...markdown before translation...",
  "final_markdown": "...markdown after translation...",
  "wall_ms": 41230,
  "processor_sha": "a1b2c3d",
  "sampling": { "temperature": 0.0, "top_k": 1, "top_p": 1.0 }
}
```

Capturing **both** the English intermediate and the final document is what makes
the three model roles separable. Without the intermediate you cannot tell a
comprehension failure from a translation failure, and the two have different
fixes.

## M1 — Structure preservation (translation pass)

**Role measured:** 3. **Judge:** none. **Range:** 0–1.

The translation prompt requires that every `#`, `**`, `-`, `|` and table pipe
stay in position. Reduce both documents to their structural skeleton — headings
by level, list markers, table row shapes, emphasis markers, code fences — with
all prose removed, then compare.

```
skeleton(md) = ordered sequence of structural tokens, prose stripped
M1 = 1 - (levenshtein(skeleton(english), skeleton(final)) / len(skeleton(english)))
```

For an English target no translation pass runs, so M1 is 1 by definition and the
row is excluded from cross-language comparison rather than counted as a win.

This is the cheapest high-signal metric in the design. A small model rendering a
five-column table into Japanese and dropping the pipes produces a visibly broken
document, and it shows up here as a number without anyone reading Japanese.

## M2 — Template compliance

**Role measured:** 2. **Judge:** none. **Range:** 0–1.

Parse the final document against `templates/standard_meeting.json`:

| Check | Weight |
|---|---|
| All 4 declared sections present, as headings | 0.4 |
| `paragraph` sections contain prose, not a list | 0.2 |
| `list` sections contain at least one item | 0.2 |
| Action Items table has the 5 declared columns | 0.2 |

M2 is scored on the **final** document, because that is what the user sees, and
on the **English intermediate** separately as `M2_en`. If `M2_en` is high and M2
is low, the translation pass broke the structure and M1 will agree — two
independent metrics pointing at the same pass is the confirmation.

A model below 0.95 on `M2_en` has failed the control condition. Its language
scores are not interpretable and should be reported as such rather than ranked.

## M3 — Planted-fact recall

**Role measured:** 1. **Judge:** for uncertain rows only. **Range:** 0–1.

For each of the 12 facts, search the **English intermediate** (not the
translation — this measures comprehension, not translation) for `canonical` or
any `paraphrase`.

Three-way outcome per fact:

- **HIT** — a normalized match on canonical or a paraphrase
- **MISS** — no candidate sentence mentions the fact's key entities at all
- **UNCERTAIN** — the fact's entities appear but no phrasing matched

`UNCERTAIN` goes to P3. Expect 10–20% of rows. Everything else is settled
locally.

```
M3 = (HIT + judge_present) / 12
```

For `action` facts the owner must also be right: a task attributed to the wrong
participant is a MISS, not a partial hit. Getting the task and losing the owner
produces a summary that sends work to the wrong person, which is worse than
omitting it.

## M4 — Fabrication rate

**Role measured:** 1. **Judge:** for flagged rows only. **Lower is better.**

Two components.

**M4a — decoys asserted.** For each of the 4 decoys, does the document present
it as settled, or give it an owner or due date? Flag if the decoy's entities
appear in a Key Decisions or Action Items section; adjudicate flagged rows with
P3b.

**M4b — unsupported entities.** Person-names and figures appearing in the
document that appear nowhere in the transcript. Distractors are the trap: they
are *in* the transcript, so they are not unsupported — but a distractor promoted
into Action Items is caught by M4a's logic extended to distractors.

```
M4 = (decoys_asserted + unsupported_entities) / (4 + entity_count)
```

**This is the metric that decides trust.** A model that invents a decision has
produced a document that is worse than no document, because a person acts on it.

## M5 — Reversal correctness

**Role measured:** 1. **Judge:** rarely. **Range:** 0 or 1.

1 if the document states `reversal.final` and does not state
`reversal.initial`. 0 otherwise, including when both appear without the change
being marked.

A single binary per transcript, so it is noisy per row and only meaningful
aggregated over the 36. It is kept because it is the cleanest possible test of
whether the model tracked the conversation rather than pattern-matched it.

## M6 — Output language correctness

**Role measured:** 3. **Judge:** none. **Range:** 0 or 1.

Language-identify the final document's prose, ignoring proper nouns, code spans
and table scaffolding. 1 if it matches the requested target.

Expected to be near-perfect because the translation pass is explicit — which is
the point. **A failure here is a pipeline bug, not a model preference**, and
should be filed as such rather than folded into a quality ranking.

**Two things M6 provably cannot see, both observed in the first run.**

Script detection cannot separate Chinese from Japanese. `qwen3.5:2b` returned a
Japanese document containing `本次会议は` — simplified Chinese, where Japanese
would be 本会議 — and M6 scored it a clean 1, because the characters are CJK
either way. Chinese contamination in Japanese output is a known small-model
behaviour and a real quality difference, and nothing here measures it.

Partial translation also passes. `gemma3:4b` translated German prose correctly
but left the title and the `**Summary**` labels in English; `qwen3.5:2b` did the
same on Japanese with `## Summary`. The document is majority target-language, so
M6 is satisfied, while a reader sees a bilingual document.

Both belong to a judge pass or a native reader, and neither should be inferred
from these numbers. Where a tier is close on the automatic metrics, that is the
signal to go and read the documents rather than to declare a winner.

## Aggregation

Per (model, group, arm), averaged over transcripts and repeats:

```
M1 structure   mean, and worst case
M2 compliance  mean of final, mean of english_intermediate
M3 recall      mean
M4 fabrication mean, and worst case
M5 reversal    proportion correct
M6 language    proportion correct
wall_ms        median  (secondary, never a tie-breaker)
```

Report **worst case alongside mean** for M1 and M4. A model that is fine on
average and catastrophic on one transcript in ten is not fine; the tail is what
users hit and post about.

Repeats are for detecting nondeterminism, not for averaging it away. In Arm M,
if the three repeats of a cell differ on any metric, mark the cell `UNSTABLE`
and investigate before drawing conclusions — greedy decoding that is not
reproducible means something else in the run is varying.

## Applying the decision rules

The rules are fixed in [01-design.md](01-design.md) and repeated here as the
arithmetic:

Within a tier, for a language group, Gemma replaces Qwen when **all** hold in
Arm M:

1. `M4(gemma) - M4(qwen) <= 0.01`
2. `M3(gemma) - M3(qwen) >= 0.03`, **or**
   `M1(gemma) - M1(qwen) >= 0.05` and `M3(gemma) - M3(qwen) >= -0.02`
3. `M2_en(gemma) >= 0.95`

Ties break on size: `gemma3:1b` (1019 MB) over `qwen3.5:2b` (1221 MB).

If Arm M and Arm S disagree, the output is a sampling-preset ticket, not a model
switch.

## The output artefact

The experiment's product is this table, filled in — one row per group per tier,
each cell the winning model and the margin that won it:

| Group | Tier A winner | margin | Tier B winner | margin |
|---|---|---|---|---|
| English | | | | |
| WesternEuropean | | | | |
| Slavic | | | | |
| Cjk | | | | |
| Other | | | | |

which becomes the body of `language_score(model_name, group) -> u8`, and
`results/REPORT.md` records how each cell was reached — including the cells
where nothing changed, because "measured, unchanged" is a result and next year
somebody will want to know it was checked.

## Validity conditions

The run is void, and must not be used to change the table, if any of these hold:

- any model failed **P0**
- the corpus did not pass `validate_corpus.py`
- `processor.rs` changed mid-run (the recorded SHA differs between records)
- any Arm M cell is `UNSTABLE`
- the Slavic ranking on synthetic disagrees with the real-meeting control set
- fewer than 3 of the 6 language slots produced usable data for a tier
