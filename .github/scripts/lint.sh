#!/usr/bin/env bash
# Lints the workflows and the scripts they run: actionlint over .github/workflows (it checks
# every `run:` block with shellcheck too), zizmor over the workflows, the composite actions and
# the Dependabot configuration (unpinned actions, template injection, stored credentials,
# excessive permissions, updates without a cooldown), shellcheck over .github/scripts and tools,
# then the tests of those scripts.
# Needs actionlint, zizmor and shellcheck on the PATH (install-lint-tools.sh installs the pinned
# ones).
set -euo pipefail

cd "$(dirname "$0")/../.."
actionlint
zizmor --offline --format plain --color never .github/workflows .github/actions .github/dependabot.yml
shopt -s globstar nullglob
shellcheck -x .github/scripts/**/*.sh tools/**/*.sh
for test in .github/scripts/tests/*.sh tools/*/tests/*.sh; do
  "$test"
done
