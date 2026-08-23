# Security Policy

## Reporting a vulnerability

**Do not open a public issue for a security problem.** Halvern holds recordings
and transcripts of people's meetings; a public report is a working exploit
handed to everyone running an unpatched copy before there is a patch.

Halvern is not yet published and has no repository, so there is currently no
private reporting address. When it is published this section will name one —
GitHub private vulnerability reporting, plus an email address. Until then, if
you have found something, raise it with whoever gave you access to this code.

That is an unsatisfying answer and it is the true one. A security policy that
lists a contact nobody reads is worse than one that admits the gap.

## What we consider a vulnerability

Halvern's central claim is that meeting data stays on the machine. Anything that
breaks that claim is a security issue even if it is not a memory-safety bug:

- **Unintended egress.** Meeting content, transcript text, audio, titles, or
  participant names reaching any host, under any configuration the user did not
  explicitly set up. This is the highest-severity class in this project.
- **Data at rest exposed beyond the user.** The database, recordings, or API
  keys becoming readable by another account, another application, or a
  world-readable path.
- **Configuration that silently widens the trust boundary.** A default that
  enables remote transcription or an external summarization provider, an
  analytics endpoint compiled into a release, or a setting whose interface
  understates where data goes.
- **Path traversal and injection** in the places that take user-shaped input:
  template identifiers, export paths, obsidian vault paths, meeting titles that
  become filenames.
- **Supply chain.** A dependency, model download, or build step that could
  substitute code or model weights we did not intend to ship.
- Conventional memory safety, privilege escalation, and code execution.

## What is out of scope

- **Anything that requires the attacker to already be running as your user.**
  Halvern does not encrypt its database or its recordings, and says so in
  [PRIVACY_POLICY.md](PRIVACY_POLICY.md). Local files being readable by local
  code with your privileges is the operating system's threat model, not a
  Halvern bug. Use FileVault.
- **Data sent to a provider you configured.** If you set an OpenAI key, your
  transcripts go to OpenAI. That is the feature.
- Unsigned builds warning on first launch. Known, tracked in
  [LAUNCH_READINESS.md](LAUNCH_READINESS.md) §1.1.
- Reports from automated scanners with no demonstrated path to impact.

## Supported versions

Pre-release. Only the latest commit on `main` is supported; there are no
backports and no security branches. There is also currently no update mechanism,
so a fix reaches you only when you download a new build — see
[LAUNCH_READINESS.md](LAUNCH_READINESS.md) §2.5.

## What is already checked automatically

Some of this class of defect is caught before review rather than after release:

- `scripts/ci/check-network-hosts.sh` fails any change that makes the source
  name a network host not on a reviewed allowlist, with a written reason for
  each host. A new endpoint cannot be added quietly.
- `scripts/ci/check-test-isolation.sh` fails any test that resolves a real user
  directory, so the suite cannot reach the machine's own meeting database.

Neither replaces review. Both exist because the failures they catch are the
kind that pass every test and look like ordinary code in a diff.
