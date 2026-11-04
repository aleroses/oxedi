"""The repair of cells whose segment ends in whitespace: edi-835-parser strips
each segment, so the compat tables read those cells back from the document."""

from __future__ import annotations

import dataclasses
import decimal as decimal_module
import functools
import json
from typing import TYPE_CHECKING, Any, Callable, Optional

from ._convert import ENCODING, STRIPPED, ints, unpadded
from ._patch import PATCH, spec

if TYPE_CHECKING:
    import numpy as np
    import numpy.typing as npt


@functools.cache
def _loops():
    """The compat spec's loops, as its JSON holds them."""
    return json.loads(spec().to_json())["loops"]


def _occurrence_segment(table, column, loop, name):
    """The segment id of occurrence ``name`` in ``loop`` or the nearest of its
    parents that declares it, or ``None``. A ``loop`` the spec does not have
    raises a ``ValueError`` naming the table, the column and the loop."""
    loops = _loops()
    while loop is not None:
        if loop not in loops:
            raise ValueError(f"table {table!r}, column {column!r}: loop {loop!r} is not in the spec")
        occurrences = loops[loop].get("occurrences")
        if occurrences is None:
            raise ValueError(f"table {table!r}, column {column!r}: loop {loop!r} declares no occurrences")
        declared = occurrences.get(name)
        if declared is not None:
            return declared["segment"].encode()
        loop = loops[loop].get("parent")
    return None


def source_segment(table: str, column: str, source: dict) -> bytes:
    """The segment id a column source reads: the ``segment`` it names, or the
    segment of the ``occurrence`` it names, looked up in its ``loop`` or else in
    the loops of ``table`` and their parents. A source naming neither, or an
    occurrence no such loop declares, raises a ``ValueError`` naming the table,
    the column and what was found."""
    if "segment" in source:
        return source["segment"].encode()
    name = source.get("occurrence")
    if name is None:
        raise ValueError(f"table {table!r}, column {column!r}: the source {source} names neither a segment nor an occurrence")
    starts = [source["loop"]] if "loop" in source else PATCH["tables"][table].get("loops", [])
    for loop in starts:
        found = _occurrence_segment(table, column, loop, name)
        if found is not None:
            return found
    raise ValueError(
        f"table {table!r}, column {column!r}: occurrence {name!r} is not declared by {starts} or their parents"
    )


def _last_of(elements, element, component):
    """The raw text of ``element`` (and ``component``) among ``elements``
    when it is the last one, else ``None``."""
    if element != len(elements):
        return None
    value = elements[element - 1]
    if component is None:
        return value if isinstance(value, bytes) else None
    if isinstance(value, list):
        return value[component - 1] if component == len(value) else None
    return value if component == 1 else None


def _segment_of(document, anchors, row, own, bound):
    """The index of the segment a column reads for ``row``: the anchor
    segment, or the first segment named ``own`` after it and before the next
    row's anchor."""
    start = int(anchors[row])
    if own is None or document[start].id == own:
        return start
    for index in range(start + 1, bound):
        if document[index].id == own:
            return index
    return None


def _group_last(document, index, source, repeat):
    """The raw text a repeated group's column reads from segment ``index``
    when that is the segment's last element (and component), else ``None``:
    the group whose element ``group_element`` is the last one."""
    elements = document[index].elements
    offset = len(elements) - repeat["from"] - source["group_element"]
    if offset < 0 or offset % repeat["step"]:
        return None
    return _last_of(elements, len(elements), source.get("component"))


def _replace(table, column, changes, kind):
    """``table`` with the cells of ``column`` at the positions of ``changes``
    set to their values."""
    import pyarrow as pa

    values = table.column(column).to_pylist()
    for row, value in changes.items():
        values[row] = value
    at = table.schema.get_field_index(column)
    return table.set_column(at, table.schema.field(at), pa.array(values, kind))


@functools.cache
def _endings():
    """The bytes that may end whitespace: the ASCII bytes ``str.strip``
    removes and every non-ASCII byte (the last byte of a multi-byte
    character)."""
    import numpy as np

    table = np.zeros(256, dtype=bool)
    table[list(STRIPPED)] = True
    table[0x80:] = True
    return table


def any_padded(view, delimiters):
    """Whether any segment may end in whitespace: a byte that may end
    whitespace right before a segment terminator, or at the end of an input
    whose last segment has no terminator. Always true when a release
    character can escape a terminator."""
    import numpy as np

    if delimiters.release is not None or len(delimiters.segment) != 1:
        return True
    data = np.frombuffer(view, dtype=np.uint8)
    ends = np.flatnonzero(data == delimiters.segment[0])
    if _endings()[data[ends[ends > 0] - 1]].any():
        return True
    tail = data[ends[-1] + 1:] if len(ends) else data
    blank = np.zeros(256, dtype=bool)
    blank[list(STRIPPED)] = True
    return bool(len(tail) and not blank[tail].all() and _endings()[tail[-1]])


def _ending_in_blanks(column):
    """Positions of the binary cells whose last byte may end whitespace."""
    import numpy as np
    import pyarrow as pa
    import pyarrow.compute as pc

    last = pc.binary_slice(column, -1).cast(pa.binary())
    ends = np.zeros(len(column), dtype=np.int64) - 1
    present = ~last.is_null().to_numpy(zero_copy_only=False)
    lengths = pc.fill_null(pc.binary_length(last), 0).to_numpy(zero_copy_only=False)
    rows = np.flatnonzero(present & (lengths > 0))
    if not len(rows):
        return rows
    flat = np.frombuffer(b"".join(last.take(pa.array(rows)).to_pylist()), dtype=np.uint8)
    ends[rows] = flat
    return rows[_endings()[ends[rows]]]


@dataclasses.dataclass(frozen=True)
class _Anchoring:
    """Where a table's rows sit in the document: each row's anchor segment
    (-1 for none), the anchors in order, whether a row is the last of its
    anchor, whether a segment the parser reported lies between a row's anchor
    and the next row's, and where each row's span ends (the next anchor, or
    the document's length)."""

    anchors: "npt.NDArray[np.int64]"
    ordered: "npt.NDArray[np.int64]"
    bounds: "npt.NDArray[np.int64]"
    last_of_segment: "npt.NDArray[np.bool_]"
    flagged: "npt.NDArray[np.bool_]"

    @classmethod
    def of(cls, table, diagnosed, size):
        import numpy as np

        anchors = ints(table.column("segment"))
        ordered = np.sort(anchors[anchors >= 0])
        last = np.r_[anchors[1:] != anchors[:-1], True] if len(anchors) else anchors.astype(bool)
        # A row can read a cell the parser flagged only when a flagged
        # segment lies between its anchor and the next row's anchor.
        at = np.searchsorted(ordered, anchors, side="right")
        following = ordered[np.minimum(at, len(ordered) - 1)] if len(ordered) else 0
        bounds = np.where(at < len(ordered), following, size)
        flagged = (np.searchsorted(diagnosed, bounds) > np.searchsorted(diagnosed, anchors)) & (anchors >= 0)
        return cls(anchors, ordered, bounds, last, flagged)


def _same(stripped):
    return stripped


def _decimal(stripped):
    """The float of a stripped decimal's text, or ``None`` when it is not one."""
    try:
        return float(decimal_module.Decimal(stripped.decode(ENCODING)))
    except (ValueError, decimal_module.InvalidOperation):
        return None


@dataclasses.dataclass(frozen=True)
class _Strategy:
    """How one column kind is repaired: which rows to look at (``rows`` of
    the column, its anchoring and whether it reads a repeated group) and how
    to read the stripped text as the column's value."""

    rows: "Callable[[Any, _Anchoring, bool], npt.NDArray[np.intp]]"
    convert: "Callable[[bytes], Any]"


def _blank_ended(column, anchoring, grouped):
    return _ending_in_blanks(column)


def _flagged_nulls(column, anchoring, grouped):
    import numpy as np

    nulls = column.is_null().to_numpy(zero_copy_only=False) & anchoring.flagged
    return np.flatnonzero(nulls & anchoring.last_of_segment) if grouped else np.flatnonzero(nulls)


def _strategy(kind, padded):
    """The repair of a column of ``kind``: binary cells that may end in
    whitespace (only when some segment can), and decimal cells the spec could
    not read. ``None`` for any other kind."""
    import pyarrow as pa

    if pa.types.is_binary(kind):
        return _Strategy(_blank_ended, _same) if padded else None
    if pa.types.is_floating(kind):
        return _Strategy(_flagged_nulls, _decimal)
    return None


@dataclasses.dataclass(frozen=True)
class _Read:
    """What a column reads from its row's segment: its source, the table's
    repeat, the segment id the column reads when the table is not anchored on
    that segment (else ``None``) and whether it reads a repeated group."""

    source: dict
    repeat: "Optional[dict]"
    segment: "Optional[bytes]"
    grouped: bool

    @classmethod
    def of(cls, name, config, column, source):
        segment = None if config.get("segment") else source_segment(name, column, source)
        return cls(source, config.get("repeat"), segment, "group_element" in source)

    def raw(self, document, anchoring, row):
        """The raw text row ``row`` reads when it is its segment's last
        element (and component), or ``None``."""
        anchor = int(anchoring.anchors[row])
        if anchor < 0:
            return None
        if self.grouped:
            return _group_last(document, anchor, self.source, self.repeat) if anchoring.last_of_segment[row] else None
        index = _segment_of(document, anchoring.anchors, row, self.segment, int(anchoring.bounds[row]))
        if index is None:
            return None
        return _last_of(document[index].elements, self.source["element"], self.source.get("component"))


class _Unpadder:
    """Reads, from the document, the cells whose segment ends in whitespace
    and returns the tables with them replaced."""

    def __init__(self, document, diagnosed, padded):
        self.document = document
        self.diagnosed = diagnosed
        self.padded = padded

    def run(self, arrow):
        for name, config in PATCH["tables"].items():
            if config is not None and name in arrow and "segment" in arrow[name].column_names:
                arrow[name] = self._table(name, config, arrow[name])
        return arrow

    def _table(self, name, config, table):
        anchoring = _Anchoring.of(table, self.diagnosed, len(self.document))
        for column, source in config["columns"].items():
            if source.get("element") is not None or "group_element" in source:
                table = self._column(name, config, anchoring, table, column, source)
        return table

    def _column(self, name, config, anchoring, table, column, source):
        kind = table.schema.field(column).type
        strategy = _strategy(kind, self.padded)
        if strategy is None:
            return table
        read = _Read.of(name, config, column, source)
        rows = strategy.rows(table.column(column), anchoring, read.grouped)
        changes = self._changes(strategy, anchoring, rows, read)
        return _replace(table, column, changes, kind) if changes else table

    def _changes(self, strategy, anchoring, rows, read):
        """The new value of each row in ``rows`` whose raw text ends in
        whitespace and reads as the column's kind once stripped."""
        changes = {}
        for row in rows.tolist():
            raw = read.raw(self.document, anchoring, row)
            stripped = None if raw is None else unpadded(raw)
            value = None if stripped in (None, raw) else strategy.convert(stripped)
            if value is not None:
                changes[row] = value
        return changes


def unpad(document, diagnostics, arrow, padded=True):
    """edi-835-parser strips each segment, so whitespace before the
    terminator never reaches its last element. A binary cell that ends in
    whitespace is stripped when it reads its segment's last element, and a
    decimal cell (a float column here) that the spec could not read because
    of it is read from the stripped text. Only those cells are looked up in
    the document: binary cells whose last byte may end whitespace, and null
    decimal cells in a segment the parser reported (a decimal it could not
    read always is; in a repeated group, only the segment's last group).
    When no segment can end in whitespace (``padded`` false), binary cells
    are not looked at. Returns the tables with those cells replaced."""
    import numpy as np

    reported = [d.segment for d in diagnostics if d.segment is not None]
    diagnosed = np.unique(np.array(reported, dtype=np.int64))
    if not padded and not len(diagnosed):
        return arrow
    return _Unpadder(document, diagnosed, padded).run(arrow)
