# Bake-off result — 19 August 2026

**Outcome: Tier A settled for Qwen; Tier B re-opened by our own fix.**
`language_score` is unchanged for now, because nothing in this run justified
changing it — but the hygiene failure that disqualified `gemma3:1b` is a defect
we have since fixed in the pipeline, so the small tier deserves a re-run before
its default is treated as decided.

That is the least interesting sentence in this document. What the run actually
produced is four defects — one in the product, three in the measuring apparatus
— and a specific finding about the small tier that has nothing to do with
ranking.

Run at `processor.rs` `4d6e76e`. Five transcripts (one per `LanguageGroup`),
four models, two arms, one repeat: 40 generations.

## The decision

Rules were fixed in [01-design.md](../01-design.md) before any number was seen.
A Gemma replaces the Qwen in its tier only if, in the matched arm, fabrication
is no worse, recall is at least 3 points better, **and** template compliance on
the English intermediate is at or above 0.95.

**Tier A — `qwen3.5:4b` vs `gemma3:4b`.** Qwen leads recall in all five groups
under matched sampling: English 0.917 vs 0.750, WesternEuropean 0.583 vs 0.333,
Slavic 0.750 vs 0.667, Cjk 0.583 vs 0.250, Other 0.583 vs 0.417. Rule 2 fails
everywhere; no cell changes. Gemma also fails output hygiene on Turkish.

**Tier B — `qwen3.5:2b` vs `gemma3:1b`.** Rule 3 disqualifies `gemma3:1b` in
every group: its English-intermediate compliance is 0.80–0.90 and never reaches
0.95. It also fails output hygiene in all five groups and fabricates more.
No cell changes.

`language_score` therefore keeps `summary_model_priority` for every group. The
table is unchanged and now measured rather than assumed.

## The finding that is not a ranking

**Neither small model is fit for CJK, and they fail differently.**

`qwen3.5:2b` wrote its *English* intermediate entirely in Chinese for the
Japanese transcript, under greedy decoding, despite the prompt stating
`non-English prose is invalid`. Recall scored 0.000 because there was no English
to match. The same document invented a statistic — "90% of orders from
high-value customers", where the transcript says 19% use instalments — and
assigned SecurePay implementation to van Dijk instead of Sasaki.

`gemma3:1b` headed every summary in every language `# <Add Title here>`, an
unfilled placeholder from the prompt, and on Japanese returned the whole
document inside a code fence with the prompt's `<document>` tag still attached.
`clean_llm_markdown_output` strips neither. This survived greedy decoding, so it
is instruction-following, not sampling.

Both defects reach the user's screen. Neither is a matter of degree.

## Defects found in the product

**`rough_token_count` is Latin-only.** A flat `chars × 0.35` for every language,
roughly three times wrong for CJK. It decides single-pass versus map/reduce and
sizes the chunks, so a Japanese meeting is sent whole when it should be split.
Recorded as LAUNCH_READINESS §2.4. Found while writing the corpus, not while
looking for it.

**Prompt scaffolding is not stripped.** `clean_llm_markdown_output` passes
through `<document>`, a wrapping code fence, and `<Add Title here>`. Only the
weakest model produced these, but the pipeline should not depend on the model
being strong enough to avoid them.

**Partial translation is invisible to the pipeline.** `gemma3:4b` translated
German prose correctly while leaving the title and `**Summary**` labels in
English; `qwen3.5:2b` did the same on Japanese. The user sees a bilingual
document and nothing detects it.

## Defects found in the measuring apparatus

Recorded because a measurement is only as good as its instrument, and three of
these would have produced a confidently wrong answer.

**The matched arm was not matched.** The harness passed `temperature: 0` and it
was silently dropped: `generate_with_builtin` takes no sampling arguments, so
local sampling comes entirely from `model_def.sampling`. The first run compared
Qwen at 0.5 against Gemma at 1.0 while claiming to have controlled exactly that.
Fixed with `HALVERN_BENCH_GREEDY` at the single choke point, verified live by
diffing the same input across arms. **The confound was named in the design
document and still went uncontrolled**, because passing a parameter was mistaken
for the parameter having an effect.

**Template compliance recognised only `#` headings** and scored a perfectly
well-formed document at zero — the application's own prompt emits `**Summary**`.
That would have made M2 useless as the control condition it exists to be.

**Structure preservation credited a model for not translating.** A document
returned unchanged scores a perfect skeleton match. Untranslated rows are now
excluded.

**Language identification called an English document German** until diacritic
signatures replaced the stopword list — and it still cannot separate Chinese
from Japanese, which is how the Chinese intermediate reached the report at all.
M8 was added for that, and caught it.

**Fabrication was reported as zero** while nothing had been adjudicated, since
pending decoys counted as clean. The aggregate now refuses to look finished.

## Fabrication, adjudicated by hand

Nineteen flagged decoys in the shipped arm, twenty-eight in the matched one.
Six were real. Every one is the same failure: **substantive work assigned to a
topic nobody took** — Luca given the analytics migration with a due date, Priya
given a beta programme, van Dijk given the Osaka office.

The criterion was refined mid-adjudication and then reapplied to both arms:
tracking that a decision is pending, with the deferral cue quoted, is faithful
reporting rather than fabrication. That reversed one earlier verdict. The change
is recorded here because revising a judgement after seeing more cases is only
honest if it is applied uniformly.

`qwen3.5:4b` fabricated nothing in either arm, in any language.

## The CJK cell, settled with repeats

The report first said the Tier B CJK cell was too noisy to read: `qwen3.5:2b`
scored 0.500 recall on Japanese shipped and 0.000 matched. Twelve more
generations — four models, three repeats, Japanese only, greedy — settle it.

| model | 3 repeats identical | English intermediate | scaffolding | median |
|---|---|---|---|---|
| `qwen3.5:4b` | yes | yes | clean | 108 s |
| `gemma3:4b` | yes | yes | clean | 74 s |
| `qwen3.5:2b` | yes | **no — writes CJK** | clean | 38 s |
| `gemma3:1b` | yes | yes | **placeholder, `<document>`, fence** | 29 s |

Two things follow.

**The methodology is sound.** All four models returned byte-identical output
across three repeats, so greedy decoding in this pipeline is deterministic and
the `UNSTABLE` validity condition does not fire. The 0.500 → 0.000 swing was not
noise: it was the sampler. Shipped sampling happened to produce English; greedy
reliably produces Chinese.

**Neither small model is usable for Japanese, reproducibly.** `qwen3.5:2b` fails
at the first stage, writing its English intermediate in Chinese every time.
`gemma3:1b` gets the intermediate right and then puts the prompt's own
scaffolding on the reader's screen every time.

## What changed in the product because of this

Both defects the run surfaced are now fixed, with tests transcribed from the
actual failing output rather than invented:

- `rough_token_count` counts CJK at about one token per character instead of
  0.35, so the chunking threshold means for Japanese what it already meant for
  Latin scripts.
- `clean_llm_markdown_output` removes an opening code fence whether or not the
  model closed it, strips our own `<document>` and `<transcript_chunk>` tags,
  and drops an unfilled `# <Add Title here>` line.

**This changes what a re-run would find.** `gemma3:1b` was disqualified in Tier
B by decision rule 3 on exactly the hygiene failures the second fix now handles.
With the fix in place its output is clean, and Tier B becomes a genuine contest
that this run cannot settle — `gemma3:1b` already wins recall in Other in both
arms, and in WesternEuropean and Cjk under matched sampling.

So the conclusion narrows honestly: **Tier A is settled for Qwen; Tier B is
re-opened by our own fix** and should be re-run before anyone treats
`qwen3.5:2b` as the right default for the small tier.

## What this result is not

- **n=1 per cell.** One transcript per language, one repeat. Cells move by more
  than their differences: `qwen3.5:2b` scored 0.500 on Cjk recall shipped and
  0.000 matched. That is a direction, not a measurement.
- **Recall is understated for everyone.** Unadjudicated facts count as misses,
  and about 40% of rows were uncertain. The ordering is probably stable; the
  absolute numbers are not.
- **No long transcripts.** The map/reduce path was never exercised. Every
  generation took the single-pass branch.
- **No control set.** Real meetings were not run, so the synthetic result has
  not been checked against real Polish.
- **The judge was the same model that wrote the corpus.** Adjudication was done
  by hand against a written criterion rather than by a fresh model, which is
  weaker than the protocol specifies.

Under [04-measurement.md](../04-measurement.md)'s validity conditions this run
does not qualify to change the table. It did not need to — nothing changed. It
would not have been enough to justify a change.

## What to do next

1. Fix `rough_token_count` for CJK. Small, and it affects the users this work
   exists to serve.
2. Strip prompt scaffolding in `clean_llm_markdown_output` rather than relying
   on the model.
3. Re-run with three repeats and the long transcripts before trusting any cell
   that is close. The Tier B CJK cell is the one worth the compute.
4. **Re-run Tier B after the hygiene fix.** It is the one cell this run leaves
   genuinely open: `gemma3:1b` lost on defects the pipeline no longer lets
   through, and already wins recall in three of five groups under matched
   sampling. Twelve generations settles it.
5. Leave `language_score` alone until then. The run id is in a comment there so
   the next person knows it was checked rather than assumed.
