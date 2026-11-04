#!/usr/bin/env bash
# Checks that zizmor, as lint.sh runs it, passes .github/ as it is and flags an added unpinned
# action and an added template injection.
# Run: .github/scripts/tests/zizmor.sh
set -uo pipefail

repo=$(cd "$(dirname "$0")/../../.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
failed=0

expect() {
  local what=$1 expected=$2 actual=$3
  if [ "$expected" != "$actual" ]; then
    echo "FAIL: $what: expected $expected, got $actual"
    failed=1
  fi
}

# Copies .github/ to a fresh directory named $1, adds the workflow on stdin to it unless it is
# empty, runs zizmor there as lint.sh does (plain format, no colour, so the audit names stay at the start of a line) and leaves its exit status in $status and its output in $out.
lint_copy() {
  local dir="$tmp/$1"
  mkdir -p "$dir"
  cp -R "$repo/.github" "$dir/.github"
  cat > "$dir/added.yml"
  if [ -s "$dir/added.yml" ]; then mv "$dir/added.yml" "$dir/.github/workflows/added.yml"; fi
  out=$(cd "$dir" && zizmor --offline --format plain --color never .github/workflows .github/actions .github/dependabot.yml 2>&1)
  status=$?
}

# The audit named in the output of the last copy, as yes or no.
named() { if grep -q "^[a-z]*\[$1\]" <<< "$out"; then echo yes; else echo no; fi; }
# The exit status of the last copy, as zero or nonzero.
zero() { if [ "$status" -eq 0 ]; then echo zero; else echo nonzero; fi; }

lint_copy clean < /dev/null
expect "the untouched copy exits" zero "$(zero)"

lint_copy unpinned <<'Y'
on: push
permissions: {}
jobs:
  j:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/setup-node@v4
Y
expect "an unpinned action exits" nonzero "$(zero)"
expect "an unpinned action is named" yes "$(named unpinned-uses)"

lint_copy injection <<'Y'
on: pull_request
permissions: {}
jobs:
  j:
    runs-on: ubuntu-latest
    steps:
      - run: echo "${{ github.event.pull_request.title }}"
Y
expect "a template injection exits" nonzero "$(zero)"
expect "a template injection is named" yes "$(named template-injection)"

lint_copy cooldown < /dev/null
sed -i '/cooldown:/,+1d' "$tmp/cooldown/.github/dependabot.yml"
out=$(cd "$tmp/cooldown" && zizmor --offline --format plain --color never .github/workflows .github/actions .github/dependabot.yml 2>&1)
status=$?
expect "a dependabot.yml without a cooldown exits" nonzero "$(zero)"
expect "a dependabot.yml without a cooldown is named" yes "$(named dependabot-cooldown)"

if [ "$failed" -eq 0 ]; then echo "zizmor: ok"; else echo "$out"; fi
exit "$failed"
