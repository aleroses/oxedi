"""``validate``: run pyx12's validation in-process and translate its error tree."""

from __future__ import annotations

import io
import os
from bisect import bisect_right
from typing import Any, BinaryIO, Optional, Union

from .. import Diagnostic, Document, Mask, Spec
from .._core import _external_diagnostic, _loop_paths, _mask_findings, _parse_document
from . import _capture
from ._tree import Finding, collect

# The arguments of one finding for ``_external_diagnostic``, but its loop path.
Draft = dict[str, Any]

Source = Union[bytes, bytearray, memoryview, str, "os.PathLike[str]", BinaryIO]

_ORIGIN = "pyx12"
_FAILURE_LEVEL = 1


def _import_pyx12() -> Any:
    try:
        import pyx12.error_handler
        import pyx12.params
        import pyx12.x12n_document
    except ImportError as err:
        raise ImportError(
            "oxedi.pyx12 needs the pyx12 package: pip install 'oxedi[pyx12]'"
        ) from err
    _capture.install(pyx12.error_handler)
    return pyx12


def _read(source: Source) -> bytes:
    if isinstance(source, (bytes, bytearray, memoryview)):
        return bytes(source)
    if isinstance(source, (str, os.PathLike)):
        with open(source, "rb") as handle:
            return handle.read()
    data = source.read()
    if isinstance(data, str):
        raise TypeError(
            "validate needs bytes, a path, or a file opened in binary mode; "
            "the file returned str"
        )
    return data


class _Positions:
    """Maps pyx12's segment numbers to segment indexes of the document.

    pyx12 splits its input on the segment terminator and drops the CR and LF
    that follow it, and counts the pieces from the ISA on; the same split is
    done here to get the offset of each piece, and the document segment whose
    raw bytes hold that offset is the segment pyx12 meant. The document's own
    segments hold their leading trivia, so a byte order mark or blank lines
    before the ISA shift nothing.
    """

    def __init__(self, ends: list[int], base: int, starts: list[int]):
        self.ends = ends
        self.base = base
        self.piece_starts = starts

    @classmethod
    def build(cls, document: Any, base: int, text: str) -> "_Positions":
        ends: list[int] = []
        offset = 0
        for index in range(len(document)):
            offset += len(document[index].raw)
            ends.append(offset)
        terminator = bytes(document.delimiters.segment).decode("latin-1")
        starts: list[int] = []
        position = 0
        while True:
            found = text.find(terminator, position)
            if found == -1:
                break
            piece = text[position:found]
            stripped = piece.lstrip("\n\r")
            if stripped == "":
                break
            starts.append(position + len(piece) - len(stripped))
            position = found + 1
        return cls(ends, base, starts)

    def locate(self, line: Optional[int]) -> Optional[int]:
        """The segment index of pyx12's 1-based segment ``line``."""
        if line is None or not 1 <= line <= len(self.piece_starts):
            return None
        offset = self.base + self.piece_starts[line - 1]
        index = bisect_right(self.ends, offset)
        return index if index < len(self.ends) else None


class _Run:
    """What one run of pyx12 left: its verdict, the error handler it built,
    the segment numbers it finished, and the exception it raised, if any."""

    def __init__(self) -> None:
        self.ok = False
        self.handler: Any = None
        self.track: list[int] = []
        self.failure: Optional[BaseException] = None

    def refused(self, found: list[Finding]) -> bool:
        """Whether pyx12 refused the file: it found nothing, did not accept
        it and finished no segment."""
        return not (found or self.ok or self.track)


def _run(text: str) -> _Run:
    import pyx12.params
    import pyx12.x12n_document

    run = _Run()

    def callback(_seg: Any, src: Any, _node: Any, _valid: bool) -> None:
        run.track.append(src.get_cur_line())

    with _capture.recording() as handlers:
        try:
            # the base class holds the defaults and reads no configuration file
            run.ok = pyx12.x12n_document.x12n_document(
                param=pyx12.params.ParamsBase(),
                src_file=io.StringIO(text),
                fd_997=None,
                fd_html=None,
                callback=callback,
            )
        except Exception as err:  # pyx12 is third-party: any failure is a finding
            run.failure = err
    run.handler = handlers[0] if handlers else None
    return run


def _datum(value: Any) -> bytes:
    return b"" if value is None else str(value).encode("latin-1", "replace")


def _diagnostic(found: Finding, positions: _Positions, document: Any) -> Draft:
    segment = positions.locate(found.line)
    datum = _datum(found.value)
    if not found.value and segment is not None:
        if found.element is not None:
            separator = bytes(document.delimiters.component)
            datum = _element_bytes(document[segment], found.element, found.component, separator)
        else:
            datum = bytes(document[segment].id)
    return dict(
        message=found.text,
        level=found.level,
        code=found.code,
        segment=segment,
        element=found.element,
        component=found.component,
        datum=datum,
    )


def _element_bytes(
    segment: Any, element: int, component: Optional[int], separator: bytes
) -> bytes:
    """The bytes of a 1-based element, or of one of its components, of
    ``segment``, a composite's components joined by ``separator``; empty
    when the segment has no such position."""
    value = segment.element(element, component)
    if value is None:
        return b""
    return separator.join(value) if isinstance(value, list) else value


def _completed(track: list[int], positions: _Positions) -> Optional[int]:
    return positions.locate(track[-1]) if track else None


def _failure(reason: str, track: list[int], positions: _Positions, document: Any) -> Draft:
    """One failure finding, with no code: pyx12 did not report it, it stopped.

    ``track`` holds the segment numbers pyx12 finished. When a segment
    follows the last of them, that is the one pyx12 was working on; when
    none does, pyx12 had read the whole file and the finding names the last
    segment it completed."""
    done = _completed(track, positions)
    current = positions.locate(track[-1] + 1 if track else 1)
    if current is not None:
        segment: Optional[int] = current
        where = f"it was processing segment #{current}" + (
            f"; the last it completed is #{done}" if done is not None else ""
        )
    elif done is not None:
        segment = done
        where = f"it had read every segment; the last it completed is #{done}"
    else:
        segment = None
        where = "it reached no segment"
    datum = bytes(document[segment].id) if segment is not None else b""
    return dict(
        message=f"could not finish validating: {reason}; {where}",
        level=_FAILURE_LEVEL,
        segment=segment,
        datum=datum,
        own_text=True,
    )


def _rejection(text: str, positions: _Positions, document: Document, masked: bool) -> Draft:
    """The failure for a file pyx12 refuses to read as X12: at the segment
    holding the ISA, with its bytes as the datum, or, when ``masked``, only
    the segment id, as the bytes name no element a mask could reach. pyx12
    only logs that it refused, so the reason is read again from its reader."""
    import pyx12.errors
    import pyx12.rawx12file

    reason = "pyx12 does not read it as an X12 file"
    try:
        pyx12.rawx12file.RawX12File(io.StringIO(text))
    except pyx12.errors.X12Error as err:
        reason = f"X12Error: {str(err).strip()}"
    segment = positions.locate(1)
    datum = b""
    if segment is not None:
        if masked:
            datum = bytes(document[segment].id)
        else:
            # The segment of line 1 is segment 0, whose raw bytes start with
            # the leading trivia the document reports before the ISA.
            datum = bytes(document[segment].raw)[positions.base:].rstrip(b"\r\n")
    where = f"it rejected segment #{segment}" if segment is not None else "it reached no segment"
    return dict(
        message=f"could not finish validating: {reason}; {where}",
        level=_FAILURE_LEVEL,
        segment=segment,
        datum=datum,
    )


def _by_segment(draft: Draft) -> tuple[bool, int]:
    return (draft["segment"] is None, draft["segment"] or 0)


def _arguments(draft: Draft) -> Draft:
    """The arguments of ``_external_diagnostic`` that ``draft`` holds."""
    return {k: v for k, v in draft.items() if k != "own_text"}


def _finish(
    document: Document, spec: Spec, drafts: list[Draft]
) -> tuple[list[Diagnostic], list[bool]]:
    """The diagnostics of ``drafts`` in segment order, each with the loop
    path ``parse`` gives its segment, and, for each, whether its message is
    text oxedi wrote (a failure finding) rather than pyx12's."""
    drafts = sorted(drafts, key=_by_segment)
    segments = [d["segment"] for d in drafts if d["segment"] is not None]
    paths = iter(_loop_paths(document, spec, segments))
    found = [
        _external_diagnostic(
            _ORIGIN,
            path=next(paths) if d["segment"] is not None else None,
            **_arguments(d),
        )
        for d in drafts
    ]
    return found, [d.get("own_text", False) for d in drafts]


def validate(
    source: Source, mask: Optional[Mask] = None, spec: Optional[Spec] = None
) -> list[Diagnostic]:
    """Validates an 835 file with pyx12 and returns what it finds.

    ``source`` is the file's bytes, a path, or a file object open in binary
    mode. pyx12 picks its map from the file's own version declaration.
    pyx12's own defaults apply: extended character set (``charset="E"``),
    no external code lists excluded (``exclude_external_codes=None``),
    and user/system configuration files (``~/.pyx12.conf.xml``,
    ``<prefix>/etc/pyx12.conf.xml``) are not read, so findings do not
    depend on the machine.

    Every error pyx12's engine records is returned: interchange, group and
    transaction errors, segment errors and element errors, including those
    of the envelope segments. They are read from the error tree pyx12
    builds; pyx12's logging is not used. The ``pyx12`` logger gets a
    ``logging.NullHandler`` the first time, when it has none, so pyx12's
    records are not printed by default and still reach the handlers the
    caller configures; levels, propagation and ``logging.disable`` stay as
    the caller set them.

    Each finding is an ``oxedi.Diagnostic``, the type ``parse``
    returns, so both lists mix, sort by ``level`` and filter by ``origin``.
    A pyx12 finding has ``kind == "External"``, ``origin == "pyx12"`` and
    ``code`` set to pyx12's own error code; ``rule`` is pyx12's message.
    ``level`` follows where pyx12 reports it: interchange, group and
    transaction findings are level 1, segment and element findings level 2.
    ``segment`` is the index of the segment at fault in the file's document
    (``None`` when pyx12 names none), and ``document[d.segment].span`` gives
    its byte range. An interchange, group or transaction finding about a
    trailer (a count or a control number that does not match) lands on the
    trailer segment at the element holding that value; one about a
    duplicate or missing control structure lands on the header, at its
    control number when that is the offending value. ``datum`` is the value
    pyx12 reports, or else the bytes of the element in the file, or the
    segment id for a finding with no element. Findings come in
    segment order. ``path`` names the loops open at that segment, as in the
    diagnostics ``parse`` gives about the same segment.

    A file pyx12 cannot read, or an exception inside pyx12, gives one
    level 1 finding with no ``code`` whose ``rule`` starts with ``could
    not finish validating``, instead of raising: a file pyx12 rejects
    points at the segment holding the ISA with its bytes as the datum; a
    failure while reading names the segment pyx12 was processing, and one
    after the last segment names the last segment it completed. A file
    with no ISA to read the delimiters from raises ``oxedi.ParseError``,
    as ``oxedi.parse`` does. Nothing is written to disk and no
    acknowledgement is generated.

    With ``mask``, an ``oxedi.Mask``, the datum of a finding at a place a
    column flags as patient data reads is masked as ``parse`` masks its own
    diagnostics with that mask. In the finding's message, the original
    datum and each value its segment holds at such a place are replaced by
    their masked forms, whether or not the finding names an element. A
    value of 4 characters or more is replaced wherever it occurs. A shorter
    one is replaced only where it stands apart: the character on each side
    is the edge of the message, a character that is not an ASCII letter or
    digit (an accented letter or ``-`` counts), or part of another replaced
    value. Short values that overlap or touch are replaced as one group when
    the group stands apart. A short value glued to letters or digits that
    are not flagged values stays as written (with ``XAB`` and ``ABYZ``
    flagged, ``QXABYZ`` keeps ``QX``), and two short values that together
    spell a word of the message replace that word. Every other attribute,
    and the findings' number and order, stay as without it. The flagged
    places are those of the spec the parse uses: ``spec``, an
    ``oxedi.Spec``, or the built-in spec of the file's version when it is
    ``None``, so a patched spec that flags more columns masks them here as
    ``parse(source, spec=spec, mask=mask)`` does. The same spec locates the
    findings' loop paths, as in ``parse(source, spec=spec)``. A failure
    finding for an exception inside pyx12, whose text no flagged place
    points at, keeps the exception's type and drops its text: ``could not
    finish validating: ValueError, whose message is not shown with a mask;
    ...``, or ``its error tree could not be read (ValueError, whose message
    is not shown with a mask)``. The rest of such a finding is text oxedi
    wrote, so the mask does not rewrite it; its datum is masked as any other.
    The finding for a file pyx12 rejects quotes pyx12's X12Error, which can
    hold the whole ISA line, and its message is masked as pyx12's messages are;
    its datum is ``ISA`` instead of the ISA line, which names no element.
    pyx12 still reads the original file: the
    mask hides what ``validate`` returns, not what pyx12 is given. ``mask``
    of any other type than ``oxedi.Mask`` or ``None`` raises ``TypeError``,
    and so does ``spec`` of any other type than ``oxedi.Spec`` or ``None``.
    """
    if mask is not None and not isinstance(mask, Mask):
        raise TypeError(
            f"validate: mask must be an oxedi.Mask or None, not {type(mask).__name__}"
        )
    if spec is not None and not isinstance(spec, Spec):
        raise TypeError(
            f"validate: spec must be an oxedi.Spec or None, not {type(spec).__name__}"
        )
    _import_pyx12()
    data = _read(source)
    document, used, base = _parse_document(data, spec)
    findings, own = _findings(data, document, used, base, masked=mask is not None)
    if mask is None:
        return findings
    return _mask_findings(findings, document, mask, used, own)


def _reason(err: BaseException, masked: bool) -> str:
    """How a failure finding names the exception pyx12 raised: its type and
    its text, or, when ``masked``, its type and a note that its text is not
    shown, as the text may quote a value of the file that no flagged place
    can point at."""
    if masked:
        return f"{type(err).__name__}, whose message is not shown with a mask"
    return f"{type(err).__name__}: {err}"


def _problem(run: _Run, masked: bool) -> Optional[str]:
    """Why pyx12's run left nothing to read, or ``None`` when it left an
    error handler."""
    if run.failure is not None:
        return _reason(run.failure, masked)
    if run.handler is None:
        return "pyx12 built no error handler"
    return None


def _collected(handler: Any, masked: bool) -> tuple[list[Finding], Optional[str]]:
    """The findings of pyx12's error tree, or why it could not be read."""
    try:
        return collect(handler), None
    except Exception as err:  # pyx12's tree is not ours: report it, do not raise
        return [], f"its error tree could not be read ({_reason(err, masked)})"


def _drafts(
    text: str, positions: _Positions, document: Document, masked: bool
) -> list[Draft]:
    """The findings of a run of pyx12 on ``text``; when it could not finish
    or its tree could not be read, the one failure finding that says so."""
    run = _run(text)
    reason = _problem(run, masked)
    found: list[Finding] = []
    if reason is None:
        found, reason = _collected(run.handler, masked)
    if reason is not None:
        return [_failure(reason, run.track, positions, document)]
    if run.refused(found):
        return [_rejection(text, positions, document, masked)]
    return [_diagnostic(f, positions, document) for f in found]


def _findings(
    data: bytes, document: Document, spec: Spec, base: int, masked: bool
) -> tuple[list[Diagnostic], list[bool]]:
    """pyx12's findings about ``data``, whose ISA starts at byte ``base``
    and whose document and spec are given, and for each whether its message
    is text oxedi wrote; with ``masked``, failure findings leave out the
    text of pyx12's exception."""
    text = data[base:].decode("latin-1")
    positions = _Positions.build(document, base, text)
    return _finish(document, spec, _drafts(text, positions, document, masked))
