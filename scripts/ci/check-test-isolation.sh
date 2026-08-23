#!/usr/bin/env bash
#
# Fails if a test reaches for a real user path instead of a temporary one.
#
# CONTRIBUTING.md has said "no test may touch a real user path" for as long as
# there have been tests, and nothing enforced it. The failure mode is not a red
# build — it is a green one. A test that writes into the real application data
# directory passes, and takes the developer's own meeting database with it.
# Whoever runs the suite on the machine they record meetings on finds out first,
# and by then it has already happened.
#
# `tempfile` is the convention; `database/repositories/transcript.rs` is the
# in-memory SQLite harness to copy.
#
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."

# Paths that belong to a real installation. Written as fragments because the
# spelling varies: a literal, a `dirs::data_dir()` join, an env expansion.
FORBIDDEN='Application Support/Halvern|Application Support/io\.halvern|Movies/halvern-recordings|\.config/Halvern|dirs::data_dir\(\)|dirs::video_dir\(\)|dirs::home_dir\(\)|dirs::config_dir\(\)'

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Only inside #[cfg(test)] blocks. Production code is *supposed* to resolve
# these directories; that is its job. awk tracks the attribute and the brace
# depth of the module it opens, so the scan covers the whole test module rather
# than the one line after the attribute.
find frontend/src-tauri/src -name '*.rs' -print0 | while IFS= read -r -d '' f; do
  awk -v file="$f" -v pat="$FORBIDDEN" '
    /^[[:space:]]*#\[cfg\(test\)\]/ { armed = 1; next }
    armed && /\{/ { intest = 1; armed = 0; depth = 0 }
    intest {
      n = gsub(/\{/, "{"); depth += n
      n = gsub(/\}/, "}"); depth -= n
      if ($0 ~ pat && $0 !~ /^[[:space:]]*\/\//) print file ":" FNR ":" $0
      if (depth <= 0) intest = 0
    }
  ' "$f"
done > "$work/hits" || true

if [ -s "$work/hits" ]; then
  echo "A test reaches for a real user path:"
  echo
  sed 's/^/    /' "$work/hits"
  echo
  echo "Tests must use tempfile, or in-memory SQLite. Copy the harness in"
  echo "frontend/src-tauri/src/database/repositories/transcript.rs."
  echo
  echo "This is not pedantry: the suite is run on the machine that records real"
  echo "meetings, and a test that resolves the real data directory writes there."
  exit 1
fi

echo "test isolation: no test resolves a real user path"
