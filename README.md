<div align="center">
  <img src="design/brand/gatehouse.svg" width="72" height="72" alt="" />
  <h1>Halvern</h1>
  <p><strong>Meeting notes that never leave your machine.</strong></p>
  <p>
    <a href="https://opensource.org/licenses/MIT"><img src="https://img.shields.io/badge/License-MIT-blue" alt="MIT licence"></a>
    <img src="https://img.shields.io/badge/Platform-macOS%2014.4%2B-lightgrey" alt="macOS 14.4 or later">
    <img src="https://img.shields.io/badge/Silicon-Apple%20only-C07A3A" alt="Apple Silicon only">
    <a href="https://halvern.io"><img src="https://img.shields.io/badge/halvern.io-download-C07A3A" alt="Download at halvern.io"></a>
  </p>
</div>

---

Halvern records your meetings, transcribes them, and writes the summary — all
on your own hardware. No account, no upload, no per-minute billing. Out of the
box the transcription and summarisation models run locally and no part of a
meeting touches the network.

It can be pointed outward, and only by you: an external LLM provider for
summaries, or a remote transcription endpoint. Both are off until configured,
and [PRIVACY_POLICY.md](PRIVACY_POLICY.md) says exactly what each one sends.

## Why it exists

Every meeting tool that writes notes for you does it by sending your meetings to
somebody else's computer. For a lot of conversations — client calls, salary
discussions, anything under NDA — that is the wrong trade, and it is not a trade
most people are asked to make consciously.

Halvern does the same job without that step. That constraint is the product.

## The privacy claim, stated precisely

Claims like this are usually vague on purpose, so here is a version you can
check in the source:

- The telemetry endpoint is read from `option_env!("HALVERN_TELEMETRY_ENDPOINT")`
  — a **compile-time** variable. Unset at build time, it is `None`.
- The analytics client is only constructed `if config.enabled &&
  !config.api_key.is_empty()` (`analytics/analytics.rs`). With no key there is
  no client, and every send path returns early.
- Consent defaults to `false` in every branch of `AnalyticsProvider.tsx`.

A default build therefore sends nothing, and cannot be made to send anything
without being rebuilt with credentials.

## What it does

- **Records** microphone and system audio together, so both halves of a call
  are captured.
- **Detects meetings** — it can notice a call starting in Teams, Zoom or Meet
  and start recording, then propose stopping when the call ends.
- **Transcribes locally**, choosing the engine that fits the language: a fast
  model for the 25 European languages it covers, Whisper for the other ~75.
- **Summarises locally** with a built-in model, or through your own OpenAI,
  Anthropic, Groq, OpenRouter or Ollama endpoint if you prefer.
- **Exports** to Markdown or Obsidian.

## Status

Pre-release, and honest about it:

- **Apple Silicon only.** There is no Intel build and there is unlikely to be
  one: summarisation runs a local model, and without unified memory it is slow
  enough to be a different product rather than a slower one.
- **macOS 14.4 or later**, which is where the system-audio capture lives.
  Windows and Linux targets exist in the build configuration, inherited from
  upstream, but have not been exercised here — do not assume they work.
- Builds are **signed and notarized** as of 0.1.0, so Gatekeeper opens them
  without a warning. Verified by downloading onto a Mac that had never run it —
  the only check that proves anything, since Gatekeeper does not quarantine what
  was built locally.
- First run downloads a speech model and a summarisation model, together
  roughly 1.5–3 GB depending on the answers you give during setup.
- **0.1.0 is the first release**, signed and notarized, and the first build
  whose update path can be exercised at all — there was nothing for the updater
  to find until it existed.

See [LAUNCH_READINESS.md](LAUNCH_READINESS.md) for the full list of what is
solid, what is not, and what is deliberately deferred.

## Building

See [docs/BUILDING.md](docs/BUILDING.md). In short: `pnpm run tauri:build` from
`frontend/`.

The toolchain versions are pinned and not suggestions — `rust-toolchain.toml`
for Rust, `packageManager` and `engines` in `frontend/package.json` for pnpm
and Node. rustup and pnpm both honour those files, so the right versions
install themselves; a mismatched one fails in ways that do not look like
version problems. [docs/VERSIONING.md](docs/VERSIONING.md) explains why.

Developer-facing notes — architecture, conventions, the test suite, and the
traps worth knowing before changing the audio path — live in
[CLAUDE.md](CLAUDE.md).

## Origins and attribution

**Halvern is a fork of [Meetily](https://github.com/Zackriya-Solutions/meeting-minutes)
by [Zackriya Solutions](https://www.zackriya.com/), used under the MIT licence.**

Meetily is the reason this project exists rather than starting from an empty
directory: the recording pipeline, the Tauri shell and the local-first
architecture are theirs. Halvern diverges in interface, branding, language
handling and its own set of fixes, but the foundation is Meetily's work and the
copyright notice in [LICENSE.md](LICENSE.md) is theirs and stays.

If you want the original — including its Pro edition and its own community —
go to [Zackriya's repository](https://github.com/Zackriya-Solutions/meeting-minutes);
this fork is not affiliated with them, and issues here are not their problem.

## Acknowledgments

Inherited from upstream, and still true of this codebase:

- Code borrowed from [Whisper.cpp](https://github.com/ggerganov/whisper.cpp).
- Code borrowed from [Screenpipe](https://github.com/mediar-ai/screenpipe).
- Code borrowed from [transcribe-rs](https://crates.io/crates/transcribe-rs).
- **NVIDIA**, for the **Parakeet** speech model.
- [istupakov](https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx), for
  the ONNX conversion of Parakeet.

## Licence

MIT — see [LICENSE.md](LICENSE.md).
