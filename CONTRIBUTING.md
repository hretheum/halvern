# Contributing to Halvern

Halvern is not yet published, so there is no public repository to open a pull
request against and no issue tracker to file against. This document describes
how the project is actually worked on, so that it is accurate when that changes
rather than describing a process nobody follows.

Until then, treat it as the house rules.

## Branches

- **`main`** is the only long-lived line of development.
- **`feat/*`, `fix/*`, `refaktor/*`** are short-lived work branches, merged by
  fast-forward once their gates pass.

There is no `devtest` branch. Earlier versions of this file, inherited from
upstream, told contributors to branch from one and to pull from Zackriya's
repository — neither applies here.

## Worktrees, and the one rule that matters

This repository is worked on through more than one git worktree at once, and
**one of them is used to record real meetings while the other is being
changed.** Never touch a worktree you were not pointed at.

A worktree left on an old branch is worse than clutter. Two were removed on 19
August because they still built the pre-rename bundle identifier, which after
the data migration means opening to an empty library and re-downloading six
gigabytes of models.

Builds from different branches share one application data directory, keyed by
the bundle identifier. If a branch changes the database schema, set
`HALVERN_APP_SUFFIX` so the build gets its own identifier, database and models
(see `frontend/scripts/tauri-auto.js`). Without it, the build that runs second
refuses to start: sqlx finds a migration in the database that its binary has
never heard of.

## Before you push

```bash
./scripts/ci/guards.sh              # the project-specific checks
cd frontend/src-tauri && cargo test --lib
```

`guards.sh` is the same script CI runs, so nothing is waiting in the pull
request that you could not have seen locally. It checks five things that no test
can catch, because each of them passes every test: a network host nobody
reviewed, a test that resolves a real user directory, Polish prose in code or
commit messages, a resurrected upstream path, and an unformatted new file.
[docs/CONTRIBUTION_PROCESS.md](docs/CONTRIBUTION_PROCESS.md) explains why each
one exists.

The suite is expected to be **green** — 502 passing, 3 ignored as of 19 August
2026. Any failure is yours. (An earlier note here excused
`test_calculate_buffer_timeout_bluetooth` as a known failure; it was a real
defect and has been fixed.)

A fresh worktree must build the sidecar once before any cargo command works:

```bash
cd frontend && node scripts/build-sidecar.js
```

How coverage is judged — by file class rather than by a repo-wide percentage —
is in [frontend/src-tauri/TEST_COVERAGE_AUDIT.md](frontend/src-tauri/TEST_COVERAGE_AUDIT.md).
Read it before adding tests, so the effort goes where it still finds defects.

## Testing conventions

- Databases in tests are in-memory SQLite with the real migrations applied. Copy
  the harness in `database/repositories/transcript.rs`.
- Filesystem tests use `tempfile`. **No test may touch a real user path**
  (`~/Library/Application Support/Halvern`, `~/Movies/halvern-recordings`,
  `frontend/models`) or the network.
- When a test documents behaviour that looks wrong, **freeze it**: assert what
  the code does today, say so in a comment, and report it. Changing the
  behaviour is a separate, separately approved commit.

## Language

**English only, everywhere** — identifiers, comments, doc comments, log messages
and commit messages. This repository is headed for public release, and `git log`
is part of what a reader gets. User-facing interface copy is a separate matter,
governed by the app's own localisation.

## Commit messages

```
<type>(<scope>): <subject>
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`.

Write the subject as a statement about the code, not about the work: what is now
true, not what you did. The body is for why the change was needed and what it
cost — especially anything the next person would otherwise have to rediscover.
Several commits in this history exist mainly to record a trap, and they are the
useful ones.

## Code style

Follow what is already there. Beyond that:

- Rust errors are `anyhow::Result`; the frontend catches and shows something a
  person can act on.
- Audio devices are "microphone" and "system" throughout, never
  "input"/"output".
- Use `perf_debug!()` / `perf_trace!()` on hot paths — they compile to nothing in
  release builds.
- Never hardcode a user path. Tauri's path APIs, or the constants in `lib.rs`.

Architecture, the audio pipeline, and the traps worth knowing before touching
any of it are in [CLAUDE.md](CLAUDE.md).

## Licence

Contributions are licensed under the project's MIT licence. Halvern is a fork of
[Meetily](https://github.com/Zackriya-Solutions/meeting-minutes) by Zackriya
Solutions; their copyright notice in [LICENSE.md](LICENSE.md) stays.
