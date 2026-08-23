#!/usr/bin/env bash
#
# Fails if Polish text reaches the source or the commit log.
#
# This repository is worked on in Polish and published in English. The rule is
# in CONTRIBUTING.md — identifiers, comments, doc comments, log messages and
# commit messages are English — and it is exactly the kind of rule that holds
# for months and then slips in one hurried commit, at which point it is in
# `git log` permanently.
#
# It matches Polish diacritics specifically rather than all non-ASCII, because
# this codebase's prose uses em dashes and ellipses deliberately and a check
# that cries wolf about typography gets switched off within a week.
#
# What it does not catch: Polish written without diacritics. Nothing automatic
# will; that is what review is for.
#
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."

POLISH='[ąćęłńóśźżĄĆĘŁŃÓŚŹŻ]'
status=0

# --- source
#
# String literals are stripped before the check, because Polish *data* is
# correct and Polish *prose* is not. The test fixtures are deliberately Polish —
# they are how the FTS5 bug that made an uppercase Ż unsearchable was caught —
# and so are the transliteration table in `export/markdown.rs` and the speaker
# labels in `lib/speaker-labels.ts`. Removing quoted spans leaves comments,
# identifiers and log message shells, which is exactly the prose the rule is
# about.
hits=$(find frontend/src-tauri/src frontend/src scripts \
        \( -name '*.rs' -o -name '*.ts' -o -name '*.tsx' -o -name '*.js' \
           -o -name '*.mjs' -o -name '*.sh' \) -type f 2>/dev/null \
       | grep -v 'scripts/ci/check-english-only.sh' \
       | while IFS= read -r f; do
           perl -CSD -ne '
             my $code = $_;
             $code =~ s/r#+".*?"#+//g;          # Rust raw strings
             $code =~ s/"(?:[^"\\]|\\.)*"//g;   # double-quoted
             $code =~ s/'"'"'(?:[^'"'"'\\]|\\.)*'"'"'//g;  # single-quoted
             $code =~ s/`[^`]*`//g;             # backticks: code or data named in prose
             print "$ARGV:$.:$_" if $code =~ /[\x{0105}\x{0107}\x{0119}\x{0142}\x{0144}\x{00F3}\x{015B}\x{017A}\x{017C}\x{0104}\x{0106}\x{0118}\x{0141}\x{0143}\x{00D3}\x{015A}\x{0179}\x{017B}]/;
           ' "$f"
         done || true)
if [ -n "$hits" ]; then
  echo "Polish text in source:"
  echo
  printf '%s\n' "$hits" | sed 's/^/    /'
  echo
  echo "Code, comments and log messages are English. A reader who does not speak"
  echo "Polish has to be able to work in this repository."
  echo
  status=1
fi

# --- commit messages, for whatever range this run covers. On a pull request
# GitHub sets BASE_SHA; locally, default to what is not yet on main.
base="${BASE_SHA:-$(git merge-base HEAD main 2>/dev/null || echo '')}"
if [ -n "$base" ]; then
  # Same distinction as the source check, by a different route. A commit
  # message has no string literals, but English prose does sometimes name a
  # Polish letter as data — "the bug that made an uppercase Ż unsearchable".
  # In real Polish prose the diacritics sit inside words; a standalone
  # single letter is a citation, so it is stripped before looking, along with
  # anything in backticks.
  # The letters are written as codepoints, not literally. With -CSD perl decodes
  # its INPUT as UTF-8 but leaves the PROGRAM TEXT as bytes, so a literal 'Ż' in
  # the pattern is two bytes and never matches the one decoded character in the
  # subject. It fails silently — the strip does nothing and the check looks
  # stricter than it is.
  bad=$(git log --format='%H %s%n%b' "$base..HEAD" 2>/dev/null \
        | perl -CSD -pe 's/`[^`]*`//g;
             s/(?<![[:alpha:]])[\x{0105}\x{0107}\x{0119}\x{0142}\x{0144}\x{00F3}\x{015B}\x{017A}\x{017C}\x{0104}\x{0106}\x{0118}\x{0141}\x{0143}\x{00D3}\x{015A}\x{0179}\x{017B}](?![[:alpha:]])//g' \
        | grep -E "$POLISH" || true)
  if [ -n "$bad" ]; then
    echo "Polish text in a commit message on this branch:"
    echo
    printf '%s\n' "$bad" | sed 's/^/    /'
    echo
    echo "git log is part of what a public reader gets. Reword with"
    echo "'git rebase -i' before merging — it cannot be fixed afterwards."
    echo
    status=1
  fi
fi

if [ "$status" -eq 0 ]; then
  echo "language: source and commit messages are English"
fi
exit "$status"
