"""The compat spec: the built-in spec with the edi-835-parser tables in place of the native ones."""

from __future__ import annotations

import functools
import json

from .. import Spec
from .._core import EDI_835_PARSER_PATCH

PATCH = json.loads(EDI_835_PARSER_PATCH)
TABLES = sorted(name for name, table in PATCH["tables"].items() if table is not None)


@functools.cache
def spec() -> Spec:
    """The built-in spec with the edi-835-parser tables in place of the native ones."""
    return Spec.builtin().patch(EDI_835_PARSER_PATCH)
