#!/usr/bin/env bash
# Runs the tests of deps.py (standard library only).
# Run: tools/deps/tests/deps.sh
set -euo pipefail

exec python3 -m unittest discover -s "$(cd "$(dirname "$0")" && pwd)" -p 'test_*.py'
