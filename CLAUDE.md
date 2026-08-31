# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Where things stand — 25 August 2026

Read this first after a break; the rest of the file is reference.

**The source is public.** [github.com/hretheum/halvern](https://github.com/hretheum/halvern),
one squashed root commit, MIT, not a GitHub fork. The full development history
stays in the private `hretheum/meetily` as the provenance record — see
[docs/OSS_LAUNCH.md](docs/OSS_LAUNCH.md) for why a new repository rather than a
rename.

**Two remotes, two histories, deliberately.** Local `main` carries the whole
history and pushes to `origin` (the private repo). The public repository has an
unrelated one-commit history, so commits reach it by building a commit from the
current tree onto its head:

```bash
TREE=$(git rev-parse 'HEAD^{tree}'); PUB=$(git rev-parse halvern/main)
NEW=$(git commit-tree "$TREE" -p "$PUB" -m "message")
git push https://github.com/hretheum/halvern.git "${NEW}:refs/heads/main"
git fetch -q halvern && git branch -f public halvern/main
```

Braces around `${NEW}` are not optional: in zsh, `"$NEW:refs/..."` triggers the
`:r` modifier and silently mangles the refspec.

**Live:** [halvern.io](https://halvern.io) serves `www/` from GitHub Pages over
HTTPS, certificate issued and enforced. `www.halvern.io` is **not** configured
yet — a CNAME to `hretheum.github.io.` is still needed, and the `www` TXT
record at OVH has to go first because a wildcard does not cover a name that
owns any record.

**Released: 0.1.0 and 0.1.1**, both signed, notarized and stapled — the app
during the build, the disk image afterwards, because Tauri does the first and
not the second. Both were cut locally; `docs/SIGNING.md` §5 lists the six
secrets CI would need and the repository holds one of them.

**The update path is proven end to end.** 0.1.1 was offered to an installed
0.1.0, downloaded, signature-checked, installed, and the application restarted
into the new version — 25 August, in the log. Everything before that had only
shown the check working, never the install.

That took three failed attempts to reach, and the reason is worth keeping: the
network was down, and nothing said so. The install logged its start and not its
failure, and the notice in the corner sent the error to `console.error`. Both
are fixed — the log records failures and download progress at quarter marks,
and the notice shows the reason with a "Try again" button. The same action had
been reporting correctly all along in Settings → Updates, which is why nobody
noticed.

**The landing page keeps its own version current.** `www/index.html` carries
`dl-size` and `dl-version` markers, stamped from the latest release when Pages
deploys. It cannot be triggered by the release event: the `github-pages`
environment only accepts deployments from `main`, and a tag deploy would
publish `www/` as it stood at the tag. Releasing therefore ends with
`gh workflow run pages.yml --ref main` — step 6 in docs/VERSIONING.md.

**Actions are off in the private repository.** Its CI ran a `macos-latest` job
that GitHub bills at ×10, Dependabot opened sixteen pull requests there, and a
single day cost around 2500 billable minutes against a monthly allowance of
2000–3000. Public repositories are free, including macOS, so the public repo
keeps its CI; the private one is an archive and needs none. Build, Release, CI
and Pages are disabled there. Dependabot is not, and closing its pull requests
did not settle it — see the paragraph on both remotes below.

**Toolchain is pinned** by `rust-toolchain.toml` (1.98.0), `packageManager`
(pnpm 11.21.0) and `engines` (Node ≥22.13). None of the three floats, and the
clippy baseline — now 25 — is only meaningful because of that.

**Dependencies are current, and the cooldown that polices them is now written
down.** All ten Dependabot pull requests were applied *here* rather than merged
there, and that is the rule, not a preference: the public history is rebuilt
from this tree, so anything merged on the public side is silently reverted by
the next `commit-tree` push. Five action majors — including `pnpm/action-setup`
v6, which is the first version that knows pnpm 11 exists, and `tauri-action`
v1, whose renamed options this workflow happens not to pass. Then the weekly
patch-and-minor group, `@types/node` 20→26, framer-motion 11→13, and TypeScript
5.9→7.0, which is the Go compiler: the TypeScript step in `next build` fell
from 3.2s to 584ms.

The cooldown was never missing, which is the reverse of what this file said
until 31 August. pnpm 11 defaults `minimumReleaseAge` to 1440, so a day-long
delay has been in force ever since the pnpm pin landed. `pnpm config get
minimumReleaseAge` answers `undefined` because it is unset, and unset is not
the same as inactive — reading it as inactive is what made the
`minimumReleaseAgeExclude` list look like it guarded nothing, and lucide-react
1.34.0 was held out of the sweep on that mistaken ground.

Both halves are now stated. `pnpm-workspace.yaml` sets seven days,
`.github/dependabot.yml` carries a matching seven-day `cooldown` per ecosystem
so the bot cannot propose a release pnpm will then refuse to install, and the
stale exclusions are gone. Checked rather than assumed: pnpm 11.21.0 rejects a
too-fresh version with `ERR_PNPM_NO_MATURE_MATCHING_VERSION`, honours
`name@version` in the exclude list, and `pnpm install --frozen-lockfile` passes
under the new value while reporting that it verified all 315 lockfile entries
against it. That last line answers the question the old note left open: a
frozen install does consult the cooldown.

**Dependabot runs against both remotes, and only one of them should be acting
on it.** The archive has Build, Release, CI and Pages disabled, which does
nothing to Dependabot — it runs as two synthetic workflows,
`dynamic/dependabot/dependabot-updates` and `dynamic/dependabot/update-graph`,
that the Actions API refuses to disable (HTTP 422) and that no file in this
tree controls. `.github/dependabot.yml` is the same blob object in both
repositories, so the config cannot say "not here" either. Closing the pull
requests is worse than doing nothing: `open-pull-requests-limit` frees the
slot, and the following Monday brings five different crates instead. On 31
August it brought ten in three minutes.

### Pick up here

1. **A real screenshot, then a demo recording, for the README.** The first
   screen still has no picture of the product. The four PNGs in
   `docs/assets/design-rollout/` are **design mockups, not app captures** —
   `Trigger silence detection (demo)` appears nowhere in the source — so they
   must not be used as if they were screenshots. The maintainer's own data was
   moved aside for this on 25 August so the app would show a clean first run;
   it is in `~/Library/Application Support/halvern-real-data-*` with a
   `README.txt` and the script that puts it back. **Restore it before recording
   anything real.**
2. **Launch on a Mac that has never run Halvern.** The signature half is
   proven twice over now: the released `.dmg` was downloaded from GitHub, given
   the quarantine attribute Safari attaches, and evaluated — `accepted /
   source=Notarized Developer ID` — and on 25 August the copy in
   `/Applications` that had updated itself in place from 0.1.0 verified the
   same way, `codesign --verify --deep --strict` and `stapler validate` both
   clean. What a second machine decides is whether it *starts*: microphone and
   screen-recording prompts, the macOS 14.4 floor, nothing missing.
3. **Back the updater key up off this machine.** One readable copy exists, in
   `~/Documents/halvern-keys/`. A GitHub secret is write-only and is not a
   backup. Losing it cuts every installation off from updates forever —
   `docs/SIGNING.md` §3a. This matters more now than yesterday: installations
   exist.
4. **`www` CNAME**, and consider dropping the wildcard A/AAAA records at OVH:
   they currently point every subdomain at GitHub Pages.
5. **The cubic.dev badge**, once someone has confirmed in a logged-out browser
   that the wiki is publicly readable.
6. **Why a Bluetooth microphone stream opens and stays empty.** Two shapes,
   both seen on 26 August and both still unexplained: a stream that produced
   its first sample three minutes and twenty-three seconds after opening, and
   streams that produced samples immediately whose every value was zero. What
   changed is that neither is invisible now — `audio_input_activity` counts
   both and the recording screen says which one is happening, naming the
   device. Gotcha 12 has the evidence and the dead ends already ruled out;
   start from the counters and `raw_tap`, not from device enumeration.
7. **Turn Dependabot off on the archive repository.** Nothing it opens there
   can be merged: `origin` is pushed to one-way from this checkout, so a merge
   commit on its `main` breaks that. Grouping and the cooldown landed on 31
   August and cut the volume, but the bot should not be running there at all.
   It is a switch in Settings → Code security on `hretheum/meetily` and
   nowhere else — there is no REST API for it, and the Actions API will not
   disable the dynamic workflow that carries it.

## Project Overview

**Halvern** is a privacy-first AI meeting assistant that captures, transcribes, and summarizes meetings entirely on local infrastructure. The supported application is the Tauri desktop app with a Rust core.

It is a fork of [Meetily](https://github.com/Zackriya-Solutions/meeting-minutes)
by Zackriya Solutions, used under the MIT licence; the name appears throughout
this document's history. See [README.md](README.md) for the attribution as
published.

1. **Frontend**: Tauri-based desktop application (Rust + Next.js + TypeScript)
2. **Rust Backend**: Tauri commands, audio capture, transcription, storage, and summarization orchestration

Upstream also shipped a Python/FastAPI tier with its own Docker setup and a
standalone whisper-server. It was archived and unsupported here, and was
deleted before the public release — there is no second tier to run, and no
service to point the app at.

### Key Technology Stack
- **Desktop App**: Tauri 2.x (Rust) + Next.js 16 (webpack, the `--webpack` flag is deliberate) + React 19 (strict mode on) + Tailwind 4 + shadcn/ui (new-york, lucide icons)
- **Audio Processing**: Rust (cpal, whisper-rs, professional audio mixing)
- **Transcription**: Whisper.cpp / whisper-rs and Parakeet paths in the Tauri app
- **App API Surface**: Tauri commands and events. There is no server tier.
- **LLM Integration**: Ollama (local), Claude, Groq, OpenRouter

## Essential Development Commands

### Frontend Development (Tauri Desktop App)

**Location**: `/frontend`

```bash
# macOS Development
./clean_run.sh              # Clean build and run with info logging
./clean_run.sh debug        # Run with debug logging
./clean_build.sh            # Production build

# Windows Development
clean_run_windows.bat       # Clean build and run
clean_build_windows.bat     # Production build

# Manual Commands
pnpm install                # Install dependencies
pnpm run dev                # Next.js dev server (port 3118)
pnpm run tauri:dev          # Full Tauri development mode
pnpm run tauri:build        # Production build

# GPU-Specific Builds (for testing acceleration)
pnpm run tauri:dev:metal    # macOS Metal GPU
pnpm run tauri:dev:cuda     # NVIDIA CUDA
pnpm run tauri:dev:vulkan   # AMD/Intel Vulkan
pnpm run tauri:dev:cpu      # CPU-only (no GPU)

# A clean profile — onboarding from step one, your real library untouched
HALVERN_APP_SUFFIX=shots pnpm run tauri:dev
rm -rf ~/Library/Application\ Support/io.halvern.app.shots   # afterwards
```

`HALVERN_APP_SUFFIX` changes the bundle identifier, so the build gets its own
database, settings and models. macOS grants permissions per identifier, so a
suffixed build asks for microphone and screen recording again — expected, not a
fault.

**The three versions that matter are pinned, and all three had to be fixed on
the day the repository went public:**

| What | Where | Why it is pinned |
|---|---|---|
| Rust 1.98.0 | `rust-toolchain.toml` | A clippy warning count is only comparable against one compiler. A floating `stable` reported 63 warnings against a local 27 and failed CI on unchanged code. |
| pnpm 11.21.0 | `packageManager` in `frontend/package.json` | `pnpm-workspace.yaml` carries settings, not workspace members, and only pnpm ≥10 reads it that way. pnpm honours this field by downgrading itself to it, so a wrong value is invisible — `npx pnpm@10` also reported 8.15.9. |
| Node ≥22.13 | `engines` in `frontend/package.json` | pnpm 11 requires it. It was undeclared, so CI discovered it as `ERR_UNKNOWN_BUILTIN_MODULE`. |

Nothing else may hardcode a version; the interface asks the app at runtime.
See [docs/VERSIONING.md](docs/VERSIONING.md).

### Window chrome, and why Liquid Glass does not apply

Liquid Glass is Apple's design language from WWDC 2025 and it does cover macOS,
not just iPhone. It does **not** reach this app: the interface is a WKWebView
rendering Tailwind, and the material is available to AppKit and SwiftUI
controls. A webview renders your CSS and gets none of it, however new the SDK.

Native window vibrancy behind a translucent webview is achievable
(`windowEffects`), and is deliberately not used. Translucency composites against
whatever sits behind the window, so no token can promise a contrast figure —
the same class of defect as dimming text with opacity, which this codebase
already had once. It would also fight a brand whose mark is a gatehouse and
whose entire pitch is that nothing gets out; a wall that looks transparent is an
unfortunate joke.

The window keeps the standard title bar. Both alternatives were built and
looked at on 19 August, and both were dropped.

`titleBarStyle: "Transparent"` does not make anything see-through — it only
stops macOS drawing its own title-bar material, showing the window background
instead. With this palette the difference was invisible, so it was a setting
that did nothing and would puzzle whoever found it later.

`Overlay` does produce the seamless bar, by running the webview under the title
bar. It also puts the traffic lights on the top-left corner, so the brand mark
has to move 78px right — off the 16px margin rule the rest of the app aligns to
(`lib/layout.ts`). Tried, seen, judged not worth the rule.

That attempt is also where a wider rule came from: **layout that must be correct
at first paint cannot depend on a client-side hook.** The clearance was behind
`usePlatform`, which reads `navigator`; during the static prerender there is no
`navigator`, so the first paint used the non-macOS padding and the traffic
lights landed on top of the mark.

`minimumSystemVersion` is `14.4`, which is where the system-audio capture lives.
It was unset until 19 August, so the package would install happily on an older
macOS and fail only when somebody tried to record.

### The landing page

**Location**: `/www` — one self-contained `index.html`, no build step and no
dependencies. Screenshots are inlined as data URIs so the file can be dropped on
any static host or opened from disk.

It takes its palette from `design/tokens/halvern.tokens.json`, the same pyramid
the app compiles, so the site cannot drift from the product on colour. Prose is
set in the system face; anything a reader could verify — sizes, versions,
requirements, how telemetry is compiled out, what the page itself records — is
monospace. That split is load-bearing, not decorative.

Both links now resolve to the public repository — see gotcha 10 for what the
download button points at and why it is not a direct file link.

The page states in public that it carries no analytics script, no cookies and no
fingerprinting. `check-network-hosts` therefore covers `www/` — it scanned only
the app before, leaving the one directory where a tracker would actually be
added invisible to it.

**Published to [halvern.io](https://halvern.io)** by
`.github/workflows/pages.yml`, which uploads `www/` as an artifact because
branch publishing only offers the repository root or `/docs`. The workflow
**refuses to deploy an `index.html` containing a `<script>` tag**: the page's
promise is enforceable, so it is enforced.

**The host was a constrained choice, and the copy is coupled to it.** Vercel,
Cloudflare and Plausible all deliver analytics as a browser beacon, which would
make the page's first sentence false. Cloudflare Pages was also dropped for its
IP-reputation interstitial, which lands on VPN and Tor users — this product's
audience. GitHub Pages gives no traffic figures at all, which is affordable
only because the number actually wanted, downloads, comes free from the GitHub
releases API. Moving to Netlify for server-side analytics means rewriting "What
this page collects" in the same commit. Reasoning in
[docs/OSS_LAUNCH.md](docs/OSS_LAUNCH.md) §8.

### Service Endpoints
- **Frontend Dev**: http://localhost:3118

## High-Level Architecture

### Tauri Desktop Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Frontend (Tauri Desktop App)                  │
│  ┌──────────────────┐  ┌─────────────────┐  ┌────────────────┐ │
│  │   Next.js UI     │  │  Rust Backend   │  │ Whisper Engine │ │
│  │  (React/TS)      │←→│  (Audio + IPC)  │←→│  (Local STT)   │ │
│  └──────────────────┘  └─────────────────┘  └────────────────┘ │
│         ↑ Tauri Events           ↑ Audio Pipeline               │
└─────────────────────────────────────────────────────────────────┘
```

There is no server tier. Meeting persistence, local transcription and summary orchestration all happen in the Rust/Tauri core, which is what lets the app work with no network at all.

### UI Structure (four screens, no sidebar)

The 2026-08 redesign replaced the sidebar-drawer layout with four full screens
under a shared top bar (theme switch, settings, live-recording indicator):

- **`/` — Library**: the home screen. Search (FTS5 with snippets), composable
  filters, sorting, date grouping, density, selection with bulk export/delete.
  Backed by `api_query_meetings`; data hook in `hooks/useMeetingLibrary.ts`.
- **`/meeting-details` — Workshop**: identity header (inline title edit,
  date/duration/source/language/speakers meta row, meeting actions), transcript
  pane with match-stepping search, summary pane (BlockNote editor, loaded on
  demand via `next/dynamic` with an `innerRef` prop — `ref` would be swallowed).
- **`/record` — Recording**: status pill, elapsed clock, source line, pause/stop
  controls, and the live transcript below them.
- **`/settings` — Settings**: six categories in a left column (General,
  Recording, Transcription, Summarization, Export, Advanced).

Onboarding is six steps: Welcome, meeting language, summary model, setup
overview, download, permissions. The model step exists because the previous flow
asked `builtin_ai_get_recommended_model` and downloaded the answer without ever
showing the list, which made a wrong default unrecoverable — the user could not
know a choice had existed.

Theme tokens live in `app/globals.css` as HSL triplets consumed via
`hsl(var(--token))`; the palette is the redesign's warm-gray + teal. The Tauri
webview lies about `prefers-color-scheme`, so system-theme detection goes
through `SystemThemeBridge` (Rust-side window theme), never the media query.

### Audio Processing Pipeline (Critical Understanding)

The audio system has **two parallel paths** with different purposes:

```
Raw Audio (Mic + System)
         ↓
┌────────────────────────────────────────────────────────────┐
│              Audio Pipeline Manager                         │
│  (frontend/src-tauri/src/audio/pipeline.rs)                │
└─────────────┬──────────────────────────┬───────────────────┘
              ↓                          ↓
    ┌─────────────────┐        ┌─────────────────────┐
    │ Recording Path  │        │ Transcription Path  │
    │ (Pre-mixed)     │        │ (VAD-filtered)      │
    └─────────────────┘        └─────────────────────┘
              ↓                          ↓
    RecordingSaver.save()      WhisperEngine.transcribe()
```

**Key Insight**: The pipeline performs **professional audio mixing** (RMS-based ducking, clipping prevention) for recording, while simultaneously applying **Voice Activity Detection (VAD)** to send only speech segments to Whisper for transcription.

### Audio Device Modularization (Recently Completed)

**Context**: The audio system was refactored from a monolithic 1028-line `core.rs` file into focused modules.

```
audio/
├── devices/                    # Device discovery and configuration
│   ├── discovery.rs           # list_audio_devices, trigger_audio_permission
│   ├── microphone.rs          # default_input_device
│   ├── speakers.rs            # default_output_device
│   ├── configuration.rs       # AudioDevice types, parsing
│   └── platform/              # Platform-specific implementations
│       ├── windows.rs         # WASAPI logic (~200 lines)
│       ├── macos.rs           # ScreenCaptureKit logic
│       └── linux.rs           # ALSA/PulseAudio logic
├── capture/                   # Audio stream capture
│   ├── microphone.rs          # Microphone capture stream
│   ├── system.rs              # System audio capture stream
│   └── core_audio.rs          # macOS ScreenCaptureKit integration
├── pipeline.rs                # Audio mixing and VAD processing
├── recording_manager.rs       # High-level recording coordination
├── recording_commands.rs      # Tauri command interface
└── recording_saver.rs         # Audio file writing
```

**When working on audio features**:
- Device detection issues → `devices/discovery.rs` or `devices/platform/{windows,macos,linux}.rs`
- Microphone/speaker problems → `devices/microphone.rs` or `devices/speakers.rs`
- Audio capture issues → `capture/microphone.rs` or `capture/system.rs`
- Mixing/processing problems → `pipeline.rs`
- Recording workflow → `recording_manager.rs`

### Rust ↔ Frontend Communication (Tauri Architecture)

**Command Pattern** (Frontend → Rust):
```typescript
// Frontend: src/app/page.tsx
await invoke('start_recording', {
  mic_device_name: "Built-in Microphone",
  system_device_name: "BlackHole 2ch",
  meeting_name: "Team Standup"
});
```

```rust
// Rust: src/lib.rs
#[tauri::command]
async fn start_recording<R: Runtime>(
    app: AppHandle<R>,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
    meeting_name: Option<String>
) -> Result<(), String> {
    // Implementation delegates to audio::recording_commands
}
```

**Event Pattern** (Rust → Frontend):
```rust
// Rust: Emit transcript updates
app.emit("transcript-update", TranscriptUpdate {
    text: "Hello world".to_string(),
    timestamp: chrono::Utc::now(),
    // ...
})?;
```

```typescript
// Frontend: Listen for events
await listen<TranscriptUpdate>('transcript-update', (event) => {
  setTranscripts(prev => [...prev, event.payload]);
});
```

### Whisper Model Management

**Model Storage Locations**:
- **Development**: `frontend/models/`
- **Production (macOS)**: `~/Library/Application Support/io.halvern.app/models/`
- **Production (Windows)**: `%APPDATA%\io.halvern.app\models\`

Both production paths are Tauri's app-data directory, so they are the bundle
identifier — change `identifier` in `tauri.conf.json` and they follow. That is
distinct from the brand directory (`APP_DATA_DIR_NAME` in `lib.rs`, currently
`Halvern`), which holds custom templates and notification settings and is shared
by every installation rather than keyed to one. Renaming either strands existing
data; `scripts/migrate-meetily-to-halvern.sh` is the worked example of what such
a rename actually costs.

**Model Loading** (frontend/src-tauri/src/whisper_engine/whisper_engine.rs):
```rust
pub async fn load_model(&self, model_name: &str) -> Result<()> {
    // Automatically detects GPU capabilities (Metal/CUDA/Vulkan)
    // Falls back to CPU if GPU unavailable
}
```

**GPU Acceleration**:
- **macOS**: Metal + CoreML (automatically enabled)
- **Windows/Linux**: CUDA (NVIDIA), Vulkan (AMD/Intel), or CPU
- Configure via Cargo features: `--features cuda`, `--features vulkan`

## Critical Development Patterns

### 1. Audio Buffer Management

**Ring Buffer Mixing** (pipeline.rs):
- Mic and system audio arrive asynchronously at different rates
- Ring buffer accumulates samples until both streams have aligned windows (50ms)
- Professional mixing applies RMS-based ducking to prevent system audio from drowning out microphone
- Uses `VecDeque` for efficient windowed processing

### 2. Thread Safety and Async Boundaries

**Recording State** (recording_state.rs):
```rust
pub struct RecordingState {
    is_recording: Arc<AtomicBool>,
    audio_sender: Arc<RwLock<Option<mpsc::UnboundedSender<AudioChunk>>>>,
    // ...
}
```

**Key Pattern**: Use `Arc<RwLock<T>>` for shared state across async tasks, `Arc<AtomicBool>` for simple flags.

### 3. Error Handling and Logging

**Performance-Aware Logging** (lib.rs):
```rust
#[cfg(debug_assertions)]
macro_rules! perf_debug {
    ($($arg:tt)*) => { log::debug!($($arg)*) };
}

#[cfg(not(debug_assertions))]
macro_rules! perf_debug {
    ($($arg:tt)*) => {};  // Zero overhead in release builds
}
```

**Usage**: Use `perf_debug!()` and `perf_trace!()` for hot-path logging that should be eliminated in production.

### 4. Accessibility rules that are easy to break by accident

Three of these were broken in this codebase and fixed on 19 August; they are
written down because each looks harmless in a diff.

**Never dim text with opacity.** Opacity composites against whatever sits
behind, so the same class gives a different contrast on a card than on the page
and no token can promise a figure. Use `text-disabled`, which is a real
variable generated from `text/disabled`. WCAG exempts disabled controls from the
contrast minimum, which is what lets that token sit at 3.25/2.99 — but the
exemption covers a control's own label, never text explaining why the control is
off. Putting an explanation inside something you are dimming is a design error,
not a contrast decision.

**Prefer `aria-disabled` to `disabled` when the control carries a reason.** A
`disabled` button leaves the tab order, so a screen-reader user meets a gap and
no explanation. Refuse the click in the handler instead.

**Twelve pixels is for badges, not for content.** `text-xs` carried error
messages here until it did not. Anything a person has to read is `text-sm` or
larger.

`prefers-reduced-motion` is honoured globally in `globals.css` by collapsing
durations rather than removing animations, so animation-end events still fire
for components that wait on them.

### 5. Frontend State Management

**App-level contexts**:
- `components/Sidebar/SidebarProvider.tsx` — despite the historical name, no
  sidebar: current meeting, the lightweight meetings list some flows update
  optimistically, the recording entry point, summary polling, and the active
  settings category (used by globally-mounted toasts)
- `contexts/RecordingStateContext.tsx` — the single source of truth for
  recording state, synced from the Rust backend
- `contexts/ConfigContext.tsx` — model/transcript config, preferences, beta flags

**Pattern**: Tauri commands update Rust state → Emit events → Frontend listeners update React state → Context propagates to components

## Common Development Tasks

### Adding a New Audio Device Platform

1. Create platform file: `audio/devices/platform/{platform_name}.rs`
2. Implement device enumeration for the platform
3. Add platform-specific configuration in `audio/devices/configuration.rs`
4. Update `audio/devices/platform/mod.rs` to export new platform functions
5. Test with `cargo check` and platform-specific device tests

### Adding a New Tauri Command

1. Define command in `src/lib.rs`:
   ```rust
   #[tauri::command]
   async fn my_command(arg: String) -> Result<String, String> { /* ... */ }
   ```
2. Register in `tauri::Builder`:
   ```rust
   .invoke_handler(tauri::generate_handler![
       start_recording,
       my_command,  // Add here
   ])
   ```
3. Call from frontend:
   ```typescript
   const result = await invoke<string>('my_command', { arg: 'value' });
   ```

### Modifying Audio Pipeline Behavior

**Location**: `frontend/src-tauri/src/audio/pipeline.rs`

Key components:
- `AudioMixerRingBuffer`: Manages mic + system audio synchronization
- `ProfessionalAudioMixer`: RMS-based ducking and mixing
- `AudioPipelineManager`: Orchestrates VAD, mixing, and distribution

**Testing Audio Changes**:
```bash
# Enable verbose audio logging
RUST_LOG=app_lib::audio=debug ./clean_run.sh

# Monitor audio metrics in real-time
# Check Developer Console in the app (Cmd+Shift+I on macOS)
```

### Tauri Backend Development

All app behaviour lives in the Rust/Tauri core. Add new frontend-facing
behaviour through Tauri commands and events, and through the existing Rust
services under `frontend/src-tauri/src`.

## Testing and Debugging

### Rust Test Suite

```bash
cd frontend/src-tauri
cargo test --lib                 # unit tests (all live in #[cfg(test)] modules)
cargo llvm-cov --lib --summary-only   # coverage; needs llvm-tools-preview
```

A fresh worktree must build the `llama-helper` sidecar once before any cargo
command succeeds, or the build fails on a missing resource path:

```bash
cd frontend && node scripts/build-sidecar.js
```

**The suite is expected to be green** — 502 passing, 3 ignored, as of 19 August
2026. It was not always: `test_calculate_buffer_timeout_bluetooth` failed on
`main` for a long time, and older notes here told readers to discount it. The
cause was `mul_f32(2.0)` round-tripping an f64 through single precision and
turning 160ms into 159.999996ms; doubling a `Duration` is integer arithmetic and
now does that instead. If a coverage gate you read elsewhere is phrased as "no
failures other than this one", the exception no longer applies.

**How coverage is judged**: not by a repo-wide percentage. Every file carries a
class — pure logic, replaceable-resource I/O, orchestration, or irreducible glue
— and each class has its own target, with glue requiring a written out-of-scope
justification instead of coverage. The method, the current per-module numbers,
and the standing audit findings are in
[frontend/src-tauri/TEST_COVERAGE_AUDIT.md](frontend/src-tauri/TEST_COVERAGE_AUDIT.md).
Read it before adding tests, so effort goes where it still finds defects.

**Test conventions in this codebase**:
- Databases in tests are in-memory SQLite with the real migrations applied; see
  `database/repositories/transcript.rs` for the harness to copy.
- Filesystem tests use `tempfile`. No test may touch a real user path
  (`~/Library/Application Support/Halvern`, `frontend/models`) or the network.
- When a test documents behaviour that looks wrong, the convention is to
  **freeze it** — assert today's behaviour with a comment saying so, and report
  it — rather than fix it in the same commit. Behaviour changes are separate,
  individually approved commits.

### Frontend Debugging

**Enable Rust Logging**:
```bash
# macOS
RUST_LOG=debug ./clean_run.sh

# Windows (PowerShell)
$env:RUST_LOG="debug"; ./clean_run_windows.bat
```

**Developer Tools**:
- Open DevTools: `Cmd+Shift+I` (macOS) or `Ctrl+Shift+I` (Windows)
- Console Toggle: Built into app UI (console icon)
- View Rust logs: Check terminal output

### Audio Pipeline Debugging

**Key Metrics** (emitted by pipeline):
- Buffer sizes (mic/system)
- Mixing window count
- VAD detection rate
- Dropped chunk warnings

**Monitor via Developer Console**: The app includes real-time metrics display when recording.

## Platform-Specific Notes

### macOS
- **Audio Capture**: Uses ScreenCaptureKit for system audio (macOS 13+)
- **GPU**: Metal + CoreML automatically enabled
- **Permissions**: Requires microphone + screen recording permissions
- **System Audio**: Requires virtual audio device (BlackHole) for system capture

### Windows
- **Audio Capture**: Uses WASAPI (Windows Audio Session API)
- **GPU**: CUDA (NVIDIA) or Vulkan (AMD/Intel) via Cargo features
- **Build Tools**: Requires Visual Studio Build Tools with C++ workload
- **System Audio**: Uses WASAPI loopback for system capture

### Linux
- **Audio Capture**: ALSA/PulseAudio
- **GPU**: CUDA (NVIDIA) or Vulkan via Cargo features
- **Dependencies**: Requires cmake, llvm, libomp

## Performance Optimization Guidelines

### Audio Processing
- Use `perf_debug!()` / `perf_trace!()` for hot-path logging (zero cost in release)
- Batch audio metrics using `AudioMetricsBatcher` (pipeline.rs)
- Pre-allocate buffers with `AudioBufferPool` (buffer_pool.rs)
- VAD filtering reduces Whisper load by ~70% (only processes speech)

### Whisper Transcription
- **Model Selection**: Balance accuracy vs speed
  - Development: `base` or `small` (fast iteration)
  - Production: `medium` or `large-v3` (best quality)
- **GPU Acceleration**: 5-10x faster than CPU
- **Parallel Processing**: Available in `whisper_engine/parallel_processor.rs` for batch workloads

### Frontend Performance
- Meetings list loads in pages of 100 with IntersectionObserver-driven
  incremental fetch; search is debounced 250ms and answered by SQLite FTS5
- Transcript rendering virtualized for large meetings
  (`VirtualizedTranscriptView`, @tanstack/react-virtual)
- BlockNote (the heaviest dependency) loads on demand on the workshop route

## Important Constraints and Gotchas

1. **Audio Chunk Size**: Pipeline expects consistent 48kHz sample rate. Resampling happens at capture time.

2. **Platform Audio Quirks**:
   - macOS: ScreenCaptureKit requires macOS 13+, needs screen recording permission
   - Windows: WASAPI exclusive mode can conflict with other apps
   - System audio requires virtual device (BlackHole on macOS, WASAPI loopback on Windows)

3. **Whisper Model Loading**: Models are loaded once and cached. Changing models requires app restart or manual unload/reload.

4. **No Separate Backend Dependency**: Meeting persistence, transcription, and LLM features are handled by the Tauri app. Do not reintroduce a server tier as a supported requirement — the app running without one is the product.

5. **Every hostname in the source is reviewed, and that is enforced.**
   `scripts/ci/check-network-hosts.sh` fails on any host not listed in
   `scripts/ci/allowed-hosts.txt` with a sentence saying what it receives.
   This is the guard that matters most here: the product's whole claim is
   that meetings stay on the machine, and the cheapest way to break it is one
   new hostname in a file nobody reads. Tests cannot catch that — a call to a
   new endpoint passes every test. If you cannot write the sentence, that is
   the finding, not the check being in your way.

6. **File Paths**: Use Tauri's path APIs (`downloadDir`, etc.) for cross-platform compatibility. Never hardcode paths.

7. **Audio Permissions**: Request permissions early. macOS requires both microphone AND screen recording for system audio.

8. **Some audio subsystems look live but are not wired.** Verify a call path
   before spending time on any of these — an audit already lost time to exactly
   this trap, ranking a device-classification bug as top priority before
   discovering its result is discarded:
   - `audio/device_detection.rs` classifies devices and computes buffer
     timeouts. The pipeline logs the result and throws it away
     (`pipeline.rs:907`, `let _ = (...)`, "can be used for adaptive
     buffering in the future").
   - `audio/ffmpeg_mixer.rs` is the only behavioural consumer of that
     classification, and `FFmpegAudioMixer` is **never constructed** by live
     code. The pipeline uses `ProfessionalAudioMixer` instead.
   - `audio/diagnostics.rs` exports five logging functions with **zero call
     sites** anywhere in the crate.
   - `RecordingManager::stop_recording` is dead; the live stop path is
     `stop_streams_and_force_flush` + `save_recording_only`. It is also the
     only caller of `raw_tap::finish()`, so the diagnostic tap never closes
     its files.
   - `attempt_device_reconnect` in `audio/recording_commands.rs` is a
     `#[tauri::command]` that is **not in `generate_handler!`**, and no
     frontend code invokes it. The live reconnect path is
     `RecordingManager::handle_device_reconnect`. The unreachable copy is
     where clippy's `await_holding_lock` points — a real deadlock shape in
     code nothing can call.
   - `audio/level_monitor.rs` measures microphone levels properly and
     `AudioLevelMonitor` is **never constructed**. What shipped instead was
     `simple_level_monitor`, emitting `sin(counter) * 0.8` as the level;
     deleted in `82254e8`. Wiring the real one first needs
     `Arc<Mutex<Vec<Stream>>>` solved — cpal's `Stream` is neither `Send` nor
     `Sync`. Until then the interface deliberately shows no meter, and
     `frontend/src/components/AudioLevelMeter.tsx` renders for nobody.
   - `PooledBuffer` (`audio/buffer_pool.rs`) is re-exported from
     `audio/mod.rs` and used nowhere. `AudioBufferPool` in the same file is
     live, used by `recording_state.rs`.
   - `useModelConfiguration` (`hooks/meeting-details/useModelConfiguration.ts`)
     is the frontend's entry in this list: two hundred lines that load, listen
     for and save the summary model configuration, called from nowhere. Found
     on 25 August while removing the `serverAddress` prop that was its only
     tie to the deleted server tier, and reported rather than deleted, like
     everything else in this list.

   The pattern behind all of these is worth naming: **a registered command, an
   export, or a `pub` item is not a call path.** Grep for the caller, and for
   Tauri commands check `generate_handler!` in `lib.rs` as well as the
   frontend `invoke(...)` sites. Two separate audits have now mis-ranked work
   by skipping that step.

   Full inventory, with dispositions still open, in
   [frontend/src-tauri/TEST_COVERAGE_AUDIT.md](frontend/src-tauri/TEST_COVERAGE_AUDIT.md).

9. **The transcription language selector does nothing while Parakeet is the
   engine.** `ParakeetProvider::transcribe` ignores the language argument — it
   logs `Parakeet doesn't support language preference '{}' yet` and carries on
   (`parakeet_provider.rs:31`). That is by design: Parakeet detects its own 25
   European languages and needs no hint, and every language picker already
   collapses to "Auto Detect" while it is active. The setting only reaches an
   engine that reads it once the provider is Whisper.

   Onboarding no longer hardcodes the engine. It asks which languages the user's
   meetings are in and calls `choose_engine` (`onboarding.rs:75`), which returns
   Parakeet only when every answer is inside its 25 and Whisper otherwise. The
   decision core is [`language.rs`](frontend/src-tauri/src/language.rs) — pure
   functions, no Tauri, 17 tests — and it owns the provider strings too. Use
   `PROVIDER_PARAKEET` / `PROVIDER_WHISPER` rather than writing them out: the
   Whisper one is `localWhisper`, not `whisper`, and a literal that disagrees
   passes onboarding and then fails at the first recording. See
   [docs/ONBOARDING_LANGUAGE_MODEL.md](docs/ONBOARDING_LANGUAGE_MODEL.md).

10. **The download button points at a release page, not at a file.**
    `www/index.html` used to carry the literal strings `DOWNLOAD_URL` and
    `SOURCE_URL`, waiting on the decision about where the public repository
    lives. Both now resolve to `github.com/hretheum/halvern`.

    The download CTA deliberately targets `/releases/latest` rather than
    `/releases/latest/download/Halvern_0.4.0_aarch64.dmg`. GitHub's direct
    form matches on the exact asset name, and Tauri puts the version in it, so
    a one-click link would break on the first patch release and break silently
    — the page still renders, the button still looks right, and it 404s. A
    stable direct link needs a version-less asset name first.

    The `download` attribute went with it: browsers ignore it cross-origin, so
    it was promising something it never did.

11. **The VAD redemption time is 400 ms everywhere, and was meant not to be.**
    `REDEMPTION_TIME_MS` in `audio/pipeline.rs` used to read
    `if cfg!(target_os = "macos") { 400 } else { 400 }`, under a comment saying
    macOS Core Audio wants 900 ms and Windows 400 ms. Both arms were the same,
    so the split never took effect and macOS has always run at the Windows
    value.

    Frozen deliberately, not fixed: the redemption time decides how long the
    detector waits before calling a pause the end of speech, so changing it
    changes where every transcript gets cut. `redemption_time_is_400ms_on_every_platform`
    asserts the shipping value so the next person to notice has to change it on
    purpose, with a real recording to justify the number.

12. **A microphone stream can open and then deliver nothing, or deliver
    silence, and neither used to be visible.** Both happened on 26 August with
    AirPods, on the released 0.1.1, and both are in the log:

    - A recording started at 11:46:24 and its first sample reached the pipeline
      at **11:49:47** — three minutes and twenty-three seconds of an open
      stream producing nothing, with not one line logged in between. The screen
      said "Listening for speech…" throughout, which was true and useless.
    - Two later recordings received samples immediately, at full rate, every
      value zero. In the log they appear only as `RMS preservation: 0.0%` on
      the resampler's first chunk.

    **Why this is not a device-enumeration problem, so nobody re-investigates
    that.** `list_audio_devices` and `default_input_device` build a fresh
    `cpal::default_host()` on every call and cache nothing, and a long-running
    process does follow the system default changing:
    `cargo run --example device_watch` caught a headset connecting and
    disconnecting over ten minutes, both times reporting the new defaults. The
    application's own log shows the same process (pid 4140, started 11:16)
    choosing `Mikrofon (MacBook Air)` at 11:43 and `To moje` at 11:46. What was
    stale was the *interface*: the device check ran once per mount and the
    device monitor only runs during a recording, so an idle application
    described the machine as it had been at launch. That is fixed; the two
    failures above are not.

    `audio/input_activity.rs` is the measurement that was missing. It counts,
    from the audio callback, how many samples each source delivered and how
    many crossed a floor just above zero — so "nothing arrived" and "zeroes
    arrived" are different numbers rather than the same blank screen. Atomics,
    no timestamps, no lock: the interface polls `audio_input_activity` and
    decides what a gap means. Ten seconds of nothing, or fifteen of silence,
    now produce a message naming the device.

    The remaining question is why Core Audio behaves this way with a Bluetooth
    headset in hands-free mode. The counters are what a next attempt should
    start from, along with the raw tap.

13. **`flex-1` without `min-h-0` is a scroll container that cannot scroll.** In
    a flex column `flex-1` leaves `min-height: auto`, so the box grows to its
    content instead of shrinking to the space available; `overflow-y-auto` then
    has nothing to scroll and the parent's `overflow-hidden` clips the rest
    with no way to reach it. `OnboardingContainer` had exactly this and the
    summary-model step lost its last models and its button, on 26 August, with
    no scrollbar to suggest anything was missing. `LibraryScreen`, the record
    and meeting-details screens and the app layout all carry `min-h-0`; check
    for it before concluding that a screen "just needs a bigger window".

## Repository-Specific Conventions

- **Language: English only, everywhere.** Identifiers, comments, doc comments, log
  messages, **and commit messages** — all English, with no exceptions. This repository
  is headed for public release, so a reader who does not speak Polish must be able to
  work in it, and `git log` is part of what they read. Planning documents live in the
  private `transcriptz` repository and may be in another language; **nothing** here is.
  User-facing UI copy is a separate matter, governed by the app's own localisation.
- **Logging Format**: Rust logs should include enough module context to diagnose app behavior
- **Error Handling**: Rust uses `anyhow::Result`, frontend uses try-catch with user-friendly messages
- **Naming**: Audio devices use "microphone" and "system" consistently (not "input"/"output")
- **Git Branches**:
  - `main`: the only long-lived line of development
  - `feat/*`, `fix/*`, `refaktor/*`: short-lived work branches, merged by
    fast-forward after their gates pass
- **One checkout, at `~/dev/halvern`.** There are no worktrees as of 26 August.
  The repository used to live at `~/dev/transcriptz/meetily` — inside the
  private planning repository, which had `meetily/` in its `.gitignore` with a
  note saying the fork has its own repository, so the nesting was leftover
  rather than intended — with a second, linked worktree at
  `~/dev/meetily-rust-coverage`. Both are gone. The move was a plain `mv` and
  needed no `git worktree repair`, because the linked worktree was removed
  first: a linked worktree points at its parent by absolute path in both
  directions, and moving either end breaks the pair.

  The two trees held 80 GB, of which 77 GB was `target/` and `node_modules`.
  Removing them took the machine from 41 GB free to 112 GB. Anything that only
  existed in the working tree because it is gitignored — the bakeoff's raw
  results, `prompts/`, the task reports — was copied across first; that is the
  check to repeat before deleting any checkout here.

  If a worktree is ever added again: builds from different branches share one
  app data directory keyed by the bundle identifier, so set
  `HALVERN_APP_SUFFIX` when a branch's schema could diverge from `main` (see
  `frontend/scripts/tauri-auto.js`). That is also how you exercise a fresh
  install without disturbing the real one — onboarding only appears when the
  data directory is empty. And a stale worktree on an old branch is a trap
  rather than clutter: two were removed on 19 August because they still built
  `com.meetily.ai`, which after the data migration means an empty library and a
  six-gigabyte re-download.

## Key Files Reference

**Core Coordination**:
- [frontend/src-tauri/src/lib.rs](frontend/src-tauri/src/lib.rs) - Main Tauri entry point, command registration
- [frontend/src-tauri/src/audio/mod.rs](frontend/src-tauri/src/audio/mod.rs) - Audio module exports
- [frontend/src-tauri/src/database/mod.rs](frontend/src-tauri/src/database/mod.rs) - Local database module

**Audio System**:
- [frontend/src-tauri/src/audio/recording_manager.rs](frontend/src-tauri/src/audio/recording_manager.rs) - Recording orchestration
- [frontend/src-tauri/src/audio/pipeline.rs](frontend/src-tauri/src/audio/pipeline.rs) - Audio mixing and VAD
- [frontend/src-tauri/src/audio/recording_saver.rs](frontend/src-tauri/src/audio/recording_saver.rs) - Audio file writing

**UI Components**:
- [frontend/src/app/page.tsx](frontend/src/app/page.tsx) - Library (home screen)
- [frontend/src/app/record/page.tsx](frontend/src/app/record/page.tsx) - Recording screen with live transcript
- [frontend/src/app/meeting-details/page.tsx](frontend/src/app/meeting-details/page.tsx) - Workshop (meeting view)
- [frontend/src/app/settings/page.tsx](frontend/src/app/settings/page.tsx) - Settings, six categories
- [frontend/src/components/AppShell/TopBar.tsx](frontend/src/components/AppShell/TopBar.tsx) - Shared top bar
- [frontend/src/components/Sidebar/SidebarProvider.tsx](frontend/src/components/Sidebar/SidebarProvider.tsx) - App-level meeting context (historical name)
- [frontend/src/lib/layout.ts](frontend/src/lib/layout.ts) - The two vertical rules every screen aligns to (16px margin, 72px content edge), and the trap that cost an afternoon: `GUTTER` is a width, so a wrapper that renders its own element makes the marginalia inline and the width is silently ignored. Read the header before changing any screen's horizontal spacing

**Brand and data paths**:
- [frontend/src-tauri/src/lib.rs](frontend/src-tauri/src/lib.rs) - `APP_DATA_DIR_NAME`, the single definition of the brand directory. It replaced two literals of differing case that resolved to one folder on macOS and two on Linux
- [frontend/src/components/brand/HalvernMark.tsx](frontend/src/components/brand/HalvernMark.tsx) - The gatehouse mark, inline SVG on `currentColor` so one file serves both themes
- [scripts/migrate-meetily-to-halvern.sh](scripts/migrate-meetily-to-halvern.sh) - Moves a Meetily-era installation onto the current names. Reports by default, writes only with `--apply`

**Speech engines and language**:
- [frontend/src-tauri/src/language.rs](frontend/src-tauri/src/language.rs) - Which engine a set of languages needs, and the canonical provider strings. Pure functions, no Tauri, so it is testable and is where language decisions belong
- [frontend/src-tauri/src/onboarding.rs](frontend/src-tauri/src/onboarding.rs) - The six-step plan, including which models a given answer downloads
- [frontend/src-tauri/src/whisper_engine/whisper_engine.rs](frontend/src-tauri/src/whisper_engine/whisper_engine.rs) - Whisper model management and transcription
- [frontend/src-tauri/src/parakeet_engine/parakeet_engine.rs](frontend/src-tauri/src/parakeet_engine/parakeet_engine.rs) - Parakeet, including the model download URLs. Both engines now fetch from HuggingFace; v3 used to come from upstream's own server
- [frontend/src-tauri/src/summary/summary_engine/models.rs](frontend/src-tauri/src/summary/summary_engine/models.rs) - The built-in lineup, their chat templates and their sampling presets. `sanitize_for_llama_helper` is the **only** place local sampling is decided — `generate_with_builtin` takes no sampling arguments, so the `temperature` threaded through `generate_meeting_summary` reaches the cloud providers and nothing else. `HALVERN_BENCH_GREEDY` overrides it there for measurement, and is unset in every normal build

**Measurement**:
- [docs/experiments/summary-model-bakeoff/](docs/experiments/summary-model-bakeoff/README.md) - How the summary models were compared: design, corpus, prompts, metrics, runbook and results. Read [results/REPORT.md](docs/experiments/summary-model-bakeoff/results/REPORT.md) before changing `language_score` — it records what was measured, what was not, and which cell is still open

**Process**:
- [docs/CONTRIBUTION_PROCESS.md](docs/CONTRIBUTION_PROCESS.md) - The CI pipeline and the five project guards, with the reasoning behind each. Read before changing anything under `scripts/ci/`
- [scripts/ci/guards.sh](scripts/ci/guards.sh) - Run this before pushing. Same script CI runs
- [scripts/ci/allowed-hosts.txt](scripts/ci/allowed-hosts.txt) - Every network host the source may name, each with the reason it may be contacted. Also the shortest honest answer to "where does this app connect"
- [CONTRIBUTING.md](CONTRIBUTING.md) - Branches, worktrees, testing conventions, commit style
- [SECURITY.md](SECURITY.md) - What counts as a vulnerability here, and what is explicitly out of scope

**Release**:
- [docs/SIGNING.md](docs/SIGNING.md) - Signing and notarizing the macOS build, done for 0.4.0 on 19 August. Two traps live here. `tauri.conf.json` deliberately does not name a signing identity — it used to carry `"signingIdentity": "-"`, which silently ignored every credential in the environment and signed ad-hoc. And **Tauri notarizes the app but not the disk image**, so the `.dmg` needs `notarytool submit` and `stapler staple` by hand after every build until `release.yml` does it. §3a covers the **second** signing key, the updater one: unrelated to Apple, unre-issuable, and the only key here whose loss permanently cuts existing installations off from updates
- [www/index.html](www/index.html) - The landing page. Self-contained, no build step, palette taken from the token pyramid
- [LAUNCH_READINESS.md](LAUNCH_READINESS.md) - What must be true before a public launch: blockers, standing risks, what is deliberately deferred, and the order of work
- [docs/OSS_LAUNCH.md](docs/OSS_LAUNCH.md) - Publishing the repository itself: why a new repo rather than a rename, what was cut from the tree and why, CodeRabbit and cubic.dev, and the missing update channel that matters more than any of it
- [docs/VERSIONING.md](docs/VERSIONING.md) - Why 0.1.0 and not 0.4.0, what "breaking" means for an app whose users own the data, the four files carrying the version, and the rule that nothing else may hardcode it

**Design**:
- [docs/ONBOARDING_LANGUAGE_MODEL.md](docs/ONBOARDING_LANGUAGE_MODEL.md) - Why onboarding must ask for the meeting language, and how that answer picks the transcription engine and the summary model
- [docs/DESIGN_SYSTEM_PLAN.md](docs/DESIGN_SYSTEM_PLAN.md) - The Halvern token pyramid, the state of the captured Figma views, and the order of work for branding the app and the future website
- [docs/DESIGN_TOKEN_MAPPING.md](docs/DESIGN_TOKEN_MAPPING.md) - Captured hex → Figma token → shadcn variable. The lookup tables that make the remaining rollout mechanical; also where the three unavoidable mismatches are recorded
- [design/tokens/halvern.tokens.json](design/tokens/halvern.tokens.json) - The exported pyramid. Edit the Figma collections and re-export; do not hand-edit

**Testing**:
- [frontend/src-tauri/TEST_COVERAGE_AUDIT.md](frontend/src-tauri/TEST_COVERAGE_AUDIT.md) - How coverage is judged, per-module targets, standing audit findings
- [frontend/src-tauri/src/detection/policy.rs](frontend/src-tauri/src/detection/policy.rs) - Reference example of the pure-logic pattern the audit's class A describes
- [frontend/src-tauri/src/database/repositories/transcript.rs](frontend/src-tauri/src/database/repositories/transcript.rs) - Reference example of the in-memory database test harness
