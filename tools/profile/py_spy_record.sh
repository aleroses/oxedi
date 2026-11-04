#!/usr/bin/env bash
# Records one py-spy profile of a Python script.
# usage: py_spy_record.sh PY_SPY FORMAT OUTPUT PYTHON SCRIPT [ARGS...]
# Fails when the script exits non-zero, when py-spy fails, or when OUTPUT is missing or empty.
# The one py-spy failure tolerated is "No child process", which py-spy 0.4 reports after it has
# written its output when the script ends while it is still sampling. py-spy exits 0 whatever
# the script's status, so the script runs through run_script.py, which records it.
set -uo pipefail

if [ "$#" -lt 5 ]; then
  echo "usage: $0 PY_SPY FORMAT OUTPUT PYTHON SCRIPT [ARGS...]" >&2
  exit 2
fi
here=$(cd "$(dirname "$0")" && pwd)
py_spy=$1 format=$2 output=$3 python=$4
shift 4

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
rm -f "$output"

"$py_spy" record --format "$format" -o "$output" -- \
  "$python" "$here/run_script.py" "$work/status" "$@" 2> "$work/stderr"
spy_status=$?
cat "$work/stderr" >&2

if [ ! -s "$work/status" ]; then
  echo "profile-py: py-spy exited with status $spy_status before the script ended, recording $output; its error:" >&2
  grep -v '^py-spy>' "$work/stderr" >&2
  exit 1
fi
script_status=$(cat "$work/status")
if [ "$script_status" != 0 ]; then
  echo "profile-py: the script ($*) exited with status $script_status; no profile recorded" >&2
  rm -f "$output"
  exit 1
fi
if [ "$spy_status" != 0 ]; then
  others=$(grep '^Error' "$work/stderr" | grep -vc '^Error: No child process (os error 10)$')
  if [ "$others" != 0 ] || ! grep -qx 'Error: No child process (os error 10)' "$work/stderr"; then
    echo "profile-py: py-spy exited with status $spy_status while recording $output" >&2
    rm -f "$output"
    exit 1
  fi
fi
if [ ! -s "$output" ]; then
  echo "profile-py: py-spy wrote no $output" >&2
  exit 1
fi
