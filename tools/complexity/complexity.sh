#!/usr/bin/env bash
# Measures cyclomatic complexity and nesting of every function in the Rust, Python and
# JavaScript sources and compares them with .github/complexity-baseline.txt (complexity.py says
# how). Measured: crates/*/src (Rust), crates/*/python and crates/*/scripts (Python) and
# viewer/js. Not measured: tests (tests/, test/, tests.rs), benches, examples, the tooling under
# .github/ and tools/, docs/ and viewer/tests, and anything git does not track (target/, site/,
# viewer/pkg/) or that is not a source file (the generated .pyi stubs).
# A baseline function the change modifies must end within the targets. The change is measured
# against $COMPLEXITY_BASE; unset, that is $COMPLEXITY_PUSH_BEFORE on a push in CI, the first
# parent of HEAD on a pull request (the checkout is the merge with the base branch) and
# origin/master elsewhere.
# Needs uv: uvx runs complexity.py with the pinned lizard. Arguments go to complexity.py:
# --update rewrites the baseline, --accept PATH::FUNCTION records a reviewed exception.
set -euo pipefail

LIZARD_VERSION="1.24.1"
PYGMENTS_VERSION="2.21.0"
PATHSPEC_VERSION="1.1.1"
PYTHON_VERSION="3.13"

cd "$(dirname "$0")/../.."
# On a push the base is the commit before the push, as ci.yml's path filters use it; a new
# branch (all-zero commit) and a pull request use the first parent of HEAD.
before=${COMPLEXITY_PUSH_BEFORE:-}
case "${GITHUB_EVENT_NAME:-}" in
  push)
    if [ -n "$before" ] && [ -n "${before//0/}" ]; then
      git cat-file -e "$before^{commit}" 2> /dev/null \
        || git fetch --quiet --no-tags origin "$before" 2> /dev/null || true
      : "${COMPLEXITY_BASE:=$before}"
    else
      : "${COMPLEXITY_BASE:=HEAD^1}"
    fi
    ;;
  pull_request) : "${COMPLEXITY_BASE:=HEAD^1}" ;;
esac
export COMPLEXITY_BASE
files=$(mktemp)
trap 'rm -f "$files"' EXIT

git ls-files -- \
  'crates/*/src/*.rs' 'crates/*/python/*.py' 'crates/*/scripts/*.py' 'viewer/js/*.js' \
  | grep -vE '(^|/)tests?(/|\.rs$)' > "$files"

uvx --python "$PYTHON_VERSION" --from "lizard==$LIZARD_VERSION" \
  --with "pygments==$PYGMENTS_VERSION" --with "pathspec==$PATHSPEC_VERSION" \
  python tools/complexity/complexity.py .github/complexity-baseline.txt --files "$files" "$@"
