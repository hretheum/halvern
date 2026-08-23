#!/usr/bin/env bash
#
# rustfmt, on files this change *adds*.
#
# The obvious rule — format every file you touched — was tried first and is
# wrong here. The tree is not rustfmt-clean, so changing one line in an existing
# file makes that whole file's formatting your problem; a rename touching 25
# files would demand reformatting all 25, producing exactly the blame-burying
# diff the rule was meant to prevent.
#
# New files have no such history to protect, so they must be clean. Existing
# ones produce a note. The tree converges as code is added and rewritten, which
# is slower than a mass reformat and costs nobody a review they did not sign up
# for — the same bargain as the clippy ratchet.
#
# BASE_SHA is set by CI to the pull request's merge base. Locally it defaults to
# the fork point from main.
#
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."

base="${BASE_SHA:-$(git merge-base HEAD main 2>/dev/null || echo '')}"
if [ -z "$base" ]; then
  echo "format: no base commit to compare against, skipping"
  exit 0
fi

added=$(git diff --name-only --diff-filter=A "$base..HEAD" -- '*.rs' | grep -v '/target/' || true)
modified=$(git diff --name-only --diff-filter=M "$base..HEAD" -- '*.rs' | grep -v '/target/' || true)

unformatted() {
  for f in $1; do
    [ -f "$f" ] || continue
    rustfmt --edition 2021 --check "$f" >/dev/null 2>&1 || printf ' %s' "$f"
  done
}

bad_new=$(unformatted "$added")
bad_old=$(unformatted "$modified")

if [ -n "$bad_old" ]; then
  echo "note: these existing files are not rustfmt-clean, which predates this"
  echo "      change. Tidying one is welcome as its own commit:"
  for f in $bad_old; do echo "        $f"; done
fi

if [ -n "$bad_new" ]; then
  echo
  echo "New files must be formatted:"
  echo
  for f in $bad_new; do echo "    $f"; done
  echo
  echo "    rustfmt --edition 2021$bad_new"
  exit 1
fi

n=$(printf '%s' "$added" | grep -c . || true)
echo "format: $n new Rust file(s), all formatted"
