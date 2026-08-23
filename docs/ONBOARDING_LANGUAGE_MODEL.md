# Onboarding: asking what language the meetings are in

A design for asking new users one question during setup — what language most of
their meetings are in — and letting that answer, together with available RAM,
choose the transcription engine and the summarization model.

Status: implemented 16 August 2026 and completed 19 August. The bake-off in §4.3
has run, so language now influences the summary model; §4.3 records what it
found. Written against `4a85476`, revised at `6ff1a49`.

The implementation departed from this design in two places, both noted where
they arise: the RAM floors live with the recommendation logic rather than in
`models.rs` (§7), and the summary-language picker was left as its own list
rather than merged (§5).

The question turned out to matter far more than it looked. What began as "pick a
better summary model" ended up uncovering a defect that silently breaks the
product for a large part of the world, described in §1. That defect, not the
model recommendation, is now the reason to build this step.

## 1. The problem this found

**Onboarding installs a transcription engine that cannot transcribe most of the
languages the app offers, and never asks.**

The chain, each link verified in the code:

- `onboarding.rs:165` hardcodes the transcription provider, with the comment
  `// Save transcription model config (parakeet provider) - always parakeet`.
  Whisper is not mentioned anywhere in `onboarding.rs`, and
  `DownloadProgressStep.tsx` downloads exactly two things: Parakeet and a
  summary model.
- The model is `parakeet-tdt-0.6b-v3-int8` (`config.rs:12`).
- Parakeet TDT 0.6B v3 supports **25 European languages** and detects among them
  automatically ([model card](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3)):
  Bulgarian, Croatian, Czech, Danish, Dutch, English, Estonian, Finnish, French,
  German, Greek, Hungarian, Italian, Latvian, Lithuanian, Maltese, Polish,
  Portuguese, Romanian, Slovak, Slovenian, Spanish, Swedish, Russian, Ukrainian.
- `parakeet_provider.rs:29` confirms the engine takes no language input. Given
  one, it logs `Parakeet doesn't support language preference '{}' yet` and
  transcribes anyway.

So a user whose meetings are in Japanese, Chinese, Korean, Arabic, Turkish,
Hindi, Hebrew, Thai, Vietnamese or Indonesian finishes onboarding, records a
meeting, and gets nonsense — from an engine that was never capable of the job
and was installed without a question being asked.

What the app does *not* do is claim otherwise. All three language pickers —
Settings, import, re-transcribe — already collapse to "Auto Detect" when a
Parakeet model is selected, because Parakeet takes no language argument. The
interface never lists Japanese as supported and then fails at it.

That makes the failure quieter rather than milder. The user is shown no list,
given no way to say that their meetings are in Japanese, and left with
transcripts that are simply wrong. Nothing in the product connects the bad
output to its cause, and the one place that could have asked — onboarding —
does not.

Three language lists exist, and none encodes engine coverage:

| List | Entries | Used by |
|---|---|---|
| `LanguageSelection.tsx` (inline) | 101 | the Settings transcription picker |
| `constants/languages.ts` | 37 | import and re-transcribe dialogs |
| `lib/summary-languages.ts` | 32 | the summary language picker |

The 37-entry list is a truncated prefix of the 101-entry one — the first 37
codes match exactly, so it is a stale copy rather than a deliberate subset,
which left the import and re-transcribe dialogs offering Whisper users about a
third of the languages Whisper can actually handle.

This is why the language question belongs in onboarding. Its primary job is not
choosing a summary model — it is choosing an engine that can do the work at all.

## 2. What the user is asked

One question, once, inserted as step 2:

```
1 Welcome  →  2 Meeting language  →  3 Setup overview  →  4 Download  →  5 Permissions (macOS)
```

> **What language are most of your meetings in?**
> This decides which speech recognition model we install, so it is worth getting
> right. You can change it later in Settings.
>
> `[ English ▾ ]`  ·  ○ My meetings are in several languages

It must come before the overview, because the overview announces what will be
downloaded and how large it is, and after this change both depend on the answer.

Decisions behind the wording:

- **"Most of your meetings", not "your language".** The interface language, the
  OS locale and the meeting language are three different things. Someone in
  Kraków whose standups are in English is exactly the user a locale-sniffing
  shortcut gets wrong.
- **Pre-select from the OS locale, but ask anyway.** The locale is a good guess
  and a bad assumption. Pre-filling removes the effort for the majority without
  silently deciding for the minority.
- **Say what the answer controls.** An unexplained language dropdown during
  setup reads as a UI-language picker and will be answered as one.
- **An explicit multi-language option.** Bilingual teams are common, and forcing
  them to name one language would record something false.

## 3. What the answer decides

| Decision | Today | After |
|---|---|---|
| Transcription engine | always Parakeet, unasked | Parakeet if the language is among its 25, otherwise Whisper |
| Summarization model | RAM only, always Qwen | RAM ceiling, then language ranking (§4.2) |
| Default summary language | unset per meeting; detected from the transcript | seeded with the answer |
| Whisper language hint | `auto` from the UI | seeded with the answer, and only meaningful once Whisper is the engine |

The first row is the one that changes correctness rather than quality, and it is
implementable immediately — the language list is published, both engines already
exist in the codebase, and `save_transcript_config` already takes a provider
argument. It does not wait on any measurement.

## 4. How the recommendation is made

### 4.1 Transcription engine — a capability test

```
engine = if language ∈ PARAKEET_V3_LANGUAGES { Parakeet } else { Whisper }
```

For "several languages": Parakeet if *every* named language is in its set —
because it detects among them automatically and needs no hint — and Whisper
otherwise. Mixed European meetings are Parakeet's best case; a
Japanese-and-English team must have Whisper.

This changes the download size, which is why the question precedes the overview.

### 4.2 Summarization model — a ceiling and an ordering

RAM and language are different kinds of constraint and must not be blended into
one score.

- **RAM is a ceiling.** A model that does not fit cannot be recommended at any
  quality. This filters.
- **Language is an ordering.** Among the models that fit, some summarise a given
  language better than others. This ranks.

```
candidates = models.filter(|m| m.min_ram_gb <= system_ram_gb)
best       = candidates.max_by_key(|m| language_score(m, language_group))
```

Ties break toward the smaller download, so that when two models score equally
the user waits for less.

### 4.3 The data §4.2 needed, and what measuring it found

This section used to say the table **does not exist yet and must not be
invented**. It was measured on 19 August 2026 — the design, corpus, prompts,
metrics and results are in
[`docs/experiments/summary-model-bakeoff/`](experiments/summary-model-bakeoff/README.md),
and the run is in
[`results/REPORT.md`](experiments/summary-model-bakeoff/results/REPORT.md).

Five languages covering every group, four models, two sampling arms, 52
generations. Qwen led recall in every group in both tiers, so **the inherited
order survived almost everywhere** and the honest headline is "measured,
unchanged".

Exactly one cell moved, and it is worth stating because it is the kind of thing
this design predicted would exist: `qwen3.5:2b` summarises Japanese into
Chinese. The pipeline's first pass demands English and it produces neither,
reproducibly across three byte-identical greedy runs, so everything downstream
is built on a corrupt intermediate. `language_score` now scores it below
`gemma3:1b` for `Cjk`, which is a choice between two weak options rather than an
endorsement — the report says plainly that neither small model is fit for CJK.

Two caveats that the report states at greater length. The run was one transcript
per language with one repeat outside the CJK re-check, no long transcripts and
no real meetings: a direction rather than a measurement. And the small tier is
**re-opened by our own fix** — `gemma3:1b` lost partly on output-hygiene defects
that `clean_llm_markdown_output` now strips.

### 4.4 What shipped before the table existed

Recorded because the sequencing was deliberate and worked. The step shipped
without the bake-off, and most of its value never depended on it:

- The **engine choice (§4.1) was fully determined from the start** — the part
  that fixes §1.
- The summary language default is seeded at the same moment.
- The model recommendation fell back to the RAM-only rule, so nothing regressed
  while the measurement was outstanding.

Neither change blocked the other, which is why both exist.

## 5. Two smaller defects found along the way

**The transcription-language default disagrees between Rust and the frontend.**
`lib.rs:71` initialises `LANGUAGE_PREFERENCE` to `"auto-translate"`, which
whisper maps to `set_translate(true)` — *translate everything into English*.
`ConfigContext.tsx:140` defaults `selectedLanguage` to `'auto'`, which does not
translate. A mount effect syncs the frontend value down, so in practice the
frontend wins before any recording starts and the Rust default is unreachable;
but nothing enforces that ordering, and the two disagree about something as
consequential as whether the user's words are silently rendered into another
language. The defaults should match, and `auto` is the correct one.

Both defaults now say `auto`.

**Three language lists, none authoritative.** The 101/37/32 split in §1 had no
single source. Adding a fourth list for Parakeet's 25 would have made it worse,
so the engine's supported set lives beside the engine definition in Rust and the
frontend reads it through `parakeet_supported_languages` rather than restating
it.

The two transcription lists are now one. The 32 summary languages were
deliberately **not** merged into it: they answer a different question — what a
summary can be written in, not what speech can be recognised — and collapsing
them would tie two sets that have no reason to move together.

A third defect surfaced only during implementation, and would have broken this
feature outright. The transcript config spells the Whisper provider
`localWhisper`, not `whisper`; `audio::transcription::engine` matches that exact
string, so writing `whisper` at onboarding would have passed setup and failed at
the first recording. Both spellings are now constants shared by the writer and
the matcher.

## 6. Edge cases

- **"Several languages"** → engine per §4.1; summary model ranked on the average
  across groups; summary language and Whisper hint left on automatic, which is
  the correct answer for that user rather than a degraded one.
- **User skips onboarding** → treated as "several languages". Since that yields
  Whisper unless everything is European, skipping is safe rather than silently
  wrong.
- **Answer changes later** → **not built.** Settings has no copy of this
  question; what it has instead is a note under the transcription language
  saying which languages the running engine covers and that Whisper covers
  more, so a user who picked wrongly can see why and switch the model by hand.
  A control that re-runs both decisions and offers the download is the obvious
  follow-up, and is the right place to also re-ask the users who completed
  onboarding before this question existed.
- **The needed engine is already downloaded** → use it, no second download.
- **RAM below every summary model's floor** → recommend the smallest and say
  plainly that summaries will be slow, rather than recommending nothing.
- **Language outside every list** → falls to Whisper, which covers 101, and to
  the "other" summary group.

## 7. What was built

Rust:
- `language.rs` — new. Parakeet's 25 codes, `parakeet_supports`, `choose_engine`,
  the language groups, and the `PROVIDER_*` constants that the transcript config
  and the live-recording dispatch now share.
- `onboarding.rs` — `complete_onboarding` takes the languages and derives the
  provider instead of hardcoding `"parakeet"`; `plan_for_languages` answers what
  an answer installs and what it weighs; the answer is stored on
  `OnboardingStatus`.
- `summary/summary_engine/commands.rs` — `recommend_summary_model` filters on a
  per-model RAM floor and ranks what survives; `_is_macos` is gone.
- `audio/transcription/engine.rs` — `transcription_model_available` and
  `transcription_model_downloading`, which follow the configured provider.
- `lib.rs` — `LANGUAGE_PREFERENCE` defaults to `auto` (§5).

Deviation from §4.2: the RAM floors and language scores sit in `commands.rs`
beside the recommendation rather than on `ModelDef`. `models.rs` is pristine
upstream with a single vendor commit, and keeping it that way keeps it
mergeable; a test requiring every shipped model to declare a floor closes the
drift risk that motivated putting the data on the model in the first place.

Frontend:
- `MeetingLanguageStep.tsx` — new, registered as step 2 of six.
- `SummaryModelStep.tsx` — new, step 3 of six. The answer above no longer only
  picks a model silently; it ranks a list the user can see and override.
- `OnboardingContext` — Parakeet-specific state generalised to transcription
  state, both engines' download events handled, the answer persisted.
- `SetupOverviewStep` / `DownloadProgressStep` — engine and sizes come from the
  plan rather than a hardcoded model and `~670 MB`.
- `constants/languages.ts` — the single transcription list; `LanguageSelection`
  imports it instead of holding a second copy.
- `useRecordingStart` — the pre-flight check follows the configured engine.
- `LanguageSelection` — the Parakeet note now names the covered languages and
  the way out.

## 8. Testability

The decisions are pure functions and belong in class A of the
[coverage audit](../frontend/src-tauri/TEST_COVERAGE_AUDIT.md):

- `parakeet_supports` — all 25 in, a sample of the excluded out, unknown codes
  out.
- `choose_engine(language)` and the multi-language variant — European-only picks
  Parakeet, any non-European member forces Whisper.
- `recommend_summary_model(ram, group)` — the ceiling excludes what does not
  fit, ranking picks the best of the rest, ties break toward the smaller
  download, and the below-every-floor case still returns a model.
- The plumbing: an answer seeds engine, model and summary language; "several
  languages" seeds no single language; skipping changes nothing.

None of these need a model, a download, or an `AppHandle`.

## 9. Launch relevance

[LAUNCH_READINESS.md](../LAUNCH_READINESS.md) §4 lists "set the default model
empirically across several languages" as pre-launch item 4. This document
supersedes the framing of that item: the model default is a quality question and
can wait for measurement, but §1 is a correctness question and cannot. A launch
that reaches an audience outside Europe currently ships them an engine that
cannot transcribe their meetings, with no warning at any point in the product.
