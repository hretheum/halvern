## What changes, and why

<!-- What is now true that was not before, and what made it necessary. If it
     fixes an issue, "Fixes #123". -->

## What you checked

<!-- Not a promise that it works — what you actually ran or watched. "Recorded a
     12-minute call with AirPods and confirmed the file ends at the stop" is
     worth more than a ticked box. -->

- [ ] `cd frontend/src-tauri && cargo test --lib` passes
- [ ] `./scripts/ci/guards.sh` passes
- [ ] Exercised by hand, if it touches audio, recording, or the interface

## Anything a reviewer would otherwise have to discover

<!-- Behaviour you deliberately left alone, a trap you hit, a decision that
     could reasonably have gone the other way. This section is the one that
     saves the most time. -->

---

<!-- Reminders, not obligations:

     - Comments and commit messages are English (CONTRIBUTING.md). CI checks it.
     - A new network host needs a line in scripts/ci/allowed-hosts.txt saying
       what it receives. CI checks that too.
     - Tests use tempfile or in-memory SQLite, never a real user path — the
       suite runs on the machine that records real meetings.
     - Behaviour that looks wrong gets frozen in a test and reported, not fixed
       in the same commit. -->
