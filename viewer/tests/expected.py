"""Write what `oxedi.parse` reads from every fixture and sample, for the viewer's oracle.

Usage: python viewer/tests/expected.py OUT_DIR

Runs in an environment with `oxedi` and `pyarrow` (the docs environment,
`make docs-dev`). For each file under `crates/oxedi_core/tests/fixtures` and
`crates/oxedi_core/tests/samples` except `README.md`, writes `OUT_DIR/<dir>__<name>.json`:

- `file`: the input's path relative to the repository root;
- for a file `oxedi.parse` rejects, `error`: the exception's text;
- otherwise `spec_version` (the first version the chosen spec declares),
  `segments` (the document's segment count), `tables` (table name → column
  name → each cell as the core renders it, `null` for a null; columns in
  order), `masked_tables` and `masked_diagnostics` (the same read with
  `oxedi.Mask(b"oxedi-test-key-0")`), `segment_paths` (each segment's open loops as `[name, ordinal]`
  pairs, outermost first, from `oxedi._core._loop_paths`), `diagnostics`
  (each one's full text), `paths` (each one's open loops as `Diagnostic.path`
  writes them) and `locations` (each one's `[level, segment, element,
  component]`).

A cell type the normalization does not know stops the run.
"""

from __future__ import annotations

import datetime
import decimal
import json
import pathlib
import sys

import pyarrow

import oxedi
from oxedi._core import _loop_paths

MASK_KEY = b"oxedi-test-key-0"
ROOT = pathlib.Path(__file__).resolve().parents[2]
CORPUS = [ROOT / "crates" / "oxedi_core" / "tests" / name for name in ("fixtures", "samples")]


def cell_text(value: object, where: str) -> str | None:
    """`value` as the core renders the cell it came from."""
    if value is None:
        return None
    if isinstance(value, str):
        return value
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    if isinstance(value, bool):
        raise TypeError(f"{where}: a bool cell has no core rendering ({value!r})")
    if isinstance(value, (int, decimal.Decimal)):
        return str(value)
    # datetime is a date too, but the core has no timestamp columns.
    if isinstance(value, datetime.datetime):
        raise TypeError(f"{where}: a datetime cell has no core rendering ({value!r})")
    if isinstance(value, datetime.date):
        return value.isoformat()
    if isinstance(value, datetime.time):
        return value.isoformat(timespec="seconds")
    raise TypeError(f"{where}: unknown cell type {type(value).__name__} ({value!r})")


def tables_of(result: oxedi.Result, relative: str) -> dict[str, dict[str, list[str | None]]]:
    tables: dict[str, dict[str, list[str | None]]] = {}
    for name in result.tables.keys():
        arrow = pyarrow.table(result.tables[name])
        tables[name] = {
            column: [
                cell_text(value, f"{relative}: {name}.{column}[{row}]")
                for row, value in enumerate(arrow.column(column).to_pylist())
            ]
            for column in arrow.column_names
        }
    return tables


def expected(path: pathlib.Path) -> dict[str, object]:
    relative = path.relative_to(ROOT).as_posix()
    try:
        result = oxedi.parse(path.read_bytes())
    except ValueError as err:
        return {"file": relative, "error": str(err)}
    masked = oxedi.parse(path.read_bytes(), mask=oxedi.Mask(MASK_KEY))
    declared = json.loads(result.spec.to_json()).get("version") or {}
    values = declared.get("values") or [None]
    return {
        "file": relative,
        "spec_version": values[0],
        "segments": len(result.document),
        "segment_paths": _loop_paths(result.document, result.spec, list(range(len(result.document)))),
        "tables": tables_of(result, relative),
        "masked_tables": tables_of(masked, relative),
        "masked_diagnostics": [str(d) for d in masked.diagnostics],
        "diagnostics": [str(d) for d in result.diagnostics],
        "paths": [d.path for d in result.diagnostics],
        "locations": [[d.level, d.segment, d.element, d.component] for d in result.diagnostics],
    }


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: python viewer/tests/expected.py OUT_DIR", file=sys.stderr)
        return 2
    out = pathlib.Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    count = 0
    for directory in CORPUS:
        for path in sorted(directory.iterdir()):
            if not path.is_file() or path.name == "README.md":
                continue
            target = out / f"{directory.name}__{path.name}.json"
            target.write_text(json.dumps(expected(path), ensure_ascii=False), encoding="utf-8")
            count += 1
    print(f"expected: wrote {count} files into {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
