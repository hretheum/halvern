# Launch Readiness

What has to be true before this app is put in front of strangers, what is
already true, and what is deliberately being left for later. Written against a
Product Hunt-style launch: a burst of non-technical first-time users, on varied
hardware, who will judge the product in its first three minutes and say so in
public.

First assessed 16 August 2026 at `bee52df`, revised 18 August after the rebrand
reached the data paths and 19 August after the summary-model bake-off. Every
claim below was checked against the code rather than assumed; where a claim
could not be verified, it says so.

The product is aimed at a global audience. That is worth stating because parts
of this codebase were inherited from a fork made for one person's own use, and
several inherited defaults — the built-in model lineup most of all — were never
chosen with a worldwide user base in mind.

## 1. Hard blockers

These make the launch fail rather than underperform. **All three are now
closed** — signing and notarization on 19 August, the rebrand and its data
migration on 18 August, the speech-engine choice on 16 August. What remains
below §1 is verification and polish, not blockage.

### 1.1 The macOS build is signed and notarized — done 19 August 2026

**Closed, and verified where it counts.** Downloaded onto a Mac that had never
run it, the `.dmg` opens and the app launches with no developer warning — which
is the only check that proves anything, because Gatekeeper does not quarantine
what was built locally and a broken signature looks perfect on the machine that
produced it.

`Halvern_0.4.0_aarch64.dmg` and the app inside it both verify as
`accepted / source=Notarized Developer ID`, signed by a Developer ID Application
certificate on team `QT32CS5UEH`, with the notarization ticket stapled to each
so the check works offline. Hardened runtime is on, the signature passes
`--deep --strict`, and the chain reaches Apple Root CA.

One thing had to be done by hand and will have to be done by hand every release
until `release.yml` does it: **Tauri notarizes the app and not the disk image.**
The same build produced an app that Gatekeeper accepted and a `.dmg` it
rejected as `Unnotarized Developer ID`. The fix is a `notarytool submit` and a
`stapler staple` on the `.dmg` afterwards, which rebuilds nothing —
[docs/SIGNING.md](docs/SIGNING.md) §3 has it. It is the kind of gap that ships
quietly, because the artefact you test locally is the app and the artefact you
upload is the image.

The certificate expires **1 February 2027**, which tracks the membership rather
than the usual five-year certificate life. Builds signed before then keep
working afterwards — that is what the secure timestamp in the signature is for —
but no new release can be signed until it is renewed.

What it looked like before: the log ended with `Warn skipping app notarization`
and every binary carried the ad-hoc identity, so a downloader met Gatekeeper
saying the developer could not be verified, immediately before the app asked for
microphone *and* screen recording.

For a product whose entire pitch is trust, this is the worst possible first
minute. It is also the item with external lead time: Apple Developer Program
enrollment ($99/year) can take a day or more to clear, so it should be started
before anything else on this list.

**First, a decision that changes the rest of the work: direct download or the
Mac App Store.** They are not two ways of shipping the same build.

Direct download needs a *Developer ID Application* certificate, notarization,
and stapling. It is what everything below assumes, and what the codebase is
already shaped for.

The Mac App Store needs a *Mac App Distribution* certificate and, more to the
point, the App Sandbox — which this app currently does not enable, and which
would have to be reconciled with capturing system audio through
ScreenCaptureKit, writing recordings into `~/Movies`, running the `llama-helper`
sidecar, and downloading gigabytes of models on first launch. Each of those is
solvable or fatal depending on the entitlement Apple grants, and none of it is
in place. Treat the App Store as a separate project, not a build flag.

**What is already correct.** `hardenedRuntime` is on, which notarization
requires. `entitlements.plist` covers microphone, audio input/output, screen
capture and calendars. `Info.plist` carries the usage strings, including both
calendar keys — macOS 13 asks with one and macOS 14+ with the other, and missing
either is a silent denial. There is no `com.apple.security.app-sandbox` key,
which is right for direct download and is exactly what the App Store route would
have to change.

**Fixed 19 August:** `tauri.conf.json` no longer hardcodes
`"signingIdentity": "-"`. With the key absent Tauri reads
`APPLE_SIGNING_IDENTITY` from the environment, which keeps the certificate name
out of a public repository and leaves an unsigned local build working. The old
value meant every credential in the environment was ignored and the build signed
itself ad-hoc, announcing it in a log line nobody read.

**What is left, and what only you can do.** The Apple Developer account exists
as of 19 August; the certificate does not. `security find-identity` shows an
*Apple Development* certificate, which is for running on your own devices —
distribution needs a *Developer ID Application* certificate, and creating one
requires signing into the account. Xcode 26.6 and `notarytool` are installed, so
nothing else is missing locally.

[docs/SIGNING.md](docs/SIGNING.md) is the procedure, in order, marking which
steps are a human in a browser and which are a build command.

`.github/workflows/release.yml` contains no macOS signing or notarization step
at all. It will need the certificate imported into a temporary keychain and
`APPLE_ID`, `APPLE_PASSWORD` (an app-specific password, not the account one),
`APPLE_TEAM_ID`, `APPLE_CERTIFICATE` and `APPLE_CERTIFICATE_PASSWORD` as
repository secrets. The `sign-binaries: true` already in that file is the
Windows path and does nothing for macOS.

**Verify on another Mac, not this one.** Gatekeeper does not quarantine what was
built locally, so a signed build will always look fine on the machine that made
it. The check that matters is downloading the `.dmg` over the network onto a Mac
that has never seen the app, and confirming `spctl -a -vvv` accepts it and the
first launch asks for microphone and screen recording without a developer
warning first.

### 1.2 The rebrand is executed, data paths included

Done, 17–18 August 2026: the gatehouse mark ships in the toolbar, the About
panel and the whole platform icon set, generated from
`design/brand/app-icon.svg`. Every user-visible string reads Halvern, as do the
product name, window title, npm package, crate and favicon. The README
describes this product and states the fork attribution plainly instead of being
upstream's document with our code beneath it, and upstream's sales funnel is
out of the app while the credit stays.

Done, 18 August 2026: the data paths too. The bundle identifier is now
`io.halvern.app`, the brand folder is `Halvern`, recordings save to
`halvern-recordings`, logs and the log file follow the identifier, and the build
variables are `HALVERN_*`. Two things were found while doing it rather than
before. The console helper still filtered `log stream --process meetily`, a name
that stopped existing when the crate was renamed, so the log window had been
showing nothing. And the brand folder was written as `Meetily` by the template
loader and `meetily` by the notification settings — one folder on macOS and
Windows, two on Linux, with half the user's settings in the wrong one. Both now
resolve through a single `APP_DATA_DIR_NAME` constant.

Moving existing data is `scripts/migrate-meetily-to-halvern.sh`, which reports
what it would do and writes nothing until `--apply`. It renames the four
directories in place, and treats the recordings folder as one operation with the
database and `recording_preferences.json`, because every meeting stores an
absolute `folder_path` and renaming the folder alone would leave all of them
pointing at nothing. It backs the database up first, refuses to run while the
app is open, sets an already-created empty destination aside instead of merging
into it, deletes nothing, and does not touch `pro.meetily.ai` — that is
upstream's separate application, not ours to adopt.

Deliberately a script rather than a first-run migration in the app. Shipping one
would mean every published build carries code that reads `com.meetily.ai`, and
would silently adopt the library of anyone who had run upstream's Meetily —
taking data from a different product without asking. If importing from Meetily
becomes worth supporting, it belongs in Settings as an explicit action with a
visible source, not in startup.

The repository is still `hretheum/meetily`. That resolves itself at launch: the
plan is a fresh repository without this history, so nothing needs renaming
first.

Two smaller open items, both recorded rather than done. Cargo's `authors` field
still names the upstream author, which is an attribution decision rather than
metadata drift. And onboarding no longer offers a "report issues" link at all,
because pointing Halvern's bugs at upstream's tracker was worse than silence —
it needs a destination once one exists.

### 1.3 The installed speech engine could not transcribe most of the world's languages — fixed, unverified

Onboarding hardcoded Parakeet as the transcription engine (`onboarding.rs`,
`// always parakeet`) and never downloaded Whisper. Parakeet TDT 0.6B v3 covers
25 European languages. A user whose meetings were in Japanese, Chinese, Korean,
Arabic, Turkish, Hindi, Hebrew, Thai, Vietnamese or Indonesian therefore got an
engine that could not do the job, with no question asked and no warning shown.

The interface did not compound this by claiming otherwise — every language
picker collapses to "Auto Detect" while Parakeet is active, because Parakeet
takes no language argument. That made the failure quieter, not milder: those
users were given no way to say what language they speak, and nothing connected
their unusable transcripts to the cause.

For a product this document opens by calling global, that was disqualifying
rather than merely disappointing, and it was unconditional: those users did not
get a degraded result, they got an unusable one.

Onboarding now asks, as step 2 of six, and installs Whisper whenever Parakeet
cannot serve the answer
([docs/ONBOARDING_LANGUAGE_MODEL.md](docs/ONBOARDING_LANGUAGE_MODEL.md)).
**This has not been exercised by a human against a real first run.** Until it
has, treat it as unverified rather than done — it changes the very first thing
every new user touches, and the paths worth walking are: a non-European answer
downloading and using Whisper end to end, a European answer still landing on
Parakeet, and an existing install upgrading without being sent back to step one.

## 2. Serious risks

The launch survives these, but they cost reputation in public.

### 2.1 Device disconnection can silently break a recording

Audit findings 13 and 14, both frozen and unfixed. When a capture device drops
mid-meeting, `handle_device_disconnect` marks whatever the session holds for
that role rather than the device actually named, and when no device is on
record it does nothing at all — so nothing ever starts a reconnect attempt.

Launch day means many users on hardware we have never seen, including Bluetooth
headsets, which drop. A meeting recorder that silently stops recording is the
single worst failure mode this product has, and the one guaranteed to be
written about. **Fix before launch.**

### 2.2 Auto-stop: two of three paths verified in real meetings

First real exercise, 17 August 2026, against a Teams call. What held:

- The observer noticed the meeting had ended and proposed a stop. Capture kept
  running through the silence debounce, which is the intended behaviour —
  `silence_duration_seconds` is 120 and the observer polls every 30, so the
  microphone stays live for roughly two to two and a half minutes after the
  meeting closes rather than cutting a meeting off the instant its window goes.
- The microphone indicator went dark as the prompt appeared. That reading was
  a coincidence, not evidence: the answer came nine seconds later, and it is
  the stop that closes the streams. What `propose_stop` does before emitting
  the prompt is stop *keeping* the audio, not stop capturing it — see the
  mechanism note below.
- Answering "stop" saved the recording, and summary generation started.

The unanswered path was then walked on a second Teams call the same day, and
the recordings were measured rather than eyeballed:

- Proposal at 13:01:54, no answer, stop at 13:03:54 — exactly the configured
  120 seconds.
- The saved audio is 31:05.76 long against a 12:30:48 start, which lands on
  13:01:54 to the second. **The two minutes the dialog stood are not in the
  file.** The first call agrees: 45:42.72 ends at its 11:16:17 proposal, not at
  the 11:16:26 confirmation, so even those nine seconds are absent.
- 207 transcript segments, logged as `ZERO chunks lost`.

One mechanism is worth recording because it is easy to misread. The pause does
not stop the audio streams — the log shows `Stopping all audio streams` only at
the real stop. It drops samples at the top of
`AudioProcessor::process_audio_data`, which returns early unless
`state.is_active()`. Nothing reaches the file, but **the microphone stays open
at the OS level for up to two minutes while the prompt waits**, so macOS shows
the app as listening while it is deliberately keeping nothing. That is a
privacy-optics discrepancy rather than a data bug, and it deserves a decision
before launch: either stop the streams on pause, or accept it knowingly.

One exit remains unexercised, and it is the riskiest:

- **"Keep recording".** `owns_pause` should resume capture, and the resulting
  file must be continuous and playable across the pause. A resume that silently
  fails would produce a recording that looks fine and contains nothing.

### 2.3 First run downloads several gigabytes before the app does anything

The installer is 56 MB. Then the app needs a summarization model (1.0–2.6 GB,
see §4) and a Whisper model on top. Someone arriving from a launch post expects
a small download and a working app; instead they get a long wait with no output.

This does not need solving with engineering — it needs honesty in the listing
and in onboarding: say the model sizes up front, default to the smallest
capable model, and make the download progress and its remaining time obvious.

One part of it did need engineering, and is fixed. Parakeet v3 — the engine
chosen for all 25 European languages, so the first run of most installations —
downloaded 3 GB from `meetily.towardsgeneralintelligence.com`, a host belonging
to the project this one forked away from. A launch that sends its traffic there
depends on someone else's server staying up, staying free, and not minding. It
now fetches from istupakov's HuggingFace repository, which upstream's server was
mirroring: every filename in both the fp32 and int8 sets resolves there, and the
sizes match to the byte against copies already on disk, the 2.4 GB encoder
weights included. Whisper and Parakeet v2 already came from HuggingFace; v3 was
the only exception.

### 2.4 The token estimator was Latin-only, so CJK transcripts were mis-chunked — fixed

Found while building the bake-off corpus, not by looking for it.

`rough_token_count` (`summary/processor.rs`) is a flat
`chars × 0.35` for every language. That is about right for Latin scripts and
about three times wrong for Chinese, Japanese and Korean, where one character is
roughly one token and a kanji can be more.

Two consequences, both in the same direction:

- `total_tokens < token_threshold` chooses single-pass over map/reduce. A
  Japanese transcript is estimated at a third of its real weight, so a meeting
  that should be chunked is sent whole.
- `chunk_text` divides by the same ratio, so when chunking does happen the
  chunks are around three times larger than intended.

The 32k context absorbs a lot of this, which is presumably why nobody noticed.
What it removes is the margin the threshold exists to provide, and it removes it
for exactly the users the language work was done to serve — a long Japanese or
Chinese meeting is the case most likely to overflow, and the case least likely
to be tested here.

**Fixed 19 August.** `rough_token_count` counts Han, kana and hangul at about
one token per character and everything else at 0.35, so the threshold means for
Japanese what it already meant for Latin scripts. Latin estimates are unchanged,
which is what keeps every existing threshold meaning what it meant.

The bake-off's corpus validator had deliberately not copied the app's figure —
copying it would have declared a Japanese transcript "short" for the same wrong
reason and hidden the problem inside the experiment meant to find it. That is
how it was found at all.

A second defect from the same run is fixed alongside it:
`clean_llm_markdown_output` stripped a code fence only when the model closed it,
and passed our own `<document>` tag and an unfilled `# <Add Title here>`
placeholder straight through to the reader. `gemma3:1b` produced all three. The
pipeline should not depend on the model being strong enough to avoid them.

### 2.5 Updates reach existing installations — built 23 August 2026, unverified

**Closed in code, open in practice.** `tauri-plugin-updater` is registered,
`tauri.conf.json` carries an endpoint and a public key, and `updater.rs`
exposes two commands the interface calls: one check, one install-and-restart.
The check runs once per launch when the setting is on, and from a button in
Settings whenever the user asks.

**Not on a timer, deliberately.** A periodic check would turn "is Halvern open
right now" into a signal with a heartbeat, and when somebody is in meetings is
the thing this product exists to keep to itself. What a check discloses is
written into PRIVACY_POLICY.md rather than left to be discovered: one GET for a
static file, no identifier, no body.

**What has not happened yet:**

- `TAURI_SIGNING_PRIVATE_KEY` is not in the repository secrets, so a release
  built today would ship without the signature the updater requires, and every
  update would be refused. The keypair exists; the secret has to be added by
  hand.
- No release has ever been cut from this repository, so `latest.json` has never
  been produced and the endpoint currently 404s. The first release proves the
  path or finds what is wrong with it.
- Nobody has installed an update this way. Until an old build has pulled a new
  one on a real machine, this is code that compiles rather than a feature that
  works.

## 3. What is already strong

### 3.1 The privacy claim is true, and provable

This was checked end to end rather than taken on faith, because it is the
product's central claim and the one an audience will test hardest.

- The telemetry endpoint comes from `option_env!("HALVERN_TELEMETRY_ENDPOINT")`
  — a compile-time variable. Unset at build time, it is `None`.
- The analytics client is constructed only `if config.enabled &&
  !config.api_key.is_empty()` (`analytics/analytics.rs`). With no key there is
  no client, and every send path returns early on `None`.
- Consent defaults to `false` in every branch of `AnalyticsProvider.tsx`.

So a default build sends nothing, and cannot be made to send anything without
rebuilding it with credentials. That is a stronger position than most products
claiming "privacy-first", and it is worth stating precisely — including how a
sceptical reader can verify it in the source, which is the kind of claim this
audience rewards.

### 3.2 The test suite is real

502 passing tests, 3 ignored, none failing, judged against per-module targets
rather than one flat number (see
[frontend/src-tauri/TEST_COVERAGE_AUDIT.md](frontend/src-tauri/TEST_COVERAGE_AUDIT.md)).
Twenty defects were found and nineteen documented or fixed during that work.
The one long-standing failure — a `Duration` comparison off by four
nanoseconds, previously written off as float precision — turned out to be a
real defect and is fixed.

## 4. The built-in model lineup

The list is **untouched upstream Meetily 0.4.0** — `models.rs` has exactly one
commit, `vendor: Meetily Community Edition 0.4.0 (pristine upstream)`. All four
download URLs were verified live and resolve.

| Model | Size | Context | Auto-recommended? |
|---|---|---|---|
| Qwen 3.5 4B (High Quality) | 2.6 GB | 32768 | yes, above the RAM threshold |
| Qwen 3.5 2B (Balanced) | 1.2 GB | 32768 | yes, below it |
| Gemma 3 4B (Balanced) | 2.3 GB | 32768 | **never** |
| Gemma 3 1B (Fast) | 1.0 GB | 32768 | **never** |

Two things follow, and both are launch-relevant:

**The recommendation only ever picks Qwen.**
`recommend_summary_model` branches on system RAM alone and returns
`qwen3.5:4b` or `qwen3.5:2b`; the Gemma entries rank lower in
`summary_model_priority` and are reachable only by manual selection. Its
`_is_macos` parameter is unused.

**Nobody has evaluated this lineup across languages, and this product is
multilingual by design.** The summary-language picker offers 32 languages, and
a meeting recorder aimed at a global audience will be handed all of them. Yet
the model that almost every user ends up running is chosen by a RAM check that
has nothing to say about language at all.

That matters because the families differ here. Gemma 3 shipped with broad
multilingual coverage as an explicit design goal, while Qwen's small models
have historically been strongest in English and Chinese. "Always Qwen, never
Gemma" may still be the right answer at these sizes — but it is currently an
inherited default, not a decision, and no one has checked what it does to a
summary of a meeting held in German, Spanish, Japanese or Polish.

**This was measured on 19 August 2026, and the lineup stands.**
[docs/experiments/summary-model-bakeoff/results/REPORT.md](docs/experiments/summary-model-bakeoff/results/REPORT.md)
has the full run: five languages covering every language group, four models,
matched and shipped sampling, 40 generations. Qwen led recall in every group in
both tiers and fabricated nothing; `gemma3:1b` failed output hygiene in every
language. No cell of `language_score` changed.

Read the caveats before treating that as settled — one transcript per language,
one repeat, no long transcripts, no real meetings. It is a direction, not a
measurement, and the report says what a decisive re-run needs.

Two findings from it do not wait for a re-run. Neither small model is usable for
CJK: `qwen3.5:2b` wrote its English intermediate entirely in Chinese, inventing
a statistic and misattributing an action item along the way, while `gemma3:1b`
headed every summary with an unfilled `# <Add Title here>` placeholder and on
Japanese returned the document wrapped in a code fence with the prompt's own
`<document>` tag attached. `clean_llm_markdown_output` strips neither, so the
pipeline is relying on the model being strong enough not to need it.

Note that this is the *quality* half of the language problem. The correctness
half is §1.3, and it does not wait for measurement:
[docs/ONBOARDING_LANGUAGE_MODEL.md](docs/ONBOARDING_LANGUAGE_MODEL.md) designs
one onboarding question that settles both, plus two smaller defects it exposed —
Rust and the frontend disagree on the default transcription language, one of the
two values meaning "translate everything into English", and three separate
language lists (101 / 37 / 32 entries) exist with no authoritative source.

Note also that the interface itself is English-only; there is no UI
localisation. That is a defensible choice for this kind of tool, but it is a
choice, and it should be a conscious one rather than something discovered
after launch.

## 5. Attribution

This is an MIT fork of [Meetily](https://github.com/Zackriya-Solutions/meeting-minutes)
by Zackriya Solutions. The licence permits the rebrand and requires only that
the copyright notice survive, which it does in `LICENSE.md`.

Beyond the legal minimum: a launch audience checks provenance, and finds it
either from us or from a comment thread. Stating it plainly and generously in
the README and in the launch post converts a possible accusation into evidence
of good faith. There is no version of this where hiding it goes better.

## 6. Housekeeping

- Two CI workflows reference a `devtest` branch that does not exist on this
  fork's origin.
- `release.yml` has no macOS signing step (see §1.1).
- Artifact names across the build workflows are still `meetily-*`, and
  `release.yml` points at `s3://meetily-updates/` — upstream's release
  infrastructure, which this project does not use (see §2.5).
- Windows and Linux workflows are inherited from upstream and have never been
  run here. Do not claim cross-platform support until at least one has produced
  a working artifact.

## 7. Deliberately not before launch

Listed so the decision is visible rather than forgotten:

- The dead-subsystem audit and the adaptive-buffering decision (Audit 2 in the
  coverage document, ~1,100 lines). Invisible to users.
- Raising coverage to the per-class targets. Invisible to users.
- Audit finding 16 (integer sample conversion overshooting −1.0 by 0.003%).
  Inaudible; fold it in only if the audio path is already being touched for
  findings 13 and 14.

## 8. Order of work

1. **Finish verifying auto-stop** (§2.2): confirm-stop and no-answer both hold
   and end the file at the right second; only keep-recording is unwalked. Decide
   the open-microphone question in the same pass. Then fix findings 13 and 14
   (device reconnect), with 16 folded in.
2. **Finish verifying the language onboarding** (§1.3). One path was walked on
   19 August — a clean install under `HALVERN_APP_SUFFIX`, which is what that
   variable exists for. The two language-dependent paths remain: a non-European
   answer downloading and using Whisper end to end, and a European answer still
   landing on Parakeet.
3. **Make first-run weight honest** (§2.3): say the model sizes up front and
   show remaining time. The default itself is now measured rather than inherited
   (§4), so what is left here is copy, not engineering.
4. **Automate signing in `release.yml`**, including the disk-image notarization
   that Tauri skips — done by hand today, and the kind of step that gets
   forgotten exactly once.
5. **Housekeeping**: CI branches, contributor docs, one non-macOS build proven.

Item 1 now decides whether the launch generates good stories or bad ones, and
item 2 is what makes it addressable to more than Europe. The long pole is gone
entirely: the Apple work — enrolment, certificate, notarization and verification
on a second machine — took an afternoon rather than the days its external lead
time suggested. **There is no hard blocker left.**

**Done since this list was first written**: the rebrand including the data paths
and a migration for existing installations (§1.2); moving the Parakeet v3
download off upstream's server onto HuggingFace (§2.3); the summary-model
bake-off, which settled §4 by measurement and closed §2.4; the onboarding model
step, which replaces a silent default with a visible choice; and an
accessibility pass over onboarding.

**Re-opened by our own fix**: the small-model tier. `gemma3:1b` lost the bake-off
partly on output-hygiene defects that `clean_llm_markdown_output` now strips, so
that comparison should be re-run before `qwen3.5:2b` is treated as settled. Not
a launch blocker — the user can now see the list and choose — but the report
says what a decisive re-run needs.
