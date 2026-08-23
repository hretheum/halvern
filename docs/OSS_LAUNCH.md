# Publishing Halvern

What has to happen before this repository is public, in the order it has to
happen, with the parts only a human can do marked as such.

Companion to [LAUNCH_READINESS.md](../LAUNCH_READINESS.md), which is about
whether the *product* is ready. This document is about the *repository* and the
launch itself.

Written 23 August 2026.

## The decision everything else follows from

**A new repository, `hretheum/halvern`, with a squashed history — not a rename
of `hretheum/meetily`.**

The existing repository is private, has zero stars, and — importantly — is not
a GitHub fork. That last part is worth keeping: forks are excluded from
repository search and from trending, so a project that starts life as a fork
button is invisible in the two places people discover things.

A rename would preserve the object store. GitHub keeps unreachable objects
addressable by SHA, and shares object storage between a repository and its
forks, so "cleaned history" achieved by force-pushing over a rename is not
cleaned in any sense that matters. A new repository is the only way old commits
actually stop existing publicly.

**Keep `hretheum/meetily` private rather than deleting it.** It holds the real
history — the fork point, every commit, the whole record of what changed and
why. That is the provenance document if anyone ever questions the lineage, and
it costs nothing to keep. Publish the squashed tree; keep the audit trail.

Squashing does not affect the MIT obligation. MIT requires the copyright notice
and permission notice to travel with copies of the software; it says nothing
about version control history. `LICENSE.md` carries Zackriya Solutions'
copyright and stays exactly as it is, and README's attribution section stays
with it.

## 1. Tree preparation — done

Committed on `main` ahead of publication:

| What | Why | Commit |
|---|---|---|
| 29 screenshots and demo GIFs from upstream deleted | `docs/` was 34.4 MB, of which 24 MB was two GIFs showing a different product under its own brand. Nothing linked to any of them. Now 652 KB. | `4b43a3a` |
| `backend/` deleted (40 files) | Upstream's FastAPI/Docker/whisper-server tier, archived and unsupported. The largest single source of "what is this project" confusion, and an invitation to file issues about a tier nobody here will fix. | `64c20f2` |
| `prompts/`, `docs/superpowers/` deleted | Working notes in Polish. `check-english-only` guards `src/` and `scripts/` only, so these would have shipped as the only Polish in an English repository. | `64c20f2` |
| Nine workflows deleted, three kept | Eleven files, one of which ran. See §2. | `a098e2b` |
| Release stopped calling itself Meetily | Draft releases were named `Meetily v…` and assets prefixed `meetily`. | `a098e2b` |
| Licence/Supabase injection removed from `build.yml` | See §2. | `a098e2b` |
| `DOWNLOAD_URL` / `SOURCE_URL` resolved | Both now point at the public repository. See gotcha 10 in CLAUDE.md for why the download button targets the release page rather than a file. | this commit |
| `.coderabbit.yaml` added | See §3. | this commit |

## 2. What was wrong in the release path

Three findings from the workflow triage, recorded because each was invisible
and each would have been public.

**The release built Windows.** README states the Windows target is inherited
from upstream and has not been exercised. The release matrix built it anyway,
so the download page would have offered a binary contradicting the page above
it. Dropped from the matrix; `build.yml` keeps its Windows and Ubuntu branches
for whenever one of them is actually tested.

**`build.yml` injected licence-validation credentials.** It passed
`MEETILY_RSA_PUBLIC_KEY`, `SUPABASE_URL` and `SUPABASE_ANON_KEY` into every
build — upstream's online licence checking for its Pro edition. This fork has
no licence code and no Supabase client, so they were inert. They are gone
anyway: on a public repository, a workflow naming `SUPABASE_URL` tells every
reader the app phones home to validate a licence, and README asks people to
verify the opposite. The privacy claim is the product; anything that reads as
contradicting it costs more than the code it removes.

**The release told the operator to upload to someone else's bucket.** The
summary step said to put `latest.json` in `s3://meetily-updates/`, for an
updater `tauri.conf.json` does not configure. It now says plainly that updates
are not wired.

That last one is not a workflow bug. It is §5.

## 3. CodeRabbit

Free for public OSS repositories. `.coderabbit.yaml` is committed and takes
effect when the app is installed on the repository — a human step, at
[coderabbit.ai](https://coderabbit.ai), after the repository is public.

The configuration's load-bearing line is the scope note at the top: **CodeRabbit
complements the deterministic gates and must not re-litigate them.** Five guards
and a clippy ratchet already fail a change mechanically. A bot that repeats them
trains contributors to skim its comments, which is how the one useful comment
gets missed.

What it is told to look for instead is what a machine cannot check:

- In `src/audio/**` — that a registered command, a `pub` item or a re-export is
  not proof of a call path, and that sampling, resampling, chunk size, VAD
  timing and mixing changes need evidence from a real recording however
  cosmetic the diff looks.
- In `src/analytics/**` — any change that constructs a client unconditionally,
  defaults consent to true, or moves the endpoint to a runtime value. README
  asks readers to verify those exact properties; a change there makes the
  README wrong.
- In `scripts/ci/**` — swallowed errors, `|| true` on a pipeline whose exit code
  matters. One of these scripts spent months counting warnings while `|| true`
  ate the exit code of the command producing them.
- In `frontend/src/**` — layout that must be correct at first paint depending on
  a client-side hook, and contrast that comes from opacity rather than a token.
- In `www/**` — any script tag, external request or cookie, since the page
  states in public that it carries none.

`knowledge_base` points at CLAUDE.md, CONTRIBUTING.md, SECURITY.md,
PRIVACY_POLICY.md and `docs/**`, so the bot reviews against this project's
stated conventions rather than generic ones.

**cubic.dev** generates a codebase wiki and gives the README a badge for it.
One badge, no maintenance, and for someone arriving cold it turns a large Rust
codebase into something navigable. Sign up after the repository is public and
add the badge alongside the existing ones.

## 4. Publication — done, 23 August 2026

`hretheum/halvern` is public and live. Description and the ten discovery
topics are set. The tree went up as a single parentless commit, built with
`git commit-tree` from the working tree rather than `git checkout --orphan`,
so the local branch was never switched and could not be left half-migrated.

Verified on a fresh clone: one commit, no parents, 509 files, 5.9 MB, nothing
else reachable.

One trap worth writing down, because it wasted a push: in zsh, `"$COMMIT:refs/heads/main"`
is not the refspec you typed. `:r` is a zsh modifier that strips an extension,
so the shell silently rewrites it and git reports a refspec that does not match
anything. Braces fix it — `"${COMMIT}:refs/heads/main"`.

Branch rules are a **ruleset** rather than classic branch protection: deletion
and force-push blocked, pull request with one approving review, code-owner
review, thread resolution required, and admin bypass. Two consequences worth
knowing. `CODEOWNERS` is `* @hretheum` and GitHub does not let anyone approve
their own pull request, so the maintainer's own PRs will always merge through
the bypass; Dependabot's will not, since it is the author and can be reviewed
normally. And the ruleset requires a review but **not a green CI**, so a red
pull request can still be merged — the required status checks are the piece
still to add.

### What publication immediately surfaced

Both were real, both predated the repository, and both were invisible until a
clean machine tried to build.

**`pnpm install --frozen-lockfile` had never worked on a clean checkout.**
`frontend/pnpm-workspace.yaml` is not a workspace definition — it carries
`overrides` and `minimumReleaseAgeExclude`, which from pnpm 10 are read there
instead of from the `pnpm` field in package.json. pnpm 8 sees the filename,
demands a `packages` list, and stops. It looked fine locally only because
`node_modules` predated the file.

The diagnosis was slower than it should have been because `packageManager`
said `pnpm@8.15.9`, and **pnpm honours that field by downgrading itself to
it** — so `pnpm --version` reported 8.15.9 even with 11.21.0 installed, and
even `npx pnpm@10` reported 8.15.9. Meanwhile `pnpm-lock.yaml` is
`lockfileVersion: '9.0'`, which pnpm 8 cannot write, and with no `pnpm` field
in package.json the ProseMirror overrides keeping BlockNote on one instance
were being ignored entirely.

**Then Node.** With pnpm at 11.21.0 the failure moved to
`ERR_UNKNOWN_BUILTIN_MODULE`: pnpm 11 declares `node: >=22.13` and the
workflows pinned Node 20, which GitHub is deprecating anyway.

The common shape is the lesson: two hard requirements that were real, unstated,
and discovered by a runner rather than by the person installing. Both are now
declared — `packageManager` for pnpm, `engines` for Node — so the next person
gets a version error instead of a stack trace.

## 5. The thing that matters more than any of this

**There is no update channel.** `tauri.conf.json` configures no updater
endpoint, so somebody who downloads 0.4.0 has no way to receive 0.4.1.
LAUNCH_READINESS.md 2.5 has said so for a while; publishing changes what it
costs.

A launch that goes well is precisely the scenario where this hurts. The version
people download in the first 48 hours is the version they judge the project on,
permanently, and the bug you find on day two reaches nobody. Every fix after
that lands for new downloads only, while the people who already arrived — the
ones who starred it — keep the broken build.

The machinery is half-present: `build.yml` already signs updater artifacts with
`TAURI_SIGNING_PRIVATE_KEY`. What is missing is an endpoint, and GitHub
Releases can serve `latest.json` directly — no S3, no bucket, no cost.

**Recommendation: wire the updater before publishing, or decide deliberately
that 0.4.0 is a one-shot release and say so on the download page.** Both are
defensible. Discovering it after a good launch is not.

## 6. What will actually earn stars

The honest version, since most advice in this area is generic.

**The strongest asset is already written.** README does something unusual: it
states the privacy claim and then names the exact lines to check — the
`option_env!` telemetry endpoint, the client constructed only with a key and
consent, the consent default. That invites verification, and someone will
verify it and say so publicly. A checkable claim outperforms any amount of
copy about being privacy-first.

**Second strongest: signed and notarized.** Most open-source Mac apps are not,
and "download, open, no Gatekeeper warning" is a real difference people notice
in the first thirty seconds.

**Being a fork is fine if you say it first.** It will be discovered. README's
attribution section is already generous and specific, which converts a
potential gotcha into a credibility signal.

**The gap: there is no demo.** Deleting upstream's GIFs was correct and left a
hole. The first screen of the README decides most of the outcome, and a README
without a moving picture of the product working loses most visitors before the
privacy argument is ever read. **This is a task, not a detail.** One 10–15
second GIF: recording starts, transcript appears live, summary lands.

**The narrowing:** Apple Silicon, macOS 14.4+, and a 1.5–3 GB first-run
download. None are fixable by launch; all should be stated plainly above the
fold, because a surprised user writes a bad comment and an informed one does
not.

**Where to post**, in the order that matters: Show HN, with the verification
instructions in the post itself rather than a link. Then `r/macapps`,
`r/selfhosted`, `r/privacy`, and `r/rust` for the Tauri angle. The landing page
already carries the analytics to tell you which of them worked, and the privacy
policy already discloses that it does.

Post mid-week, morning US Eastern. Be present in the thread for the first six
hours — on Show HN, the author answering questions is a larger factor than the
submission itself.

## 7. The recovery store, renamed and the old one deleted

`MeetilyRecoveryDB` is now `HalvernRecoveryDB`, and the old store is deleted
on first init.

The earlier draft of this document argued for leaving it, on the grounds that
renaming makes unsaved transcripts invisible. That reasoning was calibrated for
a user base that does not exist. The people with data in that store are the
maintainer's own machines; a new user opens an empty database and never sees
the name. **Publication is the last moment this costs nothing** — afterwards the
same rename needs a migration maintained forever, in the recovery path, for an
event that happens once.

Two facts settled it.

The store already expires. `RecoveryPrompt` prunes at every startup that is not
a live recording: `deleteOldMeetings(7)` and `deleteSavedMeetings(24)`. Nothing
in there survives seven days, because seven days is the ceiling this app
already applies to its own recovery data.

And the deletion matters more than the rename. A renamed database is not an
upgraded one — `DB_VERSION` and `onupgradeneeded` work within a single name, so
the old store is simply never opened again, which means never pruned again.
Left in place it would hold meeting transcripts on disk indefinitely, in a
database nothing opens and nothing manages. For a product whose entire claim is
that you decide what happens to your meetings, that is a worse artifact than
the branding the rename was fixing.

Deleted rather than migrated, deliberately: a migration is that same deletion
plus copying, which is more code in the one path that runs only after something
has already gone wrong, for a benefit that accrues to one person with backups
elsewhere. `dropSupersededDatabase()` is fire-and-forget and silent by design —
cleanup that cannot complete must never delay or fail the recovery it is
cleaning up after, and deleting a database that is already gone succeeds.

`check-brand.sh` now catches `meetily_[a-z_]+` and `Meetily[A-Z][A-Za-z]*` as a
class, exempting only the one line that names the superseded store in order to
delete it. Verified by reintroducing `meetily_user_id` and watching the guard
fail, then restoring.

## 8. Where the landing page is hosted, and why that was not a free choice

**GitHub Pages**, deployed from `www/` by `.github/workflows/pages.yml`.

The page has no `<script>` tag and says so in public: no analytics script, no
cookies, no fingerprinting, nothing running in the reader's browser. That
sentence rules out most of the obvious hosts, because their analytics *is* a
browser beacon — Vercel Web Analytics, Cloudflare Web Analytics, Plausible,
Fathom, Umami. Adding any of them would make the page's own text false, on the
one page whose entire job is to be checkable.

Cloudflare Pages was the first recommendation and was withdrawn. Not for the
analytics beacon, which is optional, but for the interstitial: the default
security level challenges visitors by IP reputation, which lands on VPN and Tor
users. That is disproportionately this product's audience, and a privacy-first
site asking a VPN user to prove they are human is a comment thread nobody wins.

That left the real trade. **Netlify Analytics** reads Netlify's own request
logs — server-side, no script, no cookies, roughly the fields the page
describes — for about $9 a month. **GitHub Pages** is free and never
challenges anyone, but hands you no logs at all.

GitHub Pages wins for now on one observation: **the number actually wanted is
free from somewhere else.** The download button points at GitHub Releases, so
GitHub hands over the file, and `download_count` per release asset is in a
public API. Netlify would be bought purely for "where from" and "in what
language" — worth paying for once there is traffic, and worth nothing before
then. Revisit after launch, not before.

The page's own copy was rewritten to match, and this is worth stating plainly
because it was wrong for a day: the "What this page collects" section used to
say the server handing you the file sees your request. Once the download button
resolved to GitHub Releases, that server became GitHub. The section now says
GitHub serves both the page and the file, that those logs are GitHub's and we
never see them, and that the only thing readable on our side is an aggregate
download count. **The copy is coupled to the host** — moving to Netlify means
rewriting it again, in the same commit as the move.

`pages.yml` refuses to deploy an `index.html` containing a `<script>` tag. The
page's promise is enforceable, so it is enforced.

## 9. Deliberately not before launch

- Trimming `build.yml`'s unused Windows and Ubuntu branches. It is the only
  workflow that produces a release, and it cannot be tested here.
- The 24 remaining clippy warnings. `too_many_arguments` and `module_inception`
  are API and file-structure changes; the ratchet already prevents growth.
- The remaining items in LAUNCH_READINESS.md §2, except 2.5 above.
