#!/usr/bin/env bash
#
# Fails if the upstream brand comes back into code or configuration.
#
# The rename touched the bundle identifier, three data directories, the log
# process name and five build variables. Some of those were already stale when
# they were found — the console helper had been filtering for a process that no
# longer existed, and nobody noticed because nothing checks.
#
# Anything reintroducing those names is almost certainly a copy-paste from
# upstream, an old branch, or a merge that resurrected a file. Documentation is
# exempt: attribution names Meetily on purpose, and so does this repository's
# own history.
#
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."

# Names that must not reappear. `Meetily` on its own is not here: it is legal
# attribution and appears correctly in comments about the fork's origin. These
# are the operational ones — paths, identifiers, and build variables — where any
# occurrence is a defect.
FORBIDDEN='com\.meetily\.ai|meetily-recordings|MEETILY_(TELEMETRY|LLAMA_HELPER|APP_SUFFIX)|meetily\.towardsgeneralintelligence\.com|Application Support/Meetily|process meetily|meetily_[a-z_]+|Meetily[A-Z][A-Za-z]*'

# Storage keys and database names are operational identifiers too, and the
# pattern above catches them. One name is allowed to appear, in exactly one
# place: `indexedDBService.ts` names the superseded IndexedDB store so that it
# can delete it. Removing that reference would strand meeting transcripts on
# disk in a database nothing opens and nothing prunes.
EXEMPT='SUPERSEDED_DB_NAME|superseded recovery store'

hits=$(grep -rnE "$FORBIDDEN" \
        frontend/src-tauri/src frontend/src frontend/scripts scripts \
        frontend/src-tauri/tauri.conf.json frontend/src-tauri/Cargo.toml \
        frontend/package.json \
        --include='*.rs' --include='*.ts' --include='*.tsx' --include='*.js' \
        --include='*.mjs' --include='*.json' --include='*.toml' --include='*.sh' \
        2>/dev/null | grep -v '^scripts/ci/' | grep -v 'migrate-meetily-to-halvern' | grep -vE "$EXEMPT" || true)

if [ -n "$hits" ]; then
  echo "An upstream name is back in code or configuration:"
  echo
  printf '%s\n' "$hits" | sed 's/^/    /'
  echo
  echo "These are paths and build variables, not attribution. The current names:"
  echo "    bundle identifier   io.halvern.app        (tauri.conf.json)"
  echo "    brand directory     APP_DATA_DIR_NAME     (lib.rs)"
  echo "    recordings folder   RECORDINGS_FOLDER_NAME (audio/recording_preferences.rs)"
  echo "    build variables     HALVERN_*"
  echo
  echo "Naming Meetily in a comment or in documentation is fine and often"
  echo "required — the fork attribution is a licence obligation."
  exit 1
fi

echo "brand: no upstream identifiers, paths or build variables in code"
