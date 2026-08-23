# 02 — Corpus

## Why synthetic, and what it buys

There are 16 real meetings in the development database, all Polish. Nothing in
German, Spanish, Japanese or Turkish.

Three ways to get a multilingual corpus, and only one of them is cheap to score:

- **Translate the Polish meetings.** Introduces translationese; you end up
  measuring the translator's register rather than the model's comprehension.
- **Public corpora** (VoxPopuli, Europarl). Real speech, wrong genre —
  parliamentary rhetoric is not four people deciding who owns a deadline.
- **Write the transcripts ourselves.** Synthetic, but you *know what is in
  them*, which turns faithfulness and coverage from a reading exercise into an
  arithmetic one.

The third is the backbone, precisely because of that property. The real Polish
meetings are kept as a **control set**: if the synthetic result and the real one
disagree for Slavic, the synthetic corpus is unrepresentative and the finding is
void.

## Coverage

Six language slots covering all five `LanguageGroup` variants:

| Slot | Code | Group | Why this one |
|---|---|---|---|
| English | `en` | `English` | The baseline, and the pipeline's internal language |
| German | `de` | `WesternEuropean` | Compound morphology; long nominal phrases stress the summarizer |
| Spanish | `es` | `WesternEuropean` | Second sample so the group is not one language wide |
| Polish | `pl` | `Slavic` | The only group with a real-meeting control set |
| Japanese | `ja` | `Cjk` | Non-Latin script; the hard case for structure preservation |
| Turkish | `tr` | `Other` | Agglutinative, outside Parakeet's 25, genuinely "other" |

## Size and shape

Per language slot:

- **4 short transcripts** — under 3,500 tokens, so `generate_meeting_summary`
  takes the single-pass branch (`token_threshold` default 4000).
- **2 long transcripts** — 6,000–9,000 tokens, forcing the map/reduce branch
  with 2–3 chunks.

That is **36 transcripts** (6 slots × 6). Combined with 4 model configurations
(2 tiers × 2 models), 2 arms, and 3 repeats:

```
36 transcripts × 4 models × 2 arms × 3 repeats = 864 generations
```

At 20–60 s per generation on Apple Silicon that is roughly 5–14 hours: one
overnight run, or two if Arm S is run separately.

**If that is too much for a first pass**, cut to Arm M only and 1 repeat
(144 generations, under two hours) to see whether any signal exists at all, then
run the full protocol on the tiers that showed movement. Record the reduction —
`05-runbook.md` §6 has the shortened variant.

## What each transcript must contain

Every transcript is written to a fixed recipe so that scoring is mechanical.

**Cast.** 4 named participants with unambiguous, non-interchangeable names,
localized to the language (a Japanese transcript uses Japanese names). Names
must not appear in each other's substrings.

**Planted facts — 12 per transcript.** Each is a discrete, checkable
proposition, tagged by kind:

| Kind | Count | Example shape |
|---|---|---|
| `decision` | 3 | "We are going with the Postgres option." |
| `action` | 4 | Owner + task + due date, all three stated aloud |
| `figure` | 3 | A number with a unit: a cost, a count, a percentage |
| `date` | 2 | An explicit calendar date or a named deadline |

Each planted fact carries an `id`, the surface form as spoken, and 2–4
**paraphrases** that should also count as a hit. Paraphrases are what make
recall scoring survive the model's rewording without collapsing into a judge
call for every row.

**Decoys — 4 per transcript.** Things raised and explicitly *not* settled. The
transcript must contain an unmistakable cancellation cue ("let's park that",
"we'll decide next week", "nobody owns that yet"). A summary that reports a
decoy as a decision or assigns it an owner is fabricating, and that is the
metric that matters most for trust.

**Distractors.** At least 2 near-miss numbers and 2 near-miss names that are
mentioned but belong to nothing — a figure quoted from last quarter, a person
who is absent and merely referenced. These catch a model that pattern-matches
digits and capitals into the Action Items table.

**Register.** Spoken, not written. Interruptions, false starts, one person
correcting another, at least one place where a decision is reversed later in the
meeting. **The reversal is deliberate**: the correct summary reports the final
decision, and a model that reports the first one has failed a comprehension test
that no amount of fluent output disguises.

## File format

One JSON file per transcript, in `corpus/<lang>/<id>.json`:

```json
{
  "id": "pl-short-01",
  "language": "pl",
  "group": "Slavic",
  "length_class": "short",
  "template_id": "standard_meeting",
  "participants": ["Anna Kowalska", "Marek Wiśniewski", "Ola Dąbrowska", "Piotr Zieliński"],
  "transcript": [
    { "t": 0.0,  "speaker": "Anna Kowalska",     "text": "..." },
    { "t": 7.4,  "speaker": "Marek Wiśniewski",  "text": "..." }
  ],
  "ground_truth": {
    "facts": [
      {
        "id": "F1",
        "kind": "decision",
        "canonical": "The team chose Postgres over MySQL",
        "paraphrases": ["went with Postgres", "Postgres was selected", "decided on PostgreSQL"],
        "owner": null,
        "due": null
      },
      {
        "id": "F4",
        "kind": "action",
        "canonical": "Marek prepares the migration script by 12 September",
        "paraphrases": ["Marek will write the migration", "migration script owned by Marek"],
        "owner": "Marek Wiśniewski",
        "due": "2026-09-12"
      }
    ],
    "decoys": [
      {
        "id": "D1",
        "claim": "Moving the analytics stack to the new cluster",
        "cancellation_cue": "we'll decide that next week",
        "must_not_appear_as": ["decision", "action"]
      }
    ],
    "distractors": {
      "numbers": ["47%", "12 400 EUR"],
      "names": ["Tomasz Lis"]
    },
    "reversal": {
      "initial": "Ship on the 5th",
      "final": "Ship on the 12th",
      "correct_answer": "final"
    }
  }
}
```

Timestamps must be monotonic and plausible (2–20 s per turn); the
`standard_meeting` template asks for a "Segment Time stamp" column, so absent or
nonsensical timestamps would penalise every model equally and for the wrong
reason.

## The real-meeting control set

**This set never leaves the machine and is never sent to any judge model.**

Six of the 16 real Polish meetings, chosen for length spread. They have no
planted ground truth, so they are scored only on the metrics that need none:
structure preservation, template compliance, and output language. Their job is
to answer one question — does the ranking obtained on synthetic Polish hold on
real Polish? — and nothing else.

Extract them with `scripts/extract-control-set.sh`, which writes to
`corpus/_control/` (git-ignored).

## Generation and validation

Transcripts are generated by a frontier model, **natively in each language, not
translated** — see [03-prompts.md](03-prompts.md) P1. Generating German by
translating English would reintroduce exactly the translationese the synthetic
approach exists to avoid.

Every file must pass `scripts/validate_corpus.py` before any run:

- schema and required fields present
- 12 facts with the declared kind distribution, 4 decoys, ≥2+2 distractors
- every fact's `canonical` and every paraphrase is non-empty and distinct
- every planted fact is actually **traceable in the transcript text** — a fact
  nobody says is a scoring error waiting to happen
- every decoy's cancellation cue appears verbatim in the transcript
- participant names do not appear as substrings of one another
- token count lands in the band for its `length_class`
- timestamps monotonic

The validator is the gate. A corpus that has not passed it produces numbers that
cannot be defended.
