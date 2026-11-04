import inspect
import mmap
from array import array

import pytest

import oxedi
from conftest import LARGEST, delimiters_for, parse_named, path_of, read

EMEDNY = "emedny_sample.txt"


def test_every_file_is_written_back_byte_for_byte(file_name):
    data = read(file_name)
    document = parse_named(file_name).document
    assert document.write() == data
    assert b"".join(document[i].raw for i in range(len(document))) == data


def test_segments_expose_index_id_elements_and_raw():
    document = parse_named(EMEDNY).document
    assert (document[0].index, document[0].id) == (0, b"ISA")
    assert document[0].elements[15] == [b"", b""]
    svc = next(
        document[i] for i in range(len(document)) if document[i].id == b"SVC"
    )
    assert isinstance(svc.elements[0], list)
    assert svc.elements[0][0] == b"HC"
    clps = [document[i] for i in range(len(document)) if document[i].id == b"CLP"]
    assert clps
    for clp in clps:
        assert all(isinstance(e, (bytes, list)) for e in clp.elements)
    assert document[-1].index == len(document) - 1


def _segment(body):
    delimiters = oxedi.Delimiters(release=b"?")
    return oxedi.parse(body, delimiters=delimiters).document[0]


def test_element_reads_a_simple_a_composite_and_a_component():
    segment = _segment(b"XX*a*b:c:d*~")
    assert segment.element(1) == b"a"
    assert segment.element(2) == [b"b", b"c", b"d"]
    assert segment.element(2, 2) == b"c"
    assert segment.element(3) == b""
    assert segment.has(1) and segment.has(2, 3) and segment.has(3)


def test_element_places_the_segment_does_not_have_are_none():
    segment = _segment(b"XX*a*b:c:d~")
    assert segment.element(1, 1) == b"a"
    assert segment.element(1, 2) is None
    assert segment.element(2, 4) is None
    assert segment.element(3) is None
    assert not segment.has(1, 2)
    assert not segment.has(2, 4)
    assert not segment.has(3)


def test_a_release_escaped_value_equals_the_elements_entry():
    segment = _segment(b"XX*a?*b~")
    assert segment.elements == [b"a*b"]
    assert segment.element(1) == segment.elements[0]


@pytest.mark.parametrize("position", [0, -1])
def test_element_refuses_a_position_below_one(position):
    segment = _segment(b"XX*a~")
    message = f"Segment.element: position must be 1 or more (XX01 is 1), got {position}"
    with pytest.raises(ValueError) as caught:
        segment.element(position)
    assert str(caught.value) == message
    with pytest.raises(ValueError) as caught:
        segment.has(position)
    assert str(caught.value) == message.replace("Segment.element", "Segment.has")


def test_element_with_an_integer_beyond_64_bits():
    segment = _segment(b"XX*a~")
    assert segment.element(2**70) is None
    assert not segment.has(1, 2**70)
    with pytest.raises(ValueError) as caught:
        segment.element(-(2**70))
    assert str(caught.value) == (
        f"Segment.element: position must be 1 or more (XX01 is 1), got {-(2**70)}"
    )
    with pytest.raises(TypeError):
        segment.element("1")


@pytest.mark.parametrize("component", [0, -3])
def test_element_refuses_a_component_below_one(component):
    segment = _segment(b"XX*a~")
    message = f"Segment.element: component must be 1 or more, got {component}"
    with pytest.raises(ValueError) as caught:
        segment.element(1, component)
    assert str(caught.value) == message
    with pytest.raises(ValueError) as caught:
        segment.has(1, component)
    assert str(caught.value) == message.replace("Segment.element", "Segment.has")


def test_an_index_past_the_end_names_the_length():
    document = parse_named(EMEDNY).document
    n = len(document)
    with pytest.raises(IndexError) as info:
        document[n]
    assert str(info.value) == (
        f"segment index {n} is out of range: the document has {n} segments"
    )


def test_the_delimiters_are_read_from_the_isa():
    delimiters = parse_named(EMEDNY).document.delimiters
    assert (delimiters.element, delimiters.component, delimiters.segment) == (
        b"*",
        b":",
        b"~",
    )
    assert delimiters.repetition == b"^"


def test_a_byte_order_mark_is_kept_in_the_first_segment_and_reported(file_name):
    data = read(file_name)
    if not data.lstrip().startswith(b"ISA"):
        pytest.skip("the file has no ISA to read the delimiters from")
    mark = b"\xef\xbb\xbf"
    plain, marked = oxedi.parse(data), oxedi.parse(mark + data)
    assert marked.document.write() == mark + data
    assert marked.document[0].raw == mark + plain.document[0].raw
    assert marked.document[0].id == b"ISA"
    assert len(marked.document) == len(plain.document)
    assert marked.tables.render() == plain.tables.render()
    first, *rest = marked.diagnostics
    assert (first.level, first.kind, first.segment, first.path, first.datum) == (
        1,
        "ByteOrderMark",
        0,
        "",
        mark,
    )
    assert str(first) == (
        "SNIP 1 · the input starts with a UTF-8 byte order mark, kept as leading "
        "trivia of the first segment · segment #0 · at the root · datum \"\\u{feff}\""
    )
    assert [str(d) for d in rest] == [str(d) for d in plain.diagnostics]


def test_input_without_an_isa_raises_parse_error_with_the_core_message():
    with pytest.raises(oxedi.ParseError) as info:
        oxedi.parse(b"ST*835*0001~")
    assert isinstance(info.value, ValueError)
    assert str(info.value) == (
        "input does not start with an ISA segment "
        "(found bytes [53 54 2a 38 33 35 2a 30])"
    )


def test_input_with_only_leading_trivia_names_what_it_holds():
    with pytest.raises(oxedi.ParseError) as info:
        oxedi.parse(b"\xef\xbb\xbf\r\n")
    assert str(info.value) == (
        "input does not start with an ISA segment "
        "(input holds only a UTF-8 byte order mark and 2 bytes of whitespace)"
    )


def test_a_delimiter_must_be_one_byte():
    with pytest.raises(ValueError) as info:
        oxedi.Delimiters(element=b"**")
    assert str(info.value) == (
        "delimiter element must be exactly one byte, got 2 bytes: b'**'"
    )


def test_delimiters_signature_names_kinds_and_defaults():
    # The constructor's signature is written three times in the binding
    # (`signature =`, `text_signature` and the stub description); this pins the
    # one Python reports, and stubtest holds the stub to it.
    parameters = inspect.signature(oxedi.Delimiters).parameters
    keyword = inspect.Parameter.POSITIONAL_OR_KEYWORD
    assert [(p.name, p.kind, p.default) for p in parameters.values()] == [
        ("element", keyword, b"*"),
        ("component", keyword, b":"),
        ("segment", keyword, b"~"),
        ("repetition", keyword, None),
        ("release", keyword, None),
    ]


def test_delimiters_takes_every_parameter_by_keyword():
    delimiters = oxedi.Delimiters(
        element=b"|", component=b">", segment=b"\n", repetition=b"^", release=b"?"
    )
    assert (
        delimiters.element,
        delimiters.component,
        delimiters.segment,
        delimiters.repetition,
        delimiters.release,
    ) == (b"|", b">", b"\n", b"^", b"?")
    assert oxedi.Delimiters() == oxedi.Delimiters(
        element=b"*", component=b":", segment=b"~", repetition=None, release=None
    )


@pytest.mark.parametrize("kind", ["bytes", "bytearray", "memoryview", "mmap"])
def test_any_buffer_is_accepted(kind):
    data = read(LARGEST)
    if kind == "bytes":
        result = oxedi.parse(data)
    elif kind == "bytearray":
        result = oxedi.parse(bytearray(data))
    elif kind == "memoryview":
        result = oxedi.parse(memoryview(data))
    else:
        with open(path_of(LARGEST), "rb") as handle:
            with mmap.mmap(handle.fileno(), 0, access=mmap.ACCESS_READ) as mapped:
                result = oxedi.parse(mapped)
    assert result.document.write() == data


def test_text_is_refused_with_a_hint():
    with pytest.raises(TypeError) as info:
        oxedi.parse("ISA*00")
    assert str(info.value) == (
        "the input must be bytes or another buffer, not str: "
        "open the file in binary mode or encode the text"
    )


EXPECTED = "a buffer of unsigned bytes, format 'B', 'b' or 'c'"


@pytest.mark.parametrize("function", ["parse", "stream"])
@pytest.mark.parametrize(
    ("value", "found"),
    [(array("i"), "format 'i'"), (5, "type int")],
)
def test_input_that_is_not_unsigned_bytes_names_function_argument_and_format(
    function, value, found
):
    with pytest.raises(TypeError) as info:
        getattr(oxedi, function)(value)
    assert str(info.value) == (
        f"{function}: argument data must be {EXPECTED}; found {found}"
    )


def test_reprs_of_the_document_types():
    result = parse_named(EMEDNY)
    assert repr(result) == "Result(segments=69, tables=5, diagnostics=0)"
    assert repr(result.document) == "Document(segments=69, bytes=1813)"
    assert repr(result.document[0]) == "Segment(index=0, id=b'ISA')"
    assert repr(result.document.delimiters) == (
        "Delimiters(element=b'*', component=b':', segment=b'~', "
        "repetition=b'^', release=None)"
    )


def test_delimiters_repr_writes_a_newline_as_python_does():
    assert repr(oxedi.Delimiters(element=b"\n", release=b"?")) == (
        "Delimiters(element=b'\\n', component=b':', segment=b'~', "
        "repetition=None, release=b'?')"
    )


def test_parse_file_reads_in_binary_mode(tmp_path):
    target = tmp_path / EMEDNY
    target.write_bytes(read(EMEDNY))
    assert oxedi.parse_file(target).document.write() == read(EMEDNY)


def spans(document):
    return [document[i].span for i in range(len(document))]


def test_a_segment_span_is_its_raw_byte_range_and_the_spans_partition_the_input():
    data = read("edi835_test_eyemed.RMT")
    document = oxedi.parse(data).document
    ranges = spans(document)
    for i, (start, end) in enumerate(ranges):
        assert data[start:end] == document[i].raw
    assert ranges[0][0] == 0
    assert ranges[-1][1] == len(data)
    assert all(left[1] == right[0] for left, right in zip(ranges, ranges[1:]))


def test_the_first_span_starts_at_zero_with_a_byte_order_mark():
    mark = b"\xef\xbb\xbf"
    data = mark + read(EMEDNY)
    document = oxedi.parse(data).document
    start, end = document[0].span
    assert start == 0
    assert data[start:end] == document[0].raw
    assert data[start:end].startswith(mark + b"ISA")


def test_the_last_span_ends_at_the_input_end_without_a_terminator():
    data = read(EMEDNY).rstrip()
    assert data.endswith(b"~")
    data = data[:-1]
    document = oxedi.parse(data).document
    start, end = document[-1].span
    assert end == len(data)
    assert data[start:end] == document[-1].raw
    assert document[-1].id == b"IEA"

