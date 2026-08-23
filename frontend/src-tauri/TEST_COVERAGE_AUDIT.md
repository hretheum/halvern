# Test Coverage: Audit Method, Targets, and Plan

This document defines how test coverage is judged in the Rust core, what the
targets are per kind of module, how to measure them reproducibly, and in what
order the remaining gaps get closed.

It exists because of what the first coverage push actually produced. Between
`6b9e8bc` and `1f28ecf` the suite went from 295 to 447 passing tests, and along
the way it surfaced **19 defects and design gaps that reading the code had not**
— a silently-lost summary reported as saved, transcript search unable to find
any Polish uppercase text, a stop-proposal answer able to resolve a different
proposal, meeting folders overwriting each other, an entire adaptive-mixing
subsystem that no live code path constructs. Writing tests is currently the
cheapest defect detector this codebase has. The plan below is built to keep that
detector pointed where it still finds things.

## 1. Why a flat percentage target is the wrong goal

Two measurements from this codebase, both true at the same time:

- `detection/service.rs` sits at **0% line coverage, and that is correct.**
  After the `7b89f43` refactor it contains no decisions — only lock, call,
  emit. Its logic lives in `detection/policy.rs`, which is at 99%.
- `audio/device_detection.rs` has **9 tests and good coverage of code that
  drives nothing.** Its classification result is logged and then explicitly
  discarded (`pipeline.rs:900`, `let _ = (...)`), and its only behavioural
  consumer, `audio/ffmpeg_mixer.rs`, is never constructed by live code.

A single repo-wide "80%" target would score the first as a failure and the
second as a success. It would reward covering dead code and penalise the
refactor that made the codebase testable. So the target is defined per class of
module, and the classification is itself an audited artifact.

## 2. Module classification

Every `.rs` file in `src/` carries exactly one class. An unclassified file is an
audit failure, not a neutral state.

| Class | What it is | Line target | Additional requirement |
|---|---|---|---|
| **A — Pure logic** | No I/O, no framework types, deterministic given arguments | ≥ 90% | 100% of public functions with a non-trivial body have both a main-path and an error/edge-path test |
| **B — Replaceable-resource I/O** | Touches a database, filesystem, or clock through a seam a test can substitute (in-memory SQLite, `tempfile`, injected path) | ≥ 85% | No test may touch a real user path or the network |
| **C — Orchestration** | Sequences A and B parts; usually holds a few real decisions tangled into glue | ≥ 50% | Mandatory extraction review: any decision found inside must be named, moved to an A-class function, and frozen by a characterization test before the rewiring |
| **D — Irreducible glue** | FFI, audio hardware, `AppHandle`, network calls — cannot be exercised without the thing itself | none | Every D file needs a written out-of-scope entry naming *what* blocks it. Coverage of 0% with no entry is a failure; 0% with an entry is a pass |

Two rules make the classification load-bearing rather than decorative:

1. **A file's class may change, and C → A is the desired direction.** Extraction
   moves logic out of orchestration into pure functions. `detection/service.rs`
   went C → D this way; `recording_manager.rs` shed five pure functions into A
   while staying C.
2. **A class-D declaration is a claim that gets re-checked.** "Needs an
   `AppHandle`" stops being true the moment someone introduces a seam. The
   out-of-scope list is reviewed each audit round, not written once.

## 3. Metrics

### Primary — these are the targets

- **M1 — Line coverage against class target.** Per file, from `cargo llvm-cov`.
  Reported as distance from the file's class target, never as a bare repo-wide
  number.
- **M2 — Public API coverage.** Share of `pub fn` with a non-trivial body that
  have at least one main-path and one error/edge-path test. Trivial getters and
  one-line delegations are excluded, and the exclusion is listed explicitly so
  it cannot quietly grow.
- **M3 — Class-D declaration completeness.** Share of D files with a written
  justification. Target: 100%. This is what stops "untestable" from becoming an
  unexamined excuse.

### Secondary — health signals, deliberately not targets

- **M4 — Finding yield.** Frozen findings per 1000 newly covered lines. The
  first push ran at roughly 19 findings per 3600 covered lines. **This is the
  metric that says when to stop:** when a module's yield approaches zero, more
  coverage there is diminishing returns and the effort should move elsewhere.
  Optimising M1 while M4 sits at zero is how teams end up with high coverage and
  the same defect rate.
- **M5 — Dead-code ratio.** Lines in modules unreachable from any entry point,
  over total lines.
- **M6 — Stub ratio.** Planned-but-unimplemented markers per module (see §6).

Making M4–M6 explicit non-targets is intentional: goodharting a finding count
would produce trivial "findings", and goodharting the dead-code ratio would
produce deletions that should have been revivals.

## 4. Measurement methods

All commands run from `frontend/src-tauri/`.

```bash
# M1 — line/region/function coverage, per file
cargo llvm-cov --lib --summary-only          # headline
cargo llvm-cov report                        # per-file table

# Suite state (the one known pre-existing failure is documented in §7)
cargo test --lib

# Lint delta — must not grow
cargo clippy --lib 2>&1 | grep -cE '^warning'
```

`cargo-llvm-cov` needs `llvm-tools-preview`; a fresh worktree also needs the
`llama-helper` sidecar built once (`node scripts/build-sidecar.js` from
`frontend/`) or the build fails on a missing resource path.

**M2** is counted by hand per module during a review round and recorded in the
per-module table. It resists scripting because "non-trivial body" is a judgment
call; making it a judgment call is the point.

**M5 (dead code)** uses a reachability sweep: for each file, take its public
symbols and check whether any is referenced from another file. Known
false-positive mode: a module whose public API is re-exported through `mod.rs`
and consumed via that path reads as unreachable, so every hit must be confirmed
by hand before it counts (`export/markdown.rs` is exactly this case). The sweep
is a candidate generator, not a verdict.

**M6 (stubs)** uses marker detectors: `todo!`/`unimplemented!`, `#[allow(dead_code)]`,
discarded computations (`let _ = (...)`), `TODO`/`FIXME`/`HACK` comments, and
prose markers such as "in the future", "not yet implemented", "placeholder",
"reserved for future use".

## 5. Audit 2 — dead subsystems

Runs against M5. For each confirmed unreachable module the audit records one of
three dispositions, and **"leave it" is not among them**:

- **Revive** — the feature was intended and is still wanted; wire it to a live
  path and cover it to its class target.
- **Delete** — it is abandoned; remove it, and remove the tests that were
  guarding it so coverage numbers stop counting it.
- **Quarantine** — keep it, but mark it at the module level with an explicit
  note saying it is not wired, so the next reader is not misled the way this
  audit was.

The reason dispositions are forced: unreachable code that *looks* live has
already cost this project real time. During this audit a device-classification
bug was ranked top priority on the assumption it degraded audio quality, and
was only correctly downgraded after tracing that its result is discarded.

## 6. Audit 3 — planned but not implemented

This audit looks for upstream's *unfinished intentions* — places where the code
records a plan that was never carried out. These matter for three reasons: they
signal where the original authors were heading, they are a common source of
"why does this setting do nothing" bugs, and they inflate the apparent surface
of the codebase.

Signals, in rough order of strength:

1. **Computed and discarded** — a value produced, logged, then thrown away with
   a comment about future use. Strongest signal of a designed-but-unbuilt
   feature.
2. **Unreachable subsystem with a complete API** — a whole module built out but
   never constructed (an abandoned implementation rather than a missing one).
3. **Platform bails** — `bail!("not yet implemented for this platform")`.
4. **Placeholder modules** — files whose body is a comment saying the real
   implementation comes later.
5. **`#[allow(dead_code)]` census** — each one is either scaffolding for a
   planned feature or a leftover; both are worth knowing.
6. **Prose markers** — `TODO`, "reserved for future use", "for now".

Each hit gets recorded with: what was planned, how far it got, whether the plan
is still wanted, and what it would cost to finish. The output is a decision list
for the product owner, not a refactor backlog.

## 7. Known suite state

`audio::device_detection::tests::test_calculate_buffer_timeout_bluetooth` fails
on this machine and on untouched `main` — a `Duration` compared to nanosecond
precision (159.999996ms vs 160ms). It is not a regression from any of this work.
It is also, per §5, a test of a function whose result is currently discarded, so
its disposition follows whatever Audit 2 decides about `device_detection.rs` and
`ffmpeg_mixer.rs`. Until then it is the one expected red in every run, and every
coverage gate is stated as "no failures other than this one".

## 8. Model routing

Which model does which work, based on what this codebase actually cost to do:

| Work | Model | Why |
|---|---|---|
| Writing A/B-class tests against an existing seam | Sonnet | Mechanical once the pattern exists. The four database repositories were replicated from one worked example with no design decisions left open |
| Replicating an established test harness across sibling modules | Sonnet, or a subagent fan-out | Pure repetition; the judgment was spent on the first module |
| Deciding what to extract from a C-class file | **Opus** | This is where the work is genuinely hard: separating a real decision from glue, and knowing when extraction does not earn its diff. Proven on `recording_manager.rs`/`stream.rs`, where the correct answer included refusing three candidate extractions |
| Dead-code and stub sweeps | Sonnet | Scripted detection plus mechanical confirmation |
| Disposition calls in Audits 2 and 3 (revive/delete/quarantine) | Opus, then the product owner | Requires reading intent from incomplete evidence; the final call on product features is not a model's to make |
| A whole multi-module push run unattended, end to end with its own gates | Fable | Long autonomous execution with self-verification. Used for the Tailwind/BlockNote migration and the first coverage push |
| Anything that changes behaviour | any model, **but never bundled with test work** | Behaviour changes are separate, reviewed, user-approved commits. The convention that produced 19 clean findings is: freeze the behaviour, report it, change it only on request |

The one rule behind the table: **spend the expensive model on decisions, not on
volume.** Test-writing volume is cheap and parallelisable; deciding what a
tangled file should become is neither.

## 9. Execution plan

Each phase ends at a gate. A phase that does not pass its gate does not hand off.

**Phase 0 — baseline (done).** Suite at 447, per-file coverage measured, three
audits designed. Gate: this document.

**Phase 1 — the three audits, no code changes.** Run Audits 1–3 to completion
and produce their finding lists. Deliberately first: Audit 2 may delete modules
that Phase 2 would otherwise waste effort covering. Gate: a disposition recorded
for every dead-code and stub hit, and a class assigned to every file.

**Phase 2 — act on dispositions.** Delete, revive, or quarantine. Behaviour
changes here are individually approved, per §8's last row. Gate: suite green,
clippy delta zero, coverage numbers re-measured after deletions.

**Phase 3 — close class-A and class-B gaps.** The cheap, high-yield work, sized
by M4: modules still yielding findings first. Gate: every A file at ≥90%, every
B file at ≥85%, or an explicit written exception.

**Phase 4 — class-C extraction rounds.** One C file at a time, Opus-led,
characterization-first. Gate per file: extracted functions at A-class coverage,
public signatures unchanged, behaviour frozen.

**Phase 5 — close the D declarations.** Write the out-of-scope justification for
every remaining D file. Gate: M3 at 100%.

Phases 3 and 4 interleave safely and can run in parallel across modules; Phase 1
and Phase 2 must precede both, because covering code that is about to be deleted
is the most expensive mistake available here.

## 10. What "ambitious" means numerically

The honest target is not a single number, but the plan does commit to one for
tracking. With the D-class mass of this codebase (FFI engines, Tauri command
surfaces, hardware capture — roughly a third of all lines), a repo-wide line
coverage of **55–60%** represents essentially full coverage of everything that
*can* be covered. That is the ambition: not 90% repo-wide, which would require
mocking hardware into meaninglessness, but **every A and B file at target, every
C file reviewed for extraction, and every D file justified in writing.**

Repo-wide percentage is therefore a lagging indicator here, reported but never
optimised directly.

---

# Round 1 results — 16 August 2026

Measured at `033f18a` (merged `main`, 464 passing tests). Commands per §4; the one expected red
per §7.

> **Scope note — resolved.** Round 1 was first measured on
> `refaktor/detection-and-coverage` alone, while `feat/ui-redesign` and
> `feat/batch-transcription-provider` were still unmerged. All three have since
> been merged into `main` (linear, fast-forward, in that order), and the numbers
> below are **re-measured on the merged tree** at `033f18a`, 464 passing tests.
>
> What the merge changed, for the record: repo line coverage 36.21% → **36.65%**;
> `database/repositories/transcript.rs` 96.5% → 93.2% (the redesign added an
> uncovered `read_recording_origin`); `audio/recording_saver.rs` 73.1% → 74.2%
> (the redesign brought its own `origin_tests`); `summary/templates/loader.rs`
> unchanged at 77.0%. One earlier worry proved unfounded: the batch branch does
> not delete the 273 lines this round covers there — it was simply seven commits
> behind, so the diff read as deletions. The batch branch also arrived with its
> own tests, `audio/transcription/remote_provider.rs` landing at 94.7%.
>
> The dead-subsystem and planned-but-unimplemented findings were verified
> branch-independent before the merge and are unaffected by it.

## Baseline

| Metric | Value |
|---|---|
| Files in `src/` | 143 |
| Physical lines | 47,523 |
| Test functions | 469 |
| Line coverage (llvm-cov) | **36.65%** (27,650 lines, 17,515 uncovered) |
| Region coverage | 38.40% |
| Function coverage | 36.88% |
| Files > 100 lines with zero tests | 45 |

Branch trajectory: 23.02% → 32.52% → 36.21% across the three pushes on
`refaktor/detection-and-coverage`, then **36.65%** once that branch,
`feat/ui-redesign` and `feat/batch-transcription-provider` were all merged
into `main`.

## Audit 1 — coverage against class targets

At or above target (class A/B, no action):

| File | Class | Line cov |
|---|---|---|
| `database/repositories/transcript_chunk.rs` | B | 100% |
| `detection/policy.rs` | A | 99.3% |
| `database/repositories/summary.rs` | B | 97.5% |
| `database/repositories/setting.rs` | B | 97.4% |
| `export/markdown.rs` | A | 96.5% |
| `database/repositories/transcript.rs` | B | 95.2% |
| `audio/buffer_pool.rs` | A | 95.2% |
| `audio/recording_state.rs` | A | 92.5% |
| `summary/summary_engine/models.rs` | A | 91.8% |
| `summary/language_detection.rs` | A | 91.0% |

Below target, ranked by expected yield:

| File | Class | Line cov | Target | Note |
|---|---|---|---|---|
| `audio/level_monitor.rs` | A | 0% | 90% | Level maths is pure; highest-yield untouched A file |
| `summary/llm_client.rs` | A/D split | 0% | 90% on the A half | Provider parsing is pure; the HTTP call is not |
| `ollama/ollama.rs` | A/D split | 0% | 90% on the A half | Same shape: response parsing pure, transport not |
| `summary/metadata.rs` | A | 89.0% | 90% | Nearly there |
| `audio/vad.rs` | A | 61.4% | 90% | Large pure surface still uncovered |
| `summary/summary_engine/model_manager.rs` | B | 62.2% | 85% | Remainder is the live download loop (D) |
| `database/repositories/meeting.rs` | B | 68.7% | 85% | Query builder paths |
| `summary/templates/loader.rs` | B | 78.2% | 85% | |
| `audio/recording_saver.rs` | B | 76.7% | 85% | Remainder is `stop_and_save` (needs `AppHandle`) |
| `audio/transcription/engine.rs` | C | 0% | 50% | Extraction review not yet done |
| `audio/transcription/worker.rs` | C | 0% | 50% | Extraction review not yet done |

Correctly at 0% (class D, declaration present): `detection/service.rs` — pure
Tauri glue since `7b89f43`, its logic covered in `policy.rs`.

Class D, declaration still owed (M3 gap): `api/api.rs`, `whisper_engine/*`,
`parakeet_engine/*`, `summary/summary_engine/sidecar.rs`, `lib.rs`,
`audio/capture/core_audio.rs`, `tray.rs`, `notifications/*`, all `commands.rs`
surfaces. **M3 currently ≈ 8%** (1 of ~13 D files declared).

## Audit 2 — dead subsystems

| Module | Evidence | Proposed disposition |
|---|---|---|
| `audio/diagnostics.rs` (~180 lines) | 5 public functions, re-exported in `audio/mod.rs`, **zero call sites anywhere**. Confirmed by hand | Delete or revive — it is a logging surface nobody calls |
| `audio/ffmpeg_mixer.rs` (~700 lines) | `FFmpegAudioMixer` is **never constructed** in live code; the live pipeline uses `ProfessionalAudioMixer`. Only 2 of 10 public symbols referenced externally | Decide first, because it is the sole behavioural consumer of `device_detection` |
| `audio/stt.rs` | 2 of 8 public symbols used externally | Candidate — needs hand confirmation |
| `RecordingManager::stop_recording` | Dead path; the live stop goes through `stop_streams_and_force_flush` + `save_recording_only`. Takes `raw_tap::finish()` with it, so the diagnostic tap never closes its files | Delete, or move `raw_tap::finish()` onto the live path |
| `StreamManagerType` enum | No references in the crate | Delete |
| Import under two mutually exclusive `cfg`s (`recording_manager.rs:8-11`) | Compiles on no platform | Delete |

`export/markdown.rs` was flagged by the sweep and **cleared by hand** — it is
reached through `export/mod.rs`'s re-export. This is the documented
false-positive mode from §4.

Coupled decision worth stating plainly: `device_detection.rs` (203 lines, 83%
covered, 9 tests, including the one perpetually failing test) exists to feed
`ffmpeg_mixer.rs`. If that mixer is deleted, this module and its tests go with
it, and the long-standing red test disappears by deletion rather than by fix.

## Audit 3 — planned but not implemented

| Signal | Location | What was planned | How far it got |
|---|---|---|---|
| Computed and discarded | `audio/pipeline.rs:898-900` | Adaptive buffering driven by device kind — "can be used for adaptive buffering in the future" | Detection fully built, wired into the pipeline signature, then explicitly thrown away |
| Complete but unreachable subsystem | `audio/ffmpeg_mixer.rs` | FFmpeg-style adaptive mixing with per-source buffers and Bluetooth-aware timeouts | Fully implemented, never constructed |
| Placeholder module | `audio/capture/microphone.rs` | "Extract microphone AudioStream logic from core.rs" | Comment only; the extraction never happened |
| Platform bail | `audio/capture/system.rs:103` | System audio capture beyond macOS | `bail!("not yet implemented for this platform")` |
| Optional feature stub | `whisper_engine/system_monitor.rs:111-113` | Temperature monitoring | Disabled deliberately, "to avoid API compatibility issues" |
| Confidence scoring | `whisper_engine/parallel_processor.rs:363` | Per-segment confidence | `confidence_score: None, // TODO` |
| `#[allow(dead_code)]` census | 24 occurrences across the crate | Mixed scaffolding and leftovers | Each needs a one-line disposition |
| Unused command scaffolds | `console_utils/commands.rs`, `openrouter/commands.rs` | "can be used for other command utilities if needed in the future" | Empty scaffolds |

Read together, these point at one abandoned direction: **adaptive,
device-aware audio buffering**. Detection, mixing, and diagnostics were all
built for it; none of it is wired. That is the single largest product decision
this audit surfaces, and it belongs to the product owner, not to a refactor.

## Immediate recommendations

1. Decide the adaptive-buffering question first (§Audit 2 + §Audit 3). It
   governs roughly 1,100 lines across three modules and one failing test.
2. Then Phase 2 deletions, before any Phase 3 coverage work, so that effort is
   not spent covering code about to be removed.
3. Highest-yield coverage work once the ground is clear: `audio/level_monitor.rs`,
   the pure halves of `summary/llm_client.rs` and `ollama/ollama.rs`, and the
   remaining `audio/vad.rs` surface.
4. Write the class-D declarations (M3 from 8% to 100%) — cheap, and it converts
   "untested" into "deliberately out of scope with a reason".
