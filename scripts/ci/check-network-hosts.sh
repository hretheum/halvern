#!/usr/bin/env bash
#
# Fails if the source names a network host that nobody has reviewed.
#
# This is the guard that matters most in this repository. Halvern's pitch is
# that meetings stay on the machine, and the cheapest way to break that is one
# new hostname in a file nobody was reading. Tests cannot catch it — a call to a
# new endpoint passes every test — and review catches it only if the reviewer
# happens to look at the right line.
#
# So the allowlist is the review. `scripts/ci/allowed-hosts.txt` records each
# host and the sentence explaining why it may be contacted; this script proves
# the source names nothing else.
#
# Written for bash 3.2, because that is what macOS ships and contributors are
# expected to run this before pushing rather than discover it in CI.
#
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."

ALLOWLIST="scripts/ci/allowed-hosts.txt"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Allowlist entries are bare tokens at column zero; the explanations are
# indented comment lines, so anything starting with whitespace or # is prose.
grep -Ev '^[[:space:]]*#|^[[:space:]]*$' "$ALLOWLIST" | tr -d ' \t' | sort -u > "$work/allowed"

# www/ is in scope because it is where an analytics script would go, and the
# landing page now promises in public that there is none. A guard that cannot
# see the page cannot protect the promise the page makes.
# `tauri.conf.json` is scanned by name because it holds addresses the source
# does not: the updater endpoint lives there, and a host reachable from the
# shipped app that no source file mentions is exactly what this guard exists
# to stop slipping through.
{
  grep -rhoE 'https?://[a-zA-Z0-9._-]+' frontend/src-tauri/src frontend/src www \
    --include='*.rs' --include='*.ts' --include='*.tsx' --include='*.html' \
    --include='*.js' --include='*.css' 2>/dev/null
  grep -hoE 'https?://[a-zA-Z0-9._-]+' frontend/src-tauri/tauri.conf.json 2>/dev/null
} | sed -E 's|https?://||' | sort -u > "$work/found"

comm -23 "$work/found" "$work/allowed" > "$work/unreviewed"

if [ -s "$work/unreviewed" ]; then
  echo "This change makes the source name a host nobody has reviewed:"
  echo
  while IFS= read -r h; do
    echo "    $h"
    grep -rn "$h" frontend/src-tauri/src frontend/src www \
      --include='*.rs' --include='*.ts' --include='*.tsx' --include='*.html' \
      --include='*.js' --include='*.css' | head -3 | sed 's/^/        /'
  done < "$work/unreviewed"
  echo
  echo "If the host belongs there, add it to $ALLOWLIST with a sentence saying"
  echo "what it receives and under what conditions. If you cannot write that"
  echo "sentence, that is the finding — not the check being in your way."
  exit 1
fi

# The reverse direction: an allowlist entry with no remaining call site is a
# stale permission, and stale permissions are how an allowlist quietly stops
# meaning anything. A warning rather than a failure, so that removing the last
# caller and its entry in one commit does not require a third.
comm -13 "$work/found" "$work/allowed" | while IFS= read -r ok; do
  echo "note: $ALLOWLIST still permits '$ok', which the source no longer names"
done

echo "network hosts: $(wc -l < "$work/found" | tr -d ' ') found, all reviewed"
