"""Lossless, fast, data-driven EDI 835 parser.

``parse`` reads a whole file into a document, typed tables and diagnostics.
``stream`` yields the tables in batches, one per closed loop instance.
``write`` turns tables back into an interchange.
Tables export to Arrow through the PyCapsule interface, so Polars, pyarrow
or DuckDB read them without copying.
"""

from __future__ import annotations

import importlib.metadata
import os
from typing import Optional, Union

from ._core import (
    Batch,
    Delimiters,
    Diagnostic,
    Document,
    Envelope,
    Mask,
    ParseError,
    Result,
    Segment,
    Spec,
    SpecError,
    Stream,
    Table,
    Tables,
    Version,
    WriteFinding,
    parse,
    stream,
)
from ._write import WriteError, write

__version__ = importlib.metadata.version("oxedi")

__all__ = [
    "Batch",
    "Delimiters",
    "Diagnostic",
    "Document",
    "Envelope",
    "Mask",
    "ParseError",
    "Result",
    "Segment",
    "Spec",
    "SpecError",
    "Stream",
    "Table",
    "Tables",
    "Version",
    "WriteError",
    "WriteFinding",
    "__version__",
    "parse",
    "parse_file",
    "stream",
    "write",
]


def parse_file(
    path: Union[str, "os.PathLike[str]"],
    spec: Optional[Spec] = None,
    delimiters: Optional[Delimiters] = None,
    mask: Optional[Mask] = None,
) -> Result:
    """Reads the file at ``path`` in binary mode and parses it."""
    with open(path, "rb") as handle:
        data = handle.read()
    return parse(data, spec=spec, delimiters=delimiters, mask=mask)
