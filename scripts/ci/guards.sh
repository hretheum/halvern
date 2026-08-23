#!/usr/bin/env bash
#
# Every project-specific check, in one command.
#
# These are the rules this repository already wrote down and had no way to
# enforce. Each one exists because the failure it catches is invisible: a new
# hostname passes every test, a test that writes to the real data directory goes
# green while eating your meetings, a Polish comment is fine until the repository
# is public, and a resurrected `com.meetily.ai` looks like every other string.
#
# Run it before pushing. It is the same script CI runs, so there are no
# surprises waiting in the pull request — that is the point of it being a script
# and not a workflow step.
#
#     ./scripts/ci/guards.sh
#
set -uo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."

failed=""
for check in check-network-hosts check-test-isolation check-english-only \
             check-brand check-format; do
  printf '\n\033[1m%s\033[0m\n' "$check"
  if ! "./scripts/ci/$check.sh"; then
    failed="$failed $check"
  fi
done

printf '\n'
if [ -n "$failed" ]; then
  echo "FAILED:$failed"
  exit 1
fi
echo "all guards passed"
