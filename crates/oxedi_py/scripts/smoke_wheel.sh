#!/usr/bin/env bash
# Builds the release wheel, checks its packaging, installs it in a fresh
# virtual environment outside the repository and runs the Python tests there
# against it.
set -euo pipefail

repo="$(cd "$(dirname "$0")/../../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

maturin build --release --manifest-path "$repo/crates/oxedi_py/Cargo.toml" --out "$work/dist"
python "$repo/crates/oxedi_py/scripts/check_wheel.py" "$(ls "$work"/dist/oxedi-*.whl)" "$repo"
python "$repo/tools/deps/deps.py" wheel "$(ls "$work"/dist/oxedi-*.whl)"
uv venv --quiet --python "$(command -v python)" "$work/venv"
VIRTUAL_ENV="$work/venv" uv pip install --quiet "$work"/dist/oxedi-*.whl --group "$repo/crates/oxedi_py/pyproject.toml:test"
cp -r "$repo/crates/oxedi_py/tests" "$work/tests"

cd "$work"
"$work/venv/bin/python" -c "
import oxedi, pathlib
assert pathlib.Path(oxedi.__file__).is_relative_to(pathlib.Path('$work/venv')), oxedi.__file__
result = oxedi.parse_file('$repo/crates/oxedi_core/tests/samples/edi835_test_united.rmt')
print('smoke:', result)
"
OXEDI_CORE_TESTS="$repo/crates/oxedi_core/tests" "$work/venv/bin/python" -m pytest -q -p no:cacheprovider "$work/tests"
