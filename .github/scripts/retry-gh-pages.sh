#!/usr/bin/env bash
# Runs a command that pushes to gh-pages, up to 3 times. Before every attempt the local
# gh-pages branch (the one mike works on) is reset to the commit it just fetched, so a retry
# after a rejected push redoes its commit on top of what the other deploy published.
#
# Only two failures are retried: the fetch itself failing, and the command's output showing that
# the push was rejected because the remote moved (non-fast-forward, "fetch first"). Any other
# failure of the command, such as a broken site build, fails at once with the command's status.
# RETRY_DELAY_UNIT (seconds, default 5) scales the wait between attempts.
#
# Run as a script, the command is a program. Sourced, the file defines retry_gh_pages, whose
# command may also be a shell function of the caller: deploy-docs.sh retries a function that
# pushes with a token it never exports. The command runs where `set -e` does not apply, so a
# function chains its steps with &&.
# Usage: retry-gh-pages.sh COMMAND [ARGS...]
#        . retry-gh-pages.sh; retry_gh_pages COMMAND [ARGS...]

retry_gh_pages() {
  local attempts=3 rejected='non-fast-forward|fetch first|\[rejected\]'
  local log attempt reason status result=1
  log=$(mktemp)
  for attempt in $(seq 1 "$attempts"); do
    reason=""
    if ! git fetch origin gh-pages; then
      reason="git fetch origin gh-pages failed"
    elif ! git branch -f gh-pages FETCH_HEAD; then
      break
    else
      # The braces keep a failing command from ending a caller that runs with `set -e`.
      {
        "$@" 2>&1 | tee "$log"
        status=${PIPESTATUS[0]}
      } || true
      if [ "$status" -eq 0 ]; then
        result=0
        break
      fi
      if ! grep -Eq "$rejected" "$log"; then
        echo "Not retrying: the failure is not a rejected push: $*" >&2
        result=$status
        break
      fi
      reason="the push was rejected because gh-pages moved"
    fi
    echo "Attempt $attempt of $attempts failed ($reason): $*" >&2
    if [ "$attempt" -lt "$attempts" ]; then
      sleep $((attempt * ${RETRY_DELAY_UNIT:-5}))
    else
      echo "Giving up after $attempts attempts: $*" >&2
    fi
  done
  rm -f "$log"
  return "$result"
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  set -uo pipefail
  retry_gh_pages "$@"
fi
