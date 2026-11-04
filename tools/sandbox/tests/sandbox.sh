#!/usr/bin/env bash
# Runs the tests of sandbox.py against a fake `docker` (standard library only; no Docker needed).
# Run: tools/sandbox/tests/sandbox.sh
set -euo pipefail

exec python3 -m unittest discover -s "$(cd "$(dirname "$0")" && pwd)" -p 'test_*.py'
