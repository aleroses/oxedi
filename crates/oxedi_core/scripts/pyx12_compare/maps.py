"""Reading pyx12's transaction map, data elements and code sets."""

from __future__ import annotations

import sys
import xml.etree.ElementTree as ET
from dataclasses import dataclass, field
from pathlib import Path


@dataclass
class MapElement:
    """One element (or component) of one segment at one place in the map."""

    seq: int
    data_ele: str
    name: str
    usage: str
    # None: any value (no list, an empty list, or an external code set).
    codes: tuple[str, ...] | None
    external: str | None
    components: dict[int, MapElement] = field(default_factory=dict)

    @property
    def composite(self):
        return bool(self.components)


@dataclass
class MapSegment:
    """One place a segment appears in the map."""

    id: str
    name: str
    usage: str
    loop: str
    elements: dict[int, MapElement]
    # Position in its spec loop (see ``map_position``); max_use, None for ">1".
    pos: int = 0
    max_use: int | None = None


@dataclass
class MapLoop:
    xid: str
    kind: str | None
    parent: str | None
    segments: list[MapSegment]
    # The loop's repeat; None for ">1" or when the map gives none.
    repeat: int | None = None
    usage: str = ""


def _count(text):
    """A map count: ``>1`` (or nothing) is no limit, otherwise the number."""
    text = (text or "").strip()
    return int(text) if text.isdigit() else None


def map_position(text, table=0):
    """A map position as the spec writes it. 4010 maps write positions with
    three digits and 5010 maps with four (``030`` is ``0300``), so a 4010
    position is scaled by ten and both versions number alike. The wrapper
    tables of a transaction (header, detail, footer) restart their
    positions, so a segment inside the n-th table, or inside a loop nested in
    it, gets ``n * 10000`` added: the transaction and every loop below it
    share one position space."""
    text = (text or "").strip()
    pos = int(text) if text.isdigit() else 0
    if len(text) == 3:
        pos *= 10
    return table * 10000 + pos


def _element(node):
    codes_node = node.find("valid_codes")
    external = None
    codes = None
    if codes_node is not None:
        external = codes_node.get("external")
        # An empty <code/> names no value; a list of only empty codes is open.
        listed = tuple(
            text for text in ((code.text or "").strip() for code in codes_node.findall("code"))
            if text
        )
        if external is None and listed:
            codes = listed
    element = MapElement(
        seq=int(node.findtext("seq")),
        data_ele=node.findtext("data_ele") or "",
        name=node.findtext("name") or "",
        usage=node.findtext("usage") or "",
        codes=codes,
        external=external,
    )
    if node.tag == "composite":
        for child in node.findall("element"):
            component = _element(child)
            element.components[component.seq] = component
    return element


def load_map(path):
    """Every loop of a pyx12 transaction map, in document order."""
    root = ET.parse(path).getroot()
    loops = []

    def walk(node, parent, inherited=0):
        tables = 0
        for child in node:
            if child.tag != "loop":
                continue
            xid = child.get("xid")
            table = inherited
            if child.get("type") == "wrapper":
                tables += 1
                table = tables
            segments = []
            for seg in child.findall("segment"):
                elements = {}
                for el in seg:
                    if el.tag in ("element", "composite"):
                        parsed = _element(el)
                        elements[parsed.seq] = parsed
                segments.append(
                    MapSegment(
                        id=seg.get("xid"),
                        name=seg.findtext("name") or "",
                        usage=seg.findtext("usage") or "",
                        loop=xid,
                        elements=elements,
                        pos=map_position(seg.findtext("pos"), table),
                        max_use=_count(seg.findtext("max_use")),
                    )
                )
            loops.append(MapLoop(xid, child.get("type"), parent, segments,
                                 _count(child.findtext("repeat")),
                                 (child.findtext("usage") or "").strip()))
            walk(child, xid, table)

    walk(root, None)
    return loops


def load_dataele(path):
    """Data element number -> (type, min, max)."""
    root = ET.parse(path).getroot()
    return {
        node.get("ele_num"): (
            node.get("data_type"),
            int(node.get("min_len")),
            int(node.get("max_len")),
        )
        for node in root.iter("data_ele")
    }


def load_codesets(path):
    """External code set id -> number of codes ``codes.xml`` lists for it."""
    root = ET.parse(path).getroot()
    return {
        node.findtext("id"): len(node.findall("./version/code"))
        for node in root.iter("codeset")
    }


def pyx12_maps():
    """The folder of the installed pyx12 maps; exits with a message when
    pyx12 is not installed."""
    try:
        from importlib.resources import files

        return Path(str(files("pyx12") / "map"))
    except ModuleNotFoundError:
        sys.exit("pyx12 is not installed: pip install 'pyx12>=4.0,<4.0.1'")
