#!/usr/bin/env bash
# Installs the wheel found in WHEEL_DIR into a fresh virtual environment outside the
# repository, checks its packaging and runs the Python suite against it.
# usage: verify_wheel.sh WHEEL_DIR   (uses `python` from PATH; runs on Linux, macOS, Windows)
# VERIFY_EXCLUDE="duckdb ..." leaves the named test dependencies out, for a platform where
# one has no wheel; its tests then skip.
set -euo pipefail

native() { (cd "$1" && { pwd -W 2>/dev/null || pwd; }); }

repo="$(native "$(dirname "$0")/../../..")"
dist="$(native "$1")"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
work="$(native "$work")"

wheel="$(ls "$dist"/oxedi-*.whl)"
version="$(basename "$wheel" | cut -d- -f2)"

python "$repo/crates/oxedi_py/scripts/check_wheel.py" "$wheel" "$repo"

python -m venv "$work/venv"
py="$work/venv/bin/python"
[ -x "$py" ] || py="$work/venv/Scripts/python.exe"

# The pyproject's test dependency group, minus VERIFY_EXCLUDE.
requirements="$("$py" - "$repo/crates/oxedi_py/pyproject.toml" "${VERIFY_EXCLUDE:-}" <<'PY'
import re, sys, tomllib

pyproject, excluded = sys.argv[1], sys.argv[2].split()
with open(pyproject, "rb") as f:
    group = tomllib.load(f)["dependency-groups"]["test"]
for spec in group:
    if re.match(r"[A-Za-z0-9._-]+", spec.strip()).group(0).lower() not in excluded:
        print(spec.replace(" ", ""))
PY
)"
"$py" -m pip install --quiet "$wheel" $requirements
cp -r "$repo/crates/oxedi_py/tests" "$work/tests"

cd "$work"
"$py" -c "
import oxedi, pathlib, importlib.metadata as m
assert pathlib.Path(oxedi.__file__).resolve().is_relative_to(pathlib.Path(r'$work').resolve()), oxedi.__file__
assert m.version('oxedi') == '$version', m.version('oxedi')
print('verify:', oxedi.__file__, m.version('oxedi'))
"
OXEDI_CORE_TESTS="$repo/crates/oxedi_core/tests" "$py" -m pytest -q -rs -p no:cacheprovider "$work/tests"
