"""The public API as a user writes it, checked with ``mypy --strict``.

``make stubtest`` type-checks this file; it is not a test module and pytest
does not collect it. Every annotation below states what the stub promises, so
a stub that loosens or changes a type makes the check fail.
"""

from __future__ import annotations

import datetime
import decimal
from collections.abc import Iterator

import polars

import oxedi
from oxedi import (
    Batch,
    Delimiters,
    Diagnostic,
    Document,
    Envelope,
    ParseError,
    Result,
    Segment,
    Spec,
    SpecError,
    Stream,
    Table,
    Tables,
    Version,
    WriteError,
    WriteFinding,
)
from oxedi.pyx12 import validate


def results(data: bytes, path: str) -> list[Result]:
    spec: Spec = Spec.builtin(version=Version.V4010).patch({"name": "x"}).patch('{"name": "y"}')
    loops: list[str] = Spec.from_json(spec.to_json()).loops()
    assert loops
    schema_names: list[str] = list(spec.schema())
    assert schema_names
    delimiters = Delimiters(element=b"*", component=b":", segment=b"~", repetition=None, release=None)
    return [
        oxedi.parse(data),
        oxedi.parse(bytearray(data), spec=spec, delimiters=delimiters),
        oxedi.parse(memoryview(data), spec=None, delimiters=None),
        oxedi.parse_file(path),
        oxedi.parse_file(path, spec=Spec.builtin(), delimiters=Delimiters()),
    ]


def result_fields(result: Result) -> None:
    document: Document = result.document
    tables: Tables = result.tables
    diagnostics: list[Diagnostic] = result.diagnostics
    claims: int = result.count_claims()
    patients: int = result.count_patients()
    total: decimal.Decimal = result.sum_payments()
    payer: dict[str, str | None] | None = result.payer
    payee: dict[str, str | None] | None = result.payee
    print(document, tables, diagnostics, claims, patients, total, payer, payee, repr(result))


def documents(document: Document) -> bytes:
    delimiters: Delimiters = document.delimiters
    element: bytes = delimiters.element
    repetition: bytes | None = delimiters.repetition
    release: bytes | None = delimiters.release
    print(element, delimiters.component, delimiters.segment, repetition, release)
    count: int = len(document)
    segment: Segment = document[0]
    last: Segment = document[count - 1]
    index: int = segment.index
    ident: bytes = segment.id
    elements: list[bytes | list[bytes]] = segment.elements
    raw: bytes = last.raw
    span: tuple[int, int] = segment.span
    start, end = span
    print(index, ident, elements, raw, start + end)
    return document.write()


def diagnostic_attributes(diagnostic: Diagnostic) -> str:
    level: int = diagnostic.level
    origin: str = diagnostic.origin
    code: str | None = diagnostic.code
    kind: str = diagnostic.kind
    rule: str = diagnostic.rule
    segment: int | None = diagnostic.segment
    element: int | None = diagnostic.element
    component: int | None = diagnostic.component
    path: str = diagnostic.path
    datum: bytes = diagnostic.datum
    print(level, origin, code, kind, rule, segment, element, component, path, datum)
    return str(diagnostic)


def table_exports(tables: Tables) -> None:
    names: list[str] = tables.keys()
    has_claims: bool = "claims" in tables
    count: int = len(tables)
    iterated: Iterator[str] = iter(tables)
    claims: Table = tables["claims"]
    name: str = claims.name
    columns: list[str] = claims.columns
    rows: int = len(claims)
    text: str = claims.render() + tables.render()
    frame: polars.DataFrame = claims.to_polars()
    frames: dict[str, polars.DataFrame] = tables.to_polars()
    stream_capsule = claims.__arrow_c_stream__()
    schema_capsule = claims.__arrow_c_schema__()
    array_capsules = claims.__arrow_c_array__(requested_schema=None)
    pandas_frames = tables.to_pandas()
    print(names, has_claims, count, next(iterated), name, columns, rows, text, frame, frames)
    print(stream_capsule, schema_capsule, array_capsules, claims.to_pandas(), pandas_frames)


def streams(data: bytes) -> int:
    batches: Stream = oxedi.stream(data, spec=None, by="claim", delimiters=None)
    total = 0
    for batch in batches:
        checked: Batch = batch
        tables: Tables = checked.tables
        diagnostics: list[Diagnostic] = checked.diagnostics
        total += len(tables) + len(diagnostics)
    return total


def errors(data: bytes) -> str:
    try:
        oxedi.parse(data)
    except ParseError as error:
        problem: ValueError = error
        return str(problem)
    try:
        Spec.from_json("{}")
    except SpecError as error:
        return str(error)
    return ""


def validated(data: bytes, path: str) -> list[Diagnostic]:
    with open(path, "rb") as handle:
        from_file: list[Diagnostic] = validate(handle)
    return validate(data) + validate(path) + from_file


def writes(result: Result, frames: dict[str, polars.DataFrame]) -> bytes:
    envelope = Envelope(
        sender_id="SENDER",
        receiver_id="RECEIVER",
        date=datetime.date(2024, 1, 1),
        time=datetime.time(12, 30),
        sender_qualifier="ZZ",
        receiver_qualifier="ZZ",
        usage_indicator="T",
        control_number=1,
        application_sender=None,
        application_receiver=None,
        delimiters=Delimiters(repetition=b"^"),
        line_break=False,
    )
    sender: str = envelope.sender_id
    print(sender, envelope.sender_qualifier, envelope.receiver_id, envelope.receiver_qualifier)
    print(envelope.usage_indicator, envelope.delimiters, repr(envelope))
    number: int = envelope.control_number
    day: datetime.date = envelope.date
    moment: datetime.time = envelope.time
    app: str | None = envelope.application_sender
    other: str | None = envelope.application_receiver
    broken: bool = envelope.line_break
    print(day, moment, app, other, broken)
    data: bytes = oxedi.write(result.tables, envelope)
    parsed_with: Spec = result.spec
    tables_spec: Spec = result.tables.spec
    again: bytes = oxedi.write(frames, envelope, spec=result.spec)
    print(parsed_with, tables_spec, again)
    strict: bytes = oxedi.write(frames, envelope, spec=Spec.builtin(), allow_findings=False)
    allowed: tuple[bytes, list[WriteFinding]] = oxedi.write(frames, envelope, allow_findings=True)
    written, findings = allowed
    for finding in findings:
        kind: str = finding.kind
        table: str | None = finding.table
        row: int | None = finding.row
        column: str | None = finding.column
        field: str | None = finding.field
        diagnostic: Diagnostic | None = finding.diagnostic
        message: str = finding.message
        print(kind, table, row, column, field, diagnostic, message, str(finding), repr(finding))
    try:
        oxedi.write(frames, envelope)
    except WriteError as error:
        problem: ValueError = error
        reasons: list[WriteFinding] = error.findings
        print(problem, reasons)
    return data + strict + written + bytes(number)


version: str = oxedi.__version__


def masked(data: bytes) -> Result:
    mask: oxedi.Mask = oxedi.Mask(b"0123456789abcdef", dates="shift")
    batches: Stream = oxedi.stream(data, mask=oxedi.Mask("0123456789abcdef"))
    assert batches
    return oxedi.parse(data, mask=mask)
