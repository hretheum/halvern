# Versioning

Halvern is at **0.1.0**, and this is its first release under that name.

## Why not 0.4.0

The repository carried `0.4.0` because that is the version of *Meetily
Community Edition* it was forked from. Halvern has never shipped four
releases, and a version number that implies a history it does not have is a
small lie that gets repeated in every issue report and every release page.

Renumbering was free exactly once: before any public release existed. The
updater compares semver, so a user on 0.4.0 offered a 0.1.0 build would be
offered nothing at all — permanently. Doing this after a release would have
meant either living with the wrong number or stranding whoever downloaded
first.

## What the numbers mean here

Semantic versioning, but "breaking" has to be read for a desktop application
rather than a library. There is no API to break. What can break is somebody's
data.

**Major** — an older Halvern can no longer read what a newer one wrote. A
database migration that is not reversible, a change to the recordings folder
layout, a settings format that an earlier build refuses. If downgrading would
lose a meeting, it is major.

**Minor** — a feature, a new setting, a new model, a new export target.
Anything that adds without taking away.

**Patch** — a fix. No new setting, no new file on disk, nothing that changes
what a recording looks like when it lands.

While the version is `0.x`, **the formats can still move**, and a migration
may be one-way. That is the whole meaning of the leading zero and it is
stated here rather than assumed.

## Where the version lives

Four files, and they must agree:

| File | Why |
|---|---|
| `frontend/src-tauri/tauri.conf.json` | The one the bundler and the updater read. This is the source of truth. |
| `frontend/src-tauri/Cargo.toml` | The crate version, which `app.package_info().version` returns. |
| `frontend/package.json` | The npm package version. |
| `Cargo.lock` | Follows the crate; updated by any `cargo` command. |

**Nothing else should carry a version literal.** The interface reads it at
runtime — `getVersion()` from `@tauri-apps/api/app` on the front end,
`app.package_info().version` in Rust. Three places used to hardcode `0.4.0`,
including the `app_version` field in two analytics payloads, which would have
reported the wrong version for the rest of the product's life. They now ask.

Documentation may name a version when it is describing a specific release
("signed and notarized as of 0.1.0"). That is a statement about the past and
does not need to track.

## Releasing

1. Bump the four files together. The Release workflow checks they agree and
   stops if they do not, because a build that reports one version while the
   release page shows another is a bug reporters cannot see past.
2. `docs/SIGNING.md` for the signed, notarized build.
3. Run the Release workflow. It reads the version from `tauri.conf.json`,
   creates the tag and a draft release, builds, signs, notarizes — the disk
   image included — and uploads the bundle plus `latest.json`. Do not create
   the tag by hand: if `v<version>` already exists the workflow refuses, rather
   than inventing a fourth segment the updater cannot compare, which is what it
   used to do.
4. `latest.json` is what the updater reads. Publishing a release without it
   means existing installations see nothing — see `updater.rs`. The workflow
   fails rather than let that happen.
5. Publish the draft. A draft is downloadable by nobody and visible to no
   installation.
6. `gh workflow run pages.yml --ref main` — the site stamps the current
   version and download size from the latest release when it deploys, and it
   has no way to know a release happened. It cannot be triggered by the release
   event: the `github-pages` environment only accepts deployments from `main`,
   and a tag deploy would publish `www/` as it stood at the tag, reverting
   anything changed since.

A release that is not signed is not a release: Gatekeeper will refuse it on
any machine that did not build it, and the download page promises otherwise.
