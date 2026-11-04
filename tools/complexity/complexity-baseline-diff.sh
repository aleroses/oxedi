#!/usr/bin/env bash
# On a pull request, reports the entries .github/complexity-baseline.txt gained or raised against
# the base branch's version, in the log and the job summary; it never fails. Outside a pull
# request ($GITHUB_BASE_REF empty), or when the base has no baseline, it reports nothing.
# Needs python3 and git.
set -euo pipefail

cd "$(dirname "$0")/../.."
baseline=.github/complexity-baseline.txt
if [ -z "${GITHUB_BASE_REF:-}" ]; then
  echo "complexity-baseline-diff: not a pull request; nothing to compare"
  exit 0
fi
if ! git fetch --quiet --no-tags origin \
  "+refs/heads/$GITHUB_BASE_REF:refs/remotes/origin/$GITHUB_BASE_REF"; then
  echo "complexity-baseline-diff: could not fetch $GITHUB_BASE_REF from origin; nothing compared"
  exit 0
fi
old=$(mktemp)
trap 'rm -f "$old"' EXIT
if ! git show "origin/$GITHUB_BASE_REF:$baseline" > "$old" 2>/dev/null; then
  echo "complexity-baseline-diff: origin/$GITHUB_BASE_REF has no $baseline; nothing to compare"
  exit 0
fi
python3 tools/complexity/complexity.py "$baseline" --compare-with "$old"
