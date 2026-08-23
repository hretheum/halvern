# 03 — Prompts

Every prompt used anywhere in the experiment, with the model it runs against and
the rule about what may be shown to it.

## Data handling rule, before anything else

**P1 and P3 run against a cloud model. They may only ever see synthetic
material.**

The real-meeting control set (`corpus/_control/`) contains actual recordings of
actual people. It is scored exclusively by local, programmatic metrics — never
by P3, never by any hosted model. Sending it to a judge to grade a summarizer
would break the product's central claim in the course of measuring the product's
quality, which is not a trade worth making for a table of integers.

`scripts/score.py` refuses to run the judge on any file under `_control/`, so
this is enforced rather than remembered.

---

## P0 — Pre-flight: is the chat template right?

**Model:** each of the four built-in models, locally, via `summary_bench`
**Runs:** once per model, before anything else
**Purpose:** rule out our own prompt assembly as the cause of any difference

Qwen and Gemma use different chat templates (`qwen3.5_nonthinking` versus
`gemma3` with `<start_of_turn>` markers). If either is assembled wrongly, the
experiment measures our bug and blames the model.

```
Reply with exactly the following three lines and nothing else:

LINE ONE
LINE TWO
LINE THREE
```

**Pass:** the model returns three lines, no preamble, no chat-template markers
leaking into the output, no repetition of the instruction.

**If a model fails P0, stop.** Fix the template handling first. A model that
cannot follow a three-line instruction is not telling you anything about
Japanese meeting summaries.

Second probe, same conditions, checking the structured path:

```
Return a Markdown table with exactly two rows and these columns:
| Name | Role |
Use the values (Ada, engineer) and (Bo, designer). Return only the table.
```

**Pass:** a well-formed Markdown table, pipes intact, no commentary.

---

## P1 — Corpus generation

**Model:** Claude Opus 5 (`claude-opus-5`)
**Runs:** 36 times — once per transcript, native in each language
**Input:** the slot parameters below
**Output:** one JSON file conforming to [02-corpus.md](02-corpus.md)

Do not generate English and translate. Generate natively in the target
language; translationese is the exact artefact this corpus exists to avoid.

Run each generation in a fresh context. Generating six Polish transcripts in one
session produces six variations of the same meeting, and the corpus needs
independent samples.

````
You are writing test material for an experiment that measures how well small
local language models summarize meetings. The material must be realistic enough
to be a fair test and constructed precisely enough to be scored automatically.

Write ONE meeting transcript with these parameters:

  language:      {{LANGUAGE_NAME}} ({{LANGUAGE_CODE}}) — write natively, do not
                 translate from English. Idiom, names, date formats, currency
                 and register must be what a real meeting in this language
                 sounds like.
  length:        {{LENGTH_CLASS}} — {{TOKEN_BAND}} tokens of transcript text
  domain:        {{DOMAIN}}
  participants:  4, named in a way that is natural for this language.
                 No name may be a substring of another.

The transcript is spoken, not written. Include interruptions, at least one false
start, one person correcting another, and natural filler. Turn lengths vary from
one sentence to several.

CONSTRUCTION REQUIREMENTS — the experiment depends on every one of these:

1. Plant exactly 12 checkable facts:
     - 3 decisions actually settled in the meeting
     - 4 action items, each with an owner named aloud, a task, and a due date
       stated in the conversation
     - 3 figures: a number with a unit (cost, count, percentage)
     - 2 explicit dates or named deadlines
   Each must be genuinely recoverable by a careful reader from the transcript
   alone.

2. Plant exactly 4 decoys: topics raised and explicitly NOT settled. Each needs
   an unmistakable cancellation cue spoken aloud — "let's park that", "we decide
   next week", "nobody owns that yet" — rendered naturally in {{LANGUAGE_NAME}}.
   A decoy must never acquire an owner or a due date.

3. Plant distractors: at least 2 numbers and 2 person-names that are mentioned
   but belong to nothing — a figure quoted from a previous quarter, someone
   absent who is merely referred to. These exist to catch a model that harvests
   digits and capitals into a table.

4. Include exactly one REVERSAL: a decision made early and explicitly changed
   later. The later one is correct. Make the change unambiguous in the dialogue.

5. Timestamps in seconds, monotonically increasing, 2–20 seconds per turn.

Return ONLY a JSON object, no prose around it, in exactly this shape:

```json
{
  "id": "{{ID}}",
  "language": "{{LANGUAGE_CODE}}",
  "group": "{{GROUP}}",
  "length_class": "{{LENGTH_CLASS}}",
  "template_id": "standard_meeting",
  "participants": ["...", "...", "...", "..."],
  "transcript": [{ "t": 0.0, "speaker": "...", "text": "..." }],
  "ground_truth": {
    "facts": [
      { "id": "F1", "kind": "decision|action|figure|date",
        "canonical": "one sentence in ENGLISH stating the fact",
        "paraphrases": ["2-4 other ways an English summary might phrase it"],
        "owner": "participant name or null",
        "due": "YYYY-MM-DD or null" }
    ],
    "decoys": [
      { "id": "D1", "claim": "in ENGLISH",
        "cancellation_cue": "the exact words from the transcript, in {{LANGUAGE_NAME}}",
        "must_not_appear_as": ["decision", "action"] }
    ],
    "distractors": { "numbers": ["..."], "names": ["..."] },
    "reversal": { "initial": "in ENGLISH", "final": "in ENGLISH",
                  "correct_answer": "final" }
  }
}
```

Note the asymmetry and keep it: the TRANSCRIPT is in {{LANGUAGE_NAME}}, but
`canonical`, `paraphrases`, `claim` and the reversal fields are in ENGLISH —
they are matched against the summarizer's English intermediate output.
````

### Slot parameters

Domains are varied so the corpus is not six versions of one meeting.

| ID | Lang | Group | Length | Tokens | Domain |
|---|---|---|---|---|---|
| `{L}-short-01` | each | — | short | 2000–3000 | Sprint planning for a mobile release |
| `{L}-short-02` | each | — | short | 2000–3000 | Budget review with a vendor decision |
| `{L}-short-03` | each | — | short | 2500–3500 | Hiring debrief for two candidates |
| `{L}-short-04` | each | — | short | 2500–3500 | Incident post-mortem |
| `{L}-long-01` | each | — | long | 6000–7500 | Quarterly planning, three workstreams |
| `{L}-long-02` | each | — | long | 7500–9000 | Client onboarding covering scope, price and timeline |

`{L}` ∈ `en`, `de`, `es`, `pl`, `ja`, `tr`. Groups per
[02-corpus.md](02-corpus.md).

---

## P2 — The prompts under test (do not author these)

The summarization prompts are **not written for this experiment**. They are what
the application already sends, and changing them would measure a different
product. They live in `summary/processor.rs`:

- the per-chunk prompt (line ~139), prefixed with
  `ENGLISH_BASE_SUMMARY_INSTRUCTION`
- the combine prompt (line ~145), same prefix
- the final structured prompt (line ~223), which carries the template sections
- `english_normalization_system_prompt()` for the English-normalization pass
- `translation_system_prompt(target)` plus the user prompt in
  `translate_markdown` for the translation pass

`summary_bench` calls `generate_meeting_summary` directly, so these are used
verbatim. **If any of them is edited, every prior result is void** — record the
commit SHA of `processor.rs` with each run, which `scripts/run.sh` does
automatically.

---

## P3 — Judge, for the residue only

**Model:** Claude Opus 5 (`claude-opus-5`)
**Runs:** only on fact-recall rows the programmatic matcher marks `UNCERTAIN`,
and only on synthetic material
**Expected volume:** 10–20% of rows

Most recall scoring is settled by matching against `canonical` plus
`paraphrases`. The judge exists for the cases where the model said the right
thing in a way the paraphrase list did not anticipate. It does not produce a
holistic quality score — a judge asked "which summary is better" mostly measures
its own stylistic preferences, and would give this experiment a number nobody
could act on.

```
You are checking whether a meeting summary captured one specific fact. Answer
mechanically; do not assess overall quality.

FACT THAT WAS IN THE MEETING:
{{CANONICAL}}

ACCEPTABLE REPHRASINGS ALREADY CONSIDERED:
{{PARAPHRASES}}

THE SUMMARY THAT WAS PRODUCED:
{{SUMMARY_MARKDOWN}}

Decide whether the summary states this fact. It counts as stated if the meaning
is present and correct, in any wording, in any section.

It does NOT count if:
  - the summary states the fact but attributes it to the wrong person
  - the summary states a different number, date, or outcome
  - the fact appears only as an open question rather than as settled
  - you have to assume something the summary does not say

Return only JSON:
{"present": true|false, "evidence": "the exact sentence from the summary, or null",
 "note": "one short clause, only if the call was close"}
```

### P3b — Fabrication adjudication

Same conditions, for decoys the matcher flags as possibly asserted.

```
A meeting discussed a topic and explicitly did NOT settle it. Check whether the
summary wrongly presents it as settled.

TOPIC LEFT OPEN:
{{DECOY_CLAIM}}

WHAT WAS SAID TO LEAVE IT OPEN:
{{CANCELLATION_CUE}}

THE SUMMARY:
{{SUMMARY_MARKDOWN}}

Mentioning the topic is fine and often correct. It is a fabrication ONLY if the
summary presents it as a decision that was made, or assigns it an owner or a due
date, or otherwise implies it was resolved.

Return only JSON:
{"fabricated": true|false, "evidence": "the exact sentence, or null"}
```

---

## P4 — Reversal check

**Model:** programmatic first, P3 for uncertain rows
**Purpose:** did the summary report the final decision or the superseded one?

Handled by the same matcher: `reversal.final` should be present,
`reversal.initial` absent. A summary containing both without marking the change
counts as `initial` — reporting a superseded decision alongside the real one is
the same failure with extra words.

---

## Prompt inventory

| Prompt | Model | Sees real meetings? | Frequency |
|---|---|---|---|
| P0 template probe | the 4 local models | no | 4 runs, once |
| P1 corpus generation | Claude Opus 5 | no | 36 runs, once |
| P2 the prompts under test | the 4 local models | yes (control set) | every generation |
| P3 fact adjudication | Claude Opus 5 | **never** | 10–20% of rows |
| P3b fabrication adjudication | Claude Opus 5 | **never** | flagged rows only |
