# Leaving onboarding before the models have finished

Design document. Written 4 September 2026, after the first run of Halvern on
Windows, and before any of it is built.

## What happened

A first run on a slow connection stalls at step five. The transcription model
is one to three gigabytes depending on the answers given, the progress bar
moves, and the Continue button stays grey until it reaches the end. There is
nothing else on screen and nothing else to do. On a household connection
shared with someone else's evening, that is a long time to look at a bar.

Nothing is broken. The download resumes, the app works afterwards, and the
person who waits gets exactly what they were promised. But the first thing a
new user learns about Halvern is that it makes them wait, before they have
seen a single screen of it.

## What the code does today

One line decides this, `DownloadProgressStep.tsx:601`:

```tsx
disabled={!transcriptionModelDownloaded || isCompleting}
```

Two things follow from reading it carefully.

**The summary model already does not block.** Only
`transcriptionModelDownloaded` appears in that condition. The summary model
downloads on the same screen, shows the same kind of progress, and nobody waits
for it. So "this one blocks, that one does not" is not a new idea being
introduced here; it is an existing distinction being moved by one model.

**The recording path already knows this state.** `useRecordingStart.ts:99-117`
checks whether the configured model is ready, and when it is not, asks
`checkIfModelDownloading()` and produces a different message for each answer —
"Model download in progress" against "Transcription model not ready". That
logic does not need writing. It needs surfacing.

## The change

### Step five stops being a gate

The button is always enabled. Its label says what pressing it does:

| State | Label |
|---|---|
| Transcription model still downloading | `Explore while this downloads` |
| Everything downloaded | `Start using Halvern` |

Progress stays on the screen exactly as it is now, so a person on a fast
connection sees the same thing they see today and never reads the first label.
Nobody is pushed out of the step; the door is simply unlocked.

`completeOnboarding()` runs either way. Onboarding is finished in the sense
that matters — the questions have been answered, the plan is chosen, the
downloads are running — and re-entering it later would ask those questions
again.

### Progress gets a home outside onboarding

Once the step is left there is nowhere in the app that says a download is
running. The top bar is the right place: it is already the shared surface for
live state, it already carries the recording indicator, and it is on every
screen.

A compact row appears there while a model is downloading, naming which model
and its percentage, and disappears when the last one finishes. On failure it
turns into an error state that links to Settings → Transcription.

This is deliberately not a toast. A toast for a fifteen-minute download is
either dismissed and gone or permanent and in the way.

### The recording control states its reason

The button is `disabled` today, which this repository's own accessibility rule
says is wrong when a control is off for a reason: a `disabled` button leaves
the tab order, so a screen-reader user meets a gap and no explanation. See
CLAUDE.md, "Accessibility rules that are easy to break by accident".

The pattern to copy is fifteen lines up in the same file. `RecordingControls.tsx:344-361`
already renders the finalizing state as `aria-disabled` with an
`aria-describedby` pointing at a visible sentence. The downloading state gets
the same shape, with the reason naming the model and the percentage rather than
being generic.

The toast in `useRecordingStart` stays. It is what answers a click, and a click
is still possible because the control is no longer `disabled`.

### Failure has to be recoverable from outside onboarding

Retry lives inside the onboarding step today, which is unreachable once the
step is left. Settings → Transcription already owns model management and is
where the retry belongs. The top-bar error state links to it.

Without this the change trades a slow first run for an unrecoverable one, which
is a worse defect than the one being fixed.

## What does not change

**Recording still requires the transcription model.** The app's model is that a
meeting becomes a searchable transcript; audio with no transcript is a file
nobody will ever find again. Recording without transcription would mean a
queue, a retry policy, and a way to tell someone their meeting is not searchable
yet — a feature, not a relaxation of this one.

Worth noting for whoever picks that up: `audio/retranscription.rs` exists, so
the machinery for transcribing an existing recording is closer than it looks.
It is out of scope here.

**No download starts that did not start before.** This changes when a person may
leave a screen. It does not change what is fetched, from where, or how much.

## Open decisions

1. **Should recording be allowed with transcription deferred?** The answer above
   is "no, out of scope", and that is a product call rather than a technical
   one. If the answer becomes yes, this document's last section is the place it
   changes.

2. **Should the top-bar indicator be dismissible?** Arguments both ways: a
   fifteen-minute row is clutter, and a dismissed row is a download nobody can
   find again. Leaning towards not dismissible, since it removes itself.

## Order of work

1. `DownloadProgressStep.tsx` — remove the gate, make the label state-dependent.
   Smallest change, and on its own it already fixes the reported problem badly:
   explorable app, no progress anywhere, recording off with no reason. Do not
   ship it alone.
2. Top-bar indicator — new component, fed by the same download events
   `DownloadProgressStep` listens to. Needs those events to be reachable from
   app-level state rather than from one step's local `useState`.
3. `RecordingControls.tsx` — `aria-disabled` plus the visible reason, copying
   the finalizing branch.
4. Settings → Transcription — retry for a failed download, and make sure the
   existing model management reflects a partially-downloaded state honestly.

Steps 1 to 3 are the change. Step 4 is what makes it safe.

## How to test it

The interesting cases are all slow or broken networks, which is exactly what a
developer machine does not have.

- **Throttle.** A first run on a connection limited to a few hundred kilobits.
  Chrome DevTools cannot help here; the download happens in Rust, not in the
  webview. On macOS use Network Link Conditioner; on Windows, `clumsy` or the
  router's own shaping.
- **Interrupt.** Pull the network mid-download, leave the step, and confirm the
  top bar shows the failure and Settings can retry it.
- **Race.** Leave the step at ninety-nine per cent and confirm the indicator
  disappears and the recording control becomes usable without a restart.
- **Fresh profile every time.** `HALVERN_APP_SUFFIX=onboarding pnpm run tauri:dev`
  gives its own data directory, so onboarding appears from step one. Remove
  `~/Library/Application Support/io.halvern.app.onboarding` afterwards.

## Why this is written down before it is built

The reported symptom is one disabled button, and the fix for one disabled
button is one line. Shipping that line alone would produce an app that can be
explored, gives no sign that anything is downloading, refuses to record without
saying why, and cannot retry a failed download at all. Three of those four are
worse than waiting at a progress bar.
