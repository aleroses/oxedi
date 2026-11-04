"""Value conversions that reproduce edi-835-parser's element parsers."""

from __future__ import annotations

import datetime
import locale

# The library reads each file with ``open()`` in text mode, which decodes with
# the locale's preferred encoding; element text is decoded with the same one.
ENCODING = locale.getpreferredencoding(False)


# ASCII bytes that ``str.strip`` removes.
STRIPPED = bytes(byte for byte in range(128) if chr(byte).isspace())


def unpadded(raw):
    """``raw`` without the trailing whitespace that ``str.strip`` removes from
    its text (Unicode whitespace such as U+00A0 or U+0085 included), as the
    library strips each segment before splitting it."""
    return raw.decode(ENCODING, "surrogateescape").rstrip().encode(ENCODING, "surrogateescape")


def readable(data):
    """Decodes ``data`` as the library's ``open()`` does, so a byte that the
    encoding cannot read fails here, before parsing, as it does in the library."""
    return str(data, ENCODING)


def text(value):
    """Text cell as ``str``; null and empty are ``None``. Where the library
    tells an element written empty from one the segment stops before,
    ``written`` reads the cell itself."""
    return None if not value else value.decode(ENCODING)


def money(value):
    """Decimal cell as ``float``, as the library's ``float(text)``."""
    return None if value is None else float(value)


def moment(value):
    """Date cell as a midnight ``datetime``, as the library's date parser."""
    return None if value is None else datetime.datetime(value.year, value.month, value.day)


def library_date(raw):
    """edi-835-parser's date parser on the element text: ``YYMMDDHHMM`` and
    ``CCYYMMDD`` become a ``datetime``, anything else stays text."""
    if len(raw) == 10:
        year, month, day, hour, minute = (int(raw[i:i + 2]) for i in range(0, 10, 2))
        return datetime.datetime(2000 + year, month, day, hour, minute)
    if len(raw) == 8:
        return datetime.datetime(int(raw[:4]), int(raw[4:6]), int(raw[6:]))
    return raw


DATE_FORMS = {10: "YYMMDDHHMM", 8: "CCYYMMDD"}


def element_date(segment, index, element, file_path):
    """The library's date parser on element ``element`` of ``segment``, the
    segment at ``index`` of ``file_path``. Text of a date's length that is not
    a date raises a ``ValueError`` naming the source, the segment, the element
    and the text."""
    elements = segment.elements
    if element > len(elements):
        return None
    raw = elements[element - 1]
    if element == len(elements) and isinstance(raw, bytes):
        raw = unpadded(raw)
    value = raw.decode(ENCODING) if isinstance(raw, bytes) else ""
    try:
        return library_date(value)
    except ValueError as error:
        name = f"{segment.id.decode(ENCODING)}{element:02d}"
        raise ValueError(
            f'{file_path}: segment {index} {name}: "{value}" is not a {DATE_FORMS[len(value)]} date, '
            f"which edi-835-parser's date parser requires of {len(value)} characters"
        ) from error


def date(value, document, index, element, file_path):
    """Date cell as ``datetime``. A null cell whose element is written holds
    text the date type rejects (an empty date, say), which the library keeps
    as text, so it is read from the element with the library's parser."""
    if value is not None or index is None:
        return moment(value)
    return element_date(document[index], index, element, file_path)


def integer(value):
    """Text cell as ``int`` when it reads as one, else the text, as the
    library's ``int()`` with its fallback; a decimal column cannot give this
    back, since it reads ``"1.0"`` and ``"1"`` as the same number. An empty
    cell is ``None``, as a null one."""
    if not value:
        return None
    value = text(value)
    try:
        return int(value)
    except ValueError:
        return value


def written(cell, convert=text, empty="", absent=None):
    """A text cell read as the library does: ``convert(cell)`` for text,
    ``empty`` for an element the segment holds empty (``b""``) and ``absent``
    for one the segment stops before (null)."""
    if cell is None:
        return absent
    return convert(cell) if cell else empty


def name(first, last):
    """``"first last"`` title-cased, with ``None`` spelled out as the library does."""
    return f"{first} {last}".title()


def ints(column):
    """An integer column as a NumPy array, with -1 for null."""
    import pyarrow.compute as pc

    return pc.fill_null(column, -1).to_numpy()
