#!/usr/bin/env bash
#
# Clippy, as a ratchet rather than a wall.
#
# This code was inherited with 135 clippy warnings. `-D warnings` on an
# inherited codebase gives you two options, both bad: block every pull request
# until someone does a thousand-line cleanup nobody asked for, or turn the lint
# off and lose it permanently. A team that reaches for the second one is right
# to, and then never gets it back.
#
# So the gate is on the direction of travel. The count may not go up. Fixing
# warnings is welcome and lowers the baseline; ignoring the existing ones costs
# a contributor nothing. Over enough pull requests the number walks to zero
# without anyone scheduling it.
#
# The baseline is a file rather than a computed comparison against main, because
# a contributor should be able to run this locally without fetching another
# branch, and because a number in the repository is something you can see
# getting smaller.
#
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."

BASELINE_FILE="scripts/ci/clippy-baseline.txt"
baseline=$(grep -Eo '^[0-9]+' "$BASELINE_FILE" | head -1)

cd frontend/src-tauri

# A warning count only means something if clippy actually finished. Some
# lints (clippy::inherent_to_string_shadow_display, for one) are deny-by-
# default, so clippy exits non-zero instead of printing one more "warning:"
# line for them. The previous version of this script piped clippy straight
# into `grep -c 'warning:' || true`, which swallowed that exit code — the
# `|| true` meant a failing build and a passing one looked identical here,
# as long as the warning count on the way down happened to match the
# baseline. That's exactly how a deny-by-default error sat in this codebase
# unnoticed: the count matched, so the ratchet said nothing was wrong.
# `--message-format=short` is parsed by grep below, so the output has to be
# plain. `swatinem/rust-cache` exports CARGO_TERM_COLOR=always, and with colour
# on, cargo writes `\e[1m\e[33mwarning\e[0m:` — the escape codes land between
# the word and the colon, so `grep 'warning:'` matches nothing and the count
# comes back 0 on a codebase with 27 warnings.
#
# That is the same failure as the `|| true` this script used to have: a number
# that is not a count of anything. It surfaced only because 0 was *below* the
# baseline and tripped the "you removed warnings" branch. Had the baseline been
# 0, or the drift been the other way, the ratchet would have reported success
# on a measurement of nothing. So the script sets this itself rather than
# trusting whatever the caller exported.
if clippy_output=$(CARGO_TERM_COLOR=never cargo clippy --lib --message-format=short 2>&1); then
  clippy_exit=0
else
  clippy_exit=$?
fi

if [ "$clippy_exit" -ne 0 ]; then
  echo "cargo clippy exited $clippy_exit — it failed, it didn't just warn."
  echo
  echo "A warning count from a build that didn't finish isn't a count of"
  echo "anything real, so this script can't tell you anything useful about"
  echo "the ratchet until clippy itself is clean. Run it to see why:"
  echo "    cd frontend/src-tauri && cargo clippy --lib"
  exit 1
fi

current=$(printf '%s\n' "$clippy_output" | grep -c 'warning:' || true)

echo "clippy: $current warnings (baseline $baseline)"

if [ "$current" -gt "$baseline" ]; then
  echo
  echo "This change adds $((current - baseline)) clippy warning(s)."
  echo
  echo "Run this to see them:"
  echo "    cd frontend/src-tauri && cargo clippy --lib"
  echo
  echo "The existing $baseline are inherited and not your problem. The new ones"
  echo "are. If a warning is wrong about your code, silence that one with"
  echo "#[allow(...)] and a comment saying why — not the whole lint."
  exit 1
fi

if [ "$current" -lt "$baseline" ]; then
  echo
  echo "You removed $((baseline - current)) warning(s). Lower the baseline so"
  echo "they cannot come back:"
  echo
  echo "    echo $current > $BASELINE_FILE"
  echo
  echo "(then restore the explanatory comment under the number)"
  exit 1
fi
