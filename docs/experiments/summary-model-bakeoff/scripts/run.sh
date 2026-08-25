#!/usr/bin/env bash
#
# Drive a full bake-off run and leave an auditable trail.
#
# Everything here is deliberately re-runnable and records the conditions it ran
# under. A result you cannot reproduce is a result you cannot defend, and the
# thing most likely to invalidate a run quietly is `processor.rs` changing
# underneath it — so the SHA is captured up front and again per record.
#
#   ./run.sh <run-id> [M|S]
#
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
ROOT=$(pwd)
TAURI="$ROOT/../../../frontend/src-tauri"

RUN_ID="${1:?usage: run.sh <run-id> [M|S]}"
ARM="${2:-M}"
OUT="$ROOT/results/raw/$RUN_ID-$ARM"

TIER_A="qwen3.5:4b,gemma3:4b"
TIER_B="qwen3.5:2b,gemma3:1b"
DATA_DIR="$HOME/Library/Application Support/io.halvern.app"

say() { printf '\n\033[1m%s\033[0m\n' "$*"; }

say "0. Preconditions"
python3 "$ROOT/scripts/validate_corpus.py" "$ROOT/corpus"

sha=$(cd "$TAURI" && git log -1 --format=%h -- src/summary/processor.rs)
dirty=$(cd "$TAURI" && git status --porcelain -- src/summary/processor.rs)
if [ -n "$dirty" ]; then
  echo "processor.rs has uncommitted changes. Commit or stash first —"
  echo "otherwise the recorded SHA does not describe what actually ran."
  exit 1
fi
echo "  processor.rs at $sha, clean"

# The four GGUF filenames are read out of models.rs rather than repeated here,
# so a lineup change cannot leave this script checking for the wrong files.
missing=0
for g in $(grep -oE 'gguf_file: "[^"]+"' "$TAURI/src/summary/summary_engine/models.rs" \
           | sed 's/gguf_file: "//; s/"//'); do
  if [ ! -f "$DATA_DIR/models/summary/$g" ]; then
    echo "  missing model: $g"
    missing=1
  fi
done
if [ "$missing" = 1 ]; then
  echo
  echo "Download the four built-in models from the app's model settings before"
  echo "running. All four are needed; two tiers of two."
  exit 1
fi
echo "  all four models present"

say "1. Building the harness"
# An example, not a bin. Anything under src/bin/ is discovered by cargo and
# picked up by the Tauri bundler, which is how a 29 MB measurement harness
# ended up inside a signed, notarized 0.1.0 disk image. Examples are built
# only when asked for by name.
mkdir -p "$TAURI/examples"
cp "$ROOT/scripts/summary_bench.rs" "$TAURI/examples/summary_bench.rs"
(cd "$TAURI" && cargo build --release --example summary_bench)

say "2. Pre-flight P0 — chat templates"
echo "  Run the two P0 probes from 03-prompts.md against each model."
echo "  This is a human check: read the output, confirm no template markers"
echo "  leak and the table comes back well-formed. Continue only if all four"
echo "  pass — a model that fails P0 tells you nothing about Japanese."
read -r -p "  All four models passed P0? [y/N] " ok
[ "$ok" = "y" ] || { echo "  stopping"; exit 1; }

say "3. Tier A — qwen3.5:4b vs gemma3:4b (arm $ARM)"
(cd "$TAURI" && "$TAURI/../../target/release/examples/summary_bench" \
  --corpus "$ROOT/corpus" --out "$OUT" --models "$TIER_A" \
  --arm "$ARM" --repeats 3 --data-dir "$DATA_DIR")

say "4. Tier B — qwen3.5:2b vs gemma3:1b (arm $ARM)"
(cd "$TAURI" && "$TAURI/../../target/release/examples/summary_bench" \
  --corpus "$ROOT/corpus" --out "$OUT" --models "$TIER_B" \
  --arm "$ARM" --repeats 3 --data-dir "$DATA_DIR")

say "5. Scoring"
python3 "$ROOT/scripts/score.py" \
  --raw "$OUT" --corpus "$ROOT/corpus" \
  --template "$TAURI/templates/standard_meeting.json" \
  --report "$ROOT/results/REPORT-$RUN_ID-$ARM.md"

say "Done"
echo "  raw     $OUT"
echo "  scored  $ROOT/results/scored.json"
echo
echo "If uncertain.json was written, adjudicate it with P3/P3b from"
echo "03-prompts.md and re-score with --adjudications."
