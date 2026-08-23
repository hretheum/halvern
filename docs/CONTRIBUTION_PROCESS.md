# The contribution process

How a change gets from someone's clone into `main`, what checks it meets, and
why each one exists. [CONTRIBUTING.md](../CONTRIBUTING.md) is the short version
for contributors; this is the design and its reasoning, for whoever maintains
the process itself.

## What it was before

Worth recording, because it explains the shape of what replaced it.

Every workflow in `.github/workflows/` was `workflow_dispatch` — manual only.
That included the one named `pr-main-check`, which despite the name reads the
version out of `tauri.conf.json` and never compiles anything. **Nothing ran on a
pull request.** A contributor could open one, see a clean page, and learn days
later that it does not build.

Meanwhile the repository had accumulated real rules — in `CLAUDE.md`, in
`CONTRIBUTING.md`, in code comments — and no way to enforce any of them. Some had
already been broken without anyone noticing: the console helper had been
filtering log output for a process name that stopped existing when the crate was
renamed, and the brand directory was being written under two different spellings
that macOS silently merged into one.

That is the pattern the design responds to. The rules were good. The gap was
that a rule nobody can check is a rule that decays.

## The pipeline

Three jobs, cheapest first, so an obvious mistake fails in under a minute rather
than after a macOS runner has spent fifteen.

| Job | Runner | What it proves |
|---|---|---|
| `guards` | ubuntu | The project-specific rules hold |
| `rust` | macOS | 502 tests pass; clippy has not got worse |
| `frontend` | ubuntu | Types check; the app builds |

`rust` runs on macOS deliberately. The audio path is macOS-specific —
ScreenCaptureKit capture and the CoreAudio tap have no Linux equivalent — so a
green Linux build would prove nothing about the code that actually runs.

## The guards

The interesting part, and the part that is specific to this project rather than
copied from a template. Each one catches a failure that is **invisible**: it
passes every test, and looks like ordinary code in a diff.

All five run from `./scripts/ci/guards.sh`, which is the same script CI runs.
Contributors are expected to run it before pushing; there is nothing waiting in
the pull request that they could not have seen locally. That is the whole reason
they are scripts rather than workflow steps.

### `check-network-hosts` — the one that matters

Halvern's claim is that meetings stay on the machine. That claim is only as good
as the last commit, and the cheapest way to break it is one new hostname in a
file nobody was reading. No test catches it: a call to a new endpoint passes
everything.

So the source may only name hosts listed in
[`scripts/ci/allowed-hosts.txt`](../scripts/ci/allowed-hosts.txt), and each entry
carries a sentence saying what that host receives and under what conditions. The
allowlist **is** the review. Adding a host is allowed; adding one silently is
not.

The file is also the best short answer to "where does this app connect": fourteen
hosts, each with its reason, including the one that is present in source but
unreachable in a shipped build and why.

The scan covers `www/` as well as the app. It did not at first, which left the
one directory where an analytics script would plausibly be added outside the
guard — and that directory now carries a public promise that none exists. A
guard that cannot see the landing page cannot protect the claim the landing page
makes.

### `check-test-isolation`

`CONTRIBUTING.md` has said "no test may touch a real user path" for as long as
there have been tests, and nothing enforced it. The failure mode is not a red
build — it is a green one. A test that writes into the real application data
directory passes, and takes the developer's own meeting database with it. This
repository is worked on in more than one worktree at once and one of them
records real meetings, so the exposure is not hypothetical.

### `check-english-only`

The repository is worked on in Polish and published in English. `git log` is
permanent, so this is the rule that cannot be fixed later.

It took two attempts to make honest. The first version flagged all Polish
characters and produced 70 hits, most of them correct code: the test fixtures
are deliberately Polish — they are how the FTS5 bug that made an uppercase `Ż`
unsearchable was caught — and so are the transliteration table and the speaker
labels. Flagging those would have got the check switched off within a week.

The version that shipped strips string literals, raw strings and backticked
spans before looking, which separates Polish **data** from Polish **prose**. It
found 23 genuine comment lines, since translated. The backtick rule is also the
escape hatch: if an English comment needs to name a Polish letter, quote it.

It does not catch Polish written without diacritics. Nothing automatic will;
that is what review is for.

### `check-brand`

Paths, identifiers and build variables from before the rename. Any reappearance
is a copy-paste from upstream or a merge that resurrected a file. Naming Meetily
in a comment or in documentation stays fine — the fork attribution is a licence
obligation, not a slip.

### `check-format`

The obvious rule — format every file you touched — was tried and is wrong here.
The tree is not rustfmt-clean, so changing one line in an existing file makes
that whole file's formatting your problem; a rename touching 25 files would
demand reformatting all 25, which is exactly the blame-burying diff the rule was
meant to prevent.

New files must be clean. Existing ones get a note. The tree converges as code is
added, which is slower than a mass reformat and costs nobody a review they did
not sign up for.

## The clippy ratchet

Same bargain, stated as a number. The code was inherited with 135 warnings.
`-D warnings` on an inherited codebase offers two bad options: block every pull
request until someone does a cleanup nobody asked for, or disable the lint and
lose it permanently.

So [`scripts/ci/clippy-baseline.txt`](../scripts/ci/clippy-baseline.txt) holds
the count, and it may only go down. New warnings fail; the inherited ones cost a
contributor nothing. Lowering the count also fails until the baseline is
updated, so every improvement is a visible commit rather than a silent drift.

## What is deliberately not automated yet

Recorded so the absence is a decision rather than an oversight.

- **A linter for the frontend.** There is no ESLint configuration and no ESLint
  dependency; `next lint` was removed in Next 16. Adding one to an inherited
  codebase produces a wall of findings that would need its own ratchet. Worth
  doing, not worth blocking on.
- **Supply-chain scanning** (`cargo-deny`, `cargo-audit`, CodeQL). The highest
  value of these three for this product is `cargo-deny`, because a dependency
  that phones home defeats the same claim `check-network-hosts` protects — and
  the guard reads first-party source only, dependencies never.
- **Coverage as a gate.** Coverage here is judged per file class, not as a
  repository percentage
  ([TEST_COVERAGE_AUDIT.md](../frontend/src-tauri/TEST_COVERAGE_AUDIT.md)). A
  blunt percentage gate would contradict that method, and encoding the real one
  is a project of its own.
- **Release automation.** `release.yml` has no macOS signing step, and the
  signing identity is hardcoded to `-` in `tauri.conf.json`. That is a launch
  blocker tracked in [LAUNCH_READINESS.md](../LAUNCH_READINESS.md) §1.1, not a
  CI improvement.
- **A changelog bot.** Not useful until there are releases and users.

## Known defects in the inherited workflows

Found while designing this, not fixed here:

- `beforeBuildCommand` is `pnpm build` and never builds the `llama-helper`
  sidecar, but Tauri resolves `externalBin` at build-script time. Every existing
  build workflow would therefore fail on a clean runner with
  `resource path 'binaries/llama-helper-...' doesn't exist`. The new `ci.yml`
  runs `node scripts/build-sidecar.js` explicitly; the release workflows still
  need the same step.
- Two workflows reference a `devtest` branch that does not exist here.
- Artifact names are still `meetily-*`, and `release.yml` points at
  `s3://meetily-updates/` — upstream's release infrastructure, which this
  project does not use.

## Status

The guards, the ratchet and the templates were all run locally and behave as
described, including against deliberately planted violations. **`ci.yml` itself
has never executed**, because that requires pushing to GitHub — the runner
setup is modelled on the existing `build-macos.yml`, which is the closest thing
to a proven configuration this repository has, but treat the first run as the
real test.
