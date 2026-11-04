"""The compat spec, and its tables as Python columns split per transaction."""

from __future__ import annotations

import functools
import json
from collections import defaultdict
from typing import Optional, Tuple

from .. import ParseError, parse
from ._convert import STRIPPED, ints
from ._patch import PATCH, TABLES, spec
from ._unpad import any_padded as _padded
from ._unpad import unpad


@functools.cache
def interchange_trigger() -> bytes:
    """The segment id that opens an interchange, as the spec declares it."""
    return json.loads(spec().to_json())["loops"]["interchange"]["trigger"]["segment"].encode()


def position(table: str, column: str) -> Tuple[int, Optional[int]]:
    """The element and component (``None`` for a whole element) that the patch
    reads for ``column`` of ``table``."""
    source = PATCH["tables"][table]["columns"][column]
    return source["element"], source.get("component")


class Rows:
    """One table's rows of one transaction: an Arrow table, read as columns of
    Python values on demand."""

    def __init__(self, table):
        self.table = table
        self._columns = {}
        self._groups = {}

    @classmethod
    def empty(cls):
        """A table with no rows, whose every column reads as empty."""
        return cls(None)

    def __len__(self):
        return 0 if self.table is None else self.table.num_rows

    def __getitem__(self, column):
        if column not in self._columns:
            self._columns[column] = [] if self.table is None else self.table.column(column).to_pylist()
        return self._columns[column]

    def under(self, parent, key):
        """Positions of the rows whose ``parent`` column is ``key``, in file
        order: the join from one loop level's table to the level it nests in."""
        if parent not in self._groups:
            groups = defaultdict(list)
            for at, value in enumerate(self[parent]):
                groups[value].append(at)
            self._groups[parent] = groups
        return self._groups[parent].get(key, [])


BOM = b"\xef\xbb\xbf"


def opening(view, trigger):
    """The offset of ``trigger`` when ``view`` is a UTF-8 byte order mark, then
    optional whitespace (any ASCII byte ``str.strip`` removes), then
    ``trigger``; otherwise ``None``. Parsing starts at that offset, so the
    parser never sees the mark or the whitespace."""
    if bytes(view[:len(BOM)]) != BOM:
        return None
    at = len(BOM)
    while at < len(view) and view[at] in STRIPPED:
        at += 1
    return at if bytes(view[at:at + len(trigger)]) == trigger else None


def _floats(table):
    """``table`` with its decimal columns as float64, the library's ``float``
    of each value: the decimal's exact text read as the nearest double."""
    import pyarrow as pa

    for at, field in enumerate(table.schema):
        if pa.types.is_decimal(field.type):
            column = table.column(at).cast(pa.string()).cast(pa.float64())
            table = table.set_column(at, pa.field(field.name, pa.float64(), field.nullable), column)
    return table


def _split(table, column, keys):
    """For each key, the slice of ``table`` whose ``column`` equals it; rows
    are in file order, so each transaction's rows are contiguous."""
    import numpy as np
    import pyarrow as pa

    ids = ints(table.column(column))
    if len(ids) and np.any(ids[1:] < ids[:-1]):
        return [table.filter(pa.array(ids == key)) for key in keys]
    starts = np.searchsorted(ids, keys, side="left")
    ends = np.searchsorted(ids, keys, side="right")
    return [table.slice(int(a), int(b - a)) for a, b in zip(starts, ends)]


def load(data):
    """Parses ``data`` (bytes or any buffer) with the compat spec, with the GIL
    released, and returns the document and, per transaction, the tables
    limited to its rows (split in one pass over each table). A table column
    cannot read a value from an enclosing loop, so each loop level is its own
    table, gathered through the parent-row columns (``payment``, ``claim``,
    ``service``).

    The library builds one transaction set from any file, reading whatever
    segments it recognises:
    - input that does not start with the interchange's opening segment gives
      one part with every table empty and no document;
    - a UTF-8 byte order mark before that segment (with or without
      whitespace between them) stays glued to the first segment in the
      library, which then misses only the interchange, so the input is parsed
      from that segment on and the interchange rows are left out; if that
      parse fails, the result is the part with every table empty, since the
      library never reads the segment that failed;
    - an interchange without transactions gives one part with only its
      interchange rows.
    Any other ``ParseError`` (an opening segment that cannot be read, with
    no mark before it) propagates."""
    import pyarrow as pa

    view = memoryview(data)
    trigger = interchange_trigger()
    start = opening(view, trigger)
    bom = start is not None
    try:
        result = parse(view[start:] if bom else data, spec=spec())
    except ParseError:
        if not bom and bytes(view[:len(trigger)]) == trigger:
            raise
        return None, [{name: Rows.empty() for name in TABLES}]
    tables = {name: _floats(pa.table(result.tables[name])) for name in result.tables}
    padded = _padded(view[start:] if bom else view, result.document.delimiters)
    arrow = unpad(result.document, result.diagnostics, tables, padded)
    payments = arrow["rows_payments"]
    interchanges = arrow["rows_interchanges"]
    if not payments.num_rows:
        return result.document, [{
            name: Rows(table if name == "rows_interchanges" and not bom else table.slice(0, 0))
            for name, table in arrow.items()
        }]
    keys = ints(payments.column("row"))
    split = {
        name: _split(table, "row" if name == "rows_payments" else "payment", keys)
        for name, table in arrow.items() if name != "rows_interchanges"
    }
    rows = ints(interchanges.column("row"))
    parts = []
    for at, interchange in enumerate(ints(payments.column("interchange"))):
        part = {name: Rows(tables[at]) for name, tables in split.items()}
        own = interchanges.slice(0, 0) if bom else interchanges.filter(pa.array(rows == interchange))
        part["rows_interchanges"] = Rows(own)
        parts.append(part)
    return result.document, parts
