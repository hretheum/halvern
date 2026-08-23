#!/usr/bin/env bash
#
# Build the real-meeting control set from the local database.
#
# The synthetic corpus is what makes scoring cheap, and it is also the thing
# most likely to be quietly unrepresentative — six invented Polish meetings
# could share a phrasing habit that flatters one model. The control set answers
# one question: does the Slavic ranking obtained on synthetic material hold on
# real material? Nothing else.
#
# These are recordings of real people. They are scored only on the metrics that
# need no ground truth — structure preservation, template compliance, output
# language — and `score.py` refuses to put them in the adjudication file, so
# they never reach a cloud judge.
#
# Output goes to corpus/_control/, which is git-ignored.
#
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
OUT="corpus/_control"
DB="$HOME/Library/Application Support/io.halvern.app/meeting_minutes.sqlite"
COUNT="${1:-6}"

[ -f "$DB" ] || { echo "no database at $DB"; exit 1; }

mkdir -p "$OUT"
echo "corpus/_control/" > "$OUT/../.gitignore.control" 2>/dev/null || true

# Longest meetings first, then take a spread: the point is length variety, so
# that both the single-pass and the map/reduce branch are exercised on real
# material as well as synthetic.
ids=$(sqlite3 "$DB" "
  SELECT m.id
  FROM meetings m
  JOIN transcripts t ON t.meeting_id = m.id
  GROUP BY m.id
  ORDER BY COUNT(t.id) DESC
  LIMIT $COUNT;")

n=0
for id in $ids; do
  n=$((n + 1))
  out="$OUT/control-$(printf '%02d' "$n").json"

  sqlite3 -json "$DB" "
    SELECT t.timestamp AS t, COALESCE(t.speaker,'unknown') AS speaker, t.text AS text
    FROM transcripts t WHERE t.meeting_id = '$id'
    ORDER BY t.timestamp;" > "$out.turns"

  python3 - "$out" "$id" <<'PY'
import json, sys, pathlib
out, mid = pathlib.Path(sys.argv[1]), sys.argv[2]
turns = json.loads(pathlib.Path(str(out) + ".turns").read_text(encoding="utf-8") or "[]")
speakers = sorted({t["speaker"] for t in turns})
doc = {
    "id": out.stem,
    "language": "pl",
    "group": "Slavic",
    "length_class": "long" if len(turns) > 200 else "short",
    "template_id": "standard_meeting",
    "participants": speakers,
    "transcript": [{"t": float(t["t"] or 0), "speaker": t["speaker"], "text": t["text"]} for t in turns],
    # No ground_truth: real meetings carry none, and inventing one by reading
    # them would be the same subjective judgement the synthetic corpus exists to
    # avoid. These files are scored on M1, M2 and M6 only.
    "control": True,
    "source_meeting_id": mid,
}
out.write_text(json.dumps(doc, ensure_ascii=False, indent=2), encoding="utf-8")
pathlib.Path(str(out) + ".turns").unlink()
print(f"  {out.name}: {len(turns)} turns, {len(speakers)} speaker(s)")
PY
done

echo
echo "$n control file(s) in $OUT — git-ignored, never sent to a judge."
echo "Confirm .gitignore covers it before committing anything in this folder."
