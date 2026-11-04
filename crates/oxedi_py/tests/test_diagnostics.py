import inspect

import pytest

import oxedi
from conftest import parse_named, read

PATH = "interchange#1/group#1/transaction#1/2000#1/2100#1/2110#1"


def first(name, kind):
    """The first diagnostic of ``kind`` on the named file."""
    return next(d for d in parse_named(name).diagnostics if d.kind == kind)


def test_a_diagnostic_carries_level_rule_location_and_datum():
    d = next(
        d
        for d in parse_named("multi_claim_sample.txt").diagnostics
        if d.kind == "RequiredElementMissing" and d.component == 2
    )
    assert (d.level, d.kind, d.segment, d.element, d.component) == (
        2,
        "RequiredElementMissing",
        25,
        1,
        2,
    )
    assert d.rule == "required element SVC01-2 (procedure_code) is missing or empty"
    assert d.path == PATH
    assert d.datum == b""
    assert str(d) == (
        "SNIP 2 · required element SVC01-2 (procedure_code) is missing or empty"
        f" · segment #25, element 1, component 2 · at {PATH} · datum \"\""
    )
    assert repr(d) == (
        "Diagnostic(level=2, origin='oxedi', kind='RequiredElementMissing', "
        "segment=25, "
        "element=1, component=2)"
    )


def test_an_unknown_segment_has_its_id_as_datum():
    d = first("multi_claim_sample.txt", "UnknownSegment")
    assert (d.level, d.kind, d.segment, d.element, d.datum) == (
        1,
        "UnknownSegment",
        19,
        None,
        b"N3",
    )


def test_a_finding_at_the_end_of_the_stream_has_no_segment():
    data = read("emedny_sample.txt")
    result = oxedi.parse(data[: data.index(b"SE*")])
    ends = [d for d in result.diagnostics if d.segment is None]
    assert len(ends) == 3
    assert [d.kind for d in ends] == ["UnterminatedLoop"] * 3
    assert all(" · end of stream · " in str(d) for d in ends)
    assert ends[0].path == "interchange#1/group#1/transaction#1"


def test_every_core_finding_has_the_core_origin_and_no_code():
    diagnostics = parse_named("multi_claim_sample.txt").diagnostics
    assert diagnostics
    assert {d.origin for d in diagnostics} == {"oxedi"}
    assert all(d.code is None for d in diagnostics)


def test_an_external_finding_is_a_diagnostic_with_its_origin_and_code():
    d = oxedi._core._external_diagnostic(
        "pyx12", "msg", 2, code="1", segment=3, element=2, datum=b"X"
    )
    assert isinstance(d, oxedi.Diagnostic)
    assert (d.kind, d.level, d.origin, d.code, d.path) == (
        "External",
        2,
        "pyx12",
        "1",
        "",
    )
    assert (d.segment, d.element, d.component, d.datum) == (3, 2, None, b"X")
    assert d.rule == "msg (reported by pyx12, code 1)"
    assert str(d) == (
        "SNIP 2 · msg (reported by pyx12, code 1) · segment #3, element 2"
        ' · at the root · datum "X"'
    )
    assert repr(d) == (
        "Diagnostic(level=2, origin='pyx12', kind='External', segment=3, "
        "element=2, component=None)"
    )


def test_external_diagnostic_signature_names_kinds_and_defaults():
    # The signature is written three times in the binding (`signature =`,
    # `text_signature` and the stub description); this pins the one Python
    # reports, and stubtest holds the stub to it.
    parameters = inspect.signature(oxedi._core._external_diagnostic).parameters
    keyword = inspect.Parameter.POSITIONAL_OR_KEYWORD
    empty = inspect.Parameter.empty
    assert [(p.name, p.kind, p.default) for p in parameters.values()] == [
        ("origin", keyword, empty),
        ("message", keyword, empty),
        ("level", keyword, empty),
        ("code", keyword, None),
        ("segment", keyword, None),
        ("element", keyword, None),
        ("component", keyword, None),
        ("datum", keyword, b""),
        ("path", keyword, None),
    ]


def test_external_diagnostic_takes_every_parameter_by_keyword():
    d = oxedi._core._external_diagnostic(
        origin="pyx12",
        message="msg",
        level=3,
        code="7",
        segment=4,
        element=2,
        component=1,
        datum=b"Z",
        path=[("interchange", 1), ("2000", 3)],
    )
    assert d.path == "interchange#1/2000#3"
    assert (d.origin, d.rule, d.level, d.code) == (
        "pyx12",
        "msg (reported by pyx12, code 7)",
        3,
        "7",
    )
    assert (d.segment, d.element, d.component, d.datum) == (4, 2, 1, b"Z")


def test_an_external_finding_without_code_or_position():
    d = oxedi._core._external_diagnostic("pyx12", "it failed", 1)
    assert (d.level, d.code, d.segment, d.element, d.datum) == (
        1,
        None,
        None,
        None,
        b"",
    )
    assert str(d) == (
        "SNIP 1 · it failed (reported by pyx12) · end of stream · at the root"
        ' · datum ""'
    )


@pytest.mark.parametrize("level", [0, 4, -1])
def test_an_external_level_outside_one_to_three_is_refused(level):
    with pytest.raises(ValueError, match=f"^level must be 1, 2 or 3, got {level}$"):
        oxedi._core._external_diagnostic("pyx12", "msg", level)


@pytest.mark.parametrize("argument", ["segment", "element", "component"])
@pytest.mark.parametrize("value", [-1, 2**64, 10**30])
def test_external_position_out_of_range_names_the_argument(argument, value):
    with pytest.raises(ValueError, match=rf"^{argument} must be a non-negative integer, got {value}$"):
        oxedi._core._external_diagnostic("pyx12", "msg", 1, **{argument: value})


@pytest.mark.parametrize("value", [-(2**63) - 1, 2**63, 99999999999999999999])
def test_external_level_beyond_i64_is_a_value_error(value):
    with pytest.raises(ValueError, match=rf"^level must be 1, 2 or 3, got {value}$"):
        oxedi._core._external_diagnostic("pyx12", "msg", value)


# Rules parse reports while a loop opens or closes: their path is the stack
# at that moment (the loop being opened, or the one being closed), not the
# loops that hold the segment.
TRANSITIONS = {"ImplicitLoop", "UnterminatedLoop", "RequiredOccurrenceMissing", "RequiredLoopMissing"}


def test_parse_document_gives_the_document_the_spec_and_the_leading_length():
    data = b"\xef\xbb\xbf\r\n" + read("multi_claim_sample.txt")
    document, spec, leading = oxedi._core._parse_document(data)
    parsed = oxedi.parse(data)
    assert leading == 3 + 2
    assert spec.to_json() == parsed.spec.to_json()
    assert document.write() == parsed.document.write() == data
    patched = oxedi.Spec.builtin().patch({"loops": {"ZZ": {"parent": "1000A", "trigger": {"segment": "N3"}}}})
    assert oxedi._core._parse_document(data, patched)[1].to_json() == patched.to_json()


def test_parse_document_refuses_input_without_an_isa_as_parse_does():
    with pytest.raises(oxedi.ParseError) as info:
        oxedi._core._parse_document(b"hello")
    with pytest.raises(oxedi.ParseError) as expected:
        oxedi.parse(b"hello")
    assert str(info.value) == str(expected.value)


def test_loop_paths_name_the_loops_parse_names_at_each_segment(file_name):
    result = parse_named(file_name)
    named = [
        d for d in result.diagnostics if d.segment is not None and d.kind not in TRANSITIONS
    ]
    paths = oxedi._core._loop_paths(result.document, result.spec, [d.segment for d in named])
    for diagnostic, path in zip(named, paths):
        built = oxedi._core._external_diagnostic("x", "m", 1, segment=diagnostic.segment, path=path)
        assert built.path == diagnostic.path, (diagnostic.segment, str(diagnostic))


def test_loop_paths_count_instances_from_the_start_of_the_stream():
    result = parse_named("multi_claim_sample.txt")
    document = result.document
    clps = [i for i in range(len(document)) if document[i].id == b"CLP"]
    paths = oxedi._core._loop_paths(result.document, result.spec, [0] + clps)
    assert paths[0] == [("interchange", 1)]
    claims = [path[-1] for path in paths[1:]]
    assert claims == [("2100", n) for n in range(1, len(clps) + 1)]


def test_loop_paths_of_no_segment_is_empty():
    result = parse_named("multi_claim_sample.txt")
    assert oxedi._core._loop_paths(result.document, result.spec, []) == []


def test_loop_paths_keep_the_order_and_repeats_asked_for():
    result = parse_named("multi_claim_sample.txt")
    every = oxedi._core._loop_paths(result.document, result.spec, list(range(len(result.document))))
    picked = [5, 0, 5, 2]
    assert oxedi._core._loop_paths(result.document, result.spec, picked) == [every[i] for i in picked]


def test_loop_paths_refuse_a_segment_past_the_last():
    result = parse_named("multi_claim_sample.txt")
    count = len(result.document)
    with pytest.raises(ValueError) as info:
        oxedi._core._loop_paths(result.document, result.spec, [0, count])
    assert str(info.value) == (
        f"_loop_paths: segment {count} is past the document's last segment ({count} segments)"
    )
