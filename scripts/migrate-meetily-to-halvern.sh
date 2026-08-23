#!/usr/bin/env bash
#
# Moves an existing Meetily-era installation onto the Halvern names.
#
# The rename in 0.1.0 changed four things that the filesystem remembers:
#
#   bundle identifier   com.meetily.ai   -> io.halvern.app
#   brand data folder   Meetily/meetily  -> Halvern
#   recordings folder   meetily-recordings -> halvern-recordings
#   log folder          com.meetily.ai   -> io.halvern.app
#
# Everything the app owns is keyed off those, so without this script a build of
# 0.1.0 starts with an empty library, re-downloads 5-6 GB of models, and leaves
# the old data orphaned on disk. Nothing is deleted here — directories are
# renamed in place, which on one volume is instant regardless of size.
#
# The recordings folder is the one that cannot be renamed on its own: every
# meeting stores an ABSOLUTE folder_path in the database and the save location
# is repeated in recording_preferences.json. Renaming the folder without
# rewriting both leaves every meeting pointing at a path that no longer exists.
# So the three happen together or not at all.
#
# Deliberately untouched: ~/Library/Application Support/pro.meetily.ai. That is
# upstream's Meetily Pro, a different application that happens to share the
# name. Adopting its data would be taking something we were not given.
#
# One caveat found by running this, not by reading it: Meetily Pro saves into
# the SAME recordings folder, and stores absolute paths of its own. Renaming the
# folder therefore leaves Pro's meetings pointing at a path that is gone. No
# audio is lost — it moved with everything else — but if you still use Pro,
# repoint its database too:
#
#   sqlite3 ~/Library/Application\ Support/pro.meetily.ai/meeting_minutes.sqlite \
#     "UPDATE meetings SET folder_path =
#        replace(folder_path, 'meetily-recordings', 'halvern-recordings');"
#
# That is left as a manual step on purpose. Writing to another application's
# database without being asked is exactly what the paragraph above refuses.
#
# Usage:
#   scripts/migrate-meetily-to-halvern.sh          # show what would happen
#   scripts/migrate-meetily-to-halvern.sh --apply  # do it
#
set -euo pipefail

APPLY=0
[[ "${1:-}" == "--apply" ]] && APPLY=1

SUPPORT="$HOME/Library/Application Support"
LOGS="$HOME/Library/Logs"
MOVIES="$HOME/Movies"

OLD_ID="com.meetily.ai"
NEW_ID="io.halvern.app"
OLD_BRAND="Meetily"
NEW_BRAND="Halvern"
OLD_REC="meetily-recordings"
NEW_REC="halvern-recordings"

OLD_REC_PATH="$MOVIES/$OLD_REC"
NEW_REC_PATH="$MOVIES/$NEW_REC"

planned=0
skipped=0
STAMP=$(date +%Y%m%d-%H%M%S)

say()  { printf '%s\n' "$*"; }
step() { printf '\n\033[1m%s\033[0m\n' "$*"; }

# Does this directory hold anything the user would miss?
#
# Launching 0.1.0 once before migrating is the normal order of events, and it
# creates the destination as an empty scaffold: a database with no meetings and
# model directories with no models. Refusing to continue in that case would send
# people to delete folders by hand, which is how real data gets lost. So the
# test is specific rather than "is the directory empty": a destination that
# holds meetings is a real installation and stops the migration; one that holds
# none is scaffolding and gets moved aside, never deleted.
holds_meetings() {
  local db="$1/meeting_minutes.sqlite" count
  # No database at all — logs and the brand folder never have one — means
  # nothing irreplaceable is here.
  [[ -f "$db" ]] || return 1
  count=$(sqlite3 "$db" "SELECT COUNT(*) FROM meetings;" 2>/dev/null || echo 0)
  [[ "$count" != "0" ]]
}

# Rename a directory, or explain why it is being left alone.
move_dir() {
  local from="$1" to="$2" label="$3"
  if [[ ! -e "$from" ]]; then
    say "  skip  $label — $from does not exist"
    skipped=$((skipped + 1))
    return
  fi
  if [[ -e "$to" ]]; then
    if holds_meetings "$to"; then
      say "  STOP  $label — destination already holds meetings: $to"
      say "        That is a real installation, not scaffolding. Refusing to merge"
      say "        two libraries. Inspect both and move by hand."
      exit 1
    fi
    local aside="$to.scaffold-$STAMP"
    say "  aside $label — destination exists but holds no meetings"
    say "        $to"
    say "     -> $aside   (kept, not deleted — delete it yourself once satisfied)"
    if (( APPLY )); then mv "$to" "$aside"; fi
  fi
  local size
  size=$(du -sh "$from" 2>/dev/null | cut -f1 || echo '?')
  say "  move  $label ($size)"
  say "        $from"
  say "     -> $to"
  planned=$((planned + 1))
  if (( APPLY )); then mv "$from" "$to"; fi
}

# --- guard: the app must not be running, or it will write to the old paths
# while we move them and re-create half of them behind us. Only a real run is
# blocked; a dry run writes nothing and is exactly what you want to read while
# deciding whether to quit the app.
if (( APPLY )) && { pgrep -x halvern >/dev/null 2>&1 || pgrep -x Halvern >/dev/null 2>&1 \
     || pgrep -f 'Halvern.app' >/dev/null 2>&1; }; then
  say "Halvern is running. Quit it first — a live app rewrites these paths"
  say "while the migration moves them."
  exit 1
fi

if (( APPLY )); then
  step "APPLYING — directories will be renamed"
else
  step "DRY RUN — nothing is written. Re-run with --apply to perform the migration."
fi

step "1. Per-installation data (database, models, preferences)"
move_dir "$SUPPORT/$OLD_ID" "$SUPPORT/$NEW_ID" "app data"

step "2. Brand data (custom templates, notification settings)"
# macOS and Windows treat Meetily/meetily as one directory; Linux does not.
# Whichever spelling exists here, it ends up as Halvern.
if [[ -e "$SUPPORT/$OLD_BRAND" ]]; then
  move_dir "$SUPPORT/$OLD_BRAND" "$SUPPORT/$NEW_BRAND" "templates + notifications"
elif [[ -e "$SUPPORT/meetily" ]]; then
  move_dir "$SUPPORT/meetily" "$SUPPORT/$NEW_BRAND" "templates + notifications"
else
  say "  skip  templates + notifications — neither Meetily nor meetily exists"
  skipped=$((skipped + 1))
fi

step "3. Logs"
move_dir "$LOGS/$OLD_ID" "$LOGS/$NEW_ID" "log files"

step "4. Recordings, and the two places that point at them"
DB="$SUPPORT/$NEW_ID/meeting_minutes.sqlite"
PREFS="$SUPPORT/$NEW_ID/recording_preferences.json"

if [[ ! -e "$OLD_REC_PATH" ]]; then
  say "  skip  recordings — $OLD_REC_PATH does not exist"
  skipped=$((skipped + 1))
else
  # Count the affected meetings for the report. In a real run step 1 has already
  # moved the database to the new path; in a dry run nothing has moved, so the
  # count has to come from the old one.
  rows=0
  probe_db="$DB"
  if ! (( APPLY )); then
    probe_db="$SUPPORT/$OLD_ID/meeting_minutes.sqlite"
  fi
  if [[ -f "$probe_db" ]]; then
    rows=$(sqlite3 "$probe_db" \
      "SELECT COUNT(*) FROM meetings WHERE folder_path LIKE '$OLD_REC_PATH%';" 2>/dev/null || echo 0)
  fi

  move_dir "$OLD_REC_PATH" "$NEW_REC_PATH" "recordings"

  say "  edit  database — rewrite folder_path on $rows meeting(s)"
  say "  edit  recording_preferences.json — save_folder -> $NEW_REC_PATH"

  if (( APPLY )); then
    if [[ -f "$DB" ]]; then
      # Fold the write-ahead log in first, so the backup below is the whole
      # database rather than a base file plus a separate pending -wal.
      sqlite3 "$DB" "PRAGMA wal_checkpoint(TRUNCATE);" >/dev/null
      backup="$DB.pre-halvern-$(date +%Y%m%d-%H%M%S).bak"
      cp "$DB" "$backup"
      say "        database backed up to $(basename "$backup")"
      sqlite3 "$DB" \
        "UPDATE meetings
            SET folder_path = '$NEW_REC_PATH' || substr(folder_path, length('$OLD_REC_PATH') + 1)
          WHERE folder_path LIKE '$OLD_REC_PATH%';"
      left=$(sqlite3 "$DB" \
        "SELECT COUNT(*) FROM meetings WHERE folder_path LIKE '$OLD_REC_PATH%';")
      if [[ "$left" != "0" ]]; then
        say "        FAILED — $left row(s) still point at the old path"
        exit 1
      fi
    fi
    if [[ -f "$PREFS" ]]; then
      tmp=$(mktemp)
      sed "s|$OLD_REC_PATH|$NEW_REC_PATH|g" "$PREFS" > "$tmp"
      mv "$tmp" "$PREFS"
    fi
  fi
fi

step "Summary"
say "  $planned director$( ((planned==1)) && echo y || echo ies) to rename, $skipped skipped"

if (( APPLY )); then
  say ""
  say "Done. Two things macOS will ask about on the next launch:"
  say "  - Microphone and screen-recording permission. The bundle identifier"
  say "    changed, so macOS considers this a different application and the"
  say "    grants made to com.meetily.ai do not carry over."
  say "  - Gatekeeper, on an unsigned build, as before."
else
  say ""
  say "Nothing was written. Re-run with --apply when the numbers above look right."
fi
