"""``oxedi.write``: tables back into a file, read back to the same tables."""

import collections
import datetime
import importlib.util
import inspect
import re

import pyarrow
import pytest

import oxedi
from conftest import parse_named, read

# The files that read without a diagnostic, and so write strictly: 5010 and
# 4010, synthetic and anonymized real.
CLEAN = [
    "edi835_test_davisvision.RMT",
    "edi835_test_eyemed.RMT",
    "edi835_test_united.rmt",
    "edi835_test_versant.RMT",
    "emedny_sample.txt",
    "united_healthcare_legacy_sample.txt",
    "balanced_5010_sample.txt",
    "balanced_4010_sample.txt",
    "edge_cases_5010_sample.txt",
    "edge_cases_4010_sample.txt",
]

# The files with findings: unbalanced synthetic fixtures and the two excerpts
# whose payment no longer adds up to the claims they keep.
REFUSED = [
    "multi_claim_sample.txt",
    "trizetto_sample.rmt",
    "edi835_test_file.RMT",
    "edi835_test_not_available_claim_id.RMT",
    "masked_findings_5010_sample.txt",
    "masked_findings_4010_sample.txt",
]

# The excerpts: only their payment's balance fails, so every other cell
# writes back as it was read.
EXCERPTS = {"edi835_test_file.RMT", "edi835_test_not_available_claim_id.RMT"}

# The clean files whose originals pyx12 already reports: a payee state code
# outside its list, a rendering provider without its identifier, the
# implementation convention reference pyx12's 5010 map marks Not Used, and a
# second PLB segment, which pyx12's 5010 map counts as a second occurrence of
# the one-time loop that wraps PLB although PLB itself may repeat. A finding
# that names no element goes by its segment.
PYX12_FINDINGS = {
    "united_healthcare_legacy_sample.txt": ["N402"],
    "edi835_test_eyemed.RMT": ["NM108", "NM109"],
    "balanced_5010_sample.txt": ["ST03"],
    "edge_cases_5010_sample.txt": ["PLB"],
}

# What the writer leaves out, so pyx12 cannot find it in the written file:
# the transaction set header's implementation convention reference.
UNWRITTEN = {"ST03"}

requires_pyx12 = pytest.mark.skipif(
    importlib.util.find_spec("pyx12") is None, reason="pyx12 is not installed"
)


def envelope(**changes):
    fields = dict(
        sender_id="SENDER",
        receiver_id="RECEIVER",
        date=datetime.date(2024, 1, 1),
        time=datetime.time(12, 30),
    )
    fields.update(changes)
    return oxedi.Envelope(**fields)


def arrow(tables):
    """Every table as a pyarrow table, without the anchor segment's index
    (the written file holds fewer segments)."""
    out = {}
    for name in tables.keys():
        table = pyarrow.table(tables[name])
        out[name] = table.drop_columns(["segment"])
    return out


def assert_same(before, after):
    first, second = arrow(before), arrow(after)
    assert first.keys() == second.keys()
    for name in first:
        assert first[name].equals(second[name]), name


@pytest.mark.parametrize("name", CLEAN)
def test_the_tables_of_a_parse_write_back_to_the_same_tables(name):
    result = parse_named(name)
    data = oxedi.write(result.tables, envelope())
    again = oxedi.parse(data)
    assert again.diagnostics == []
    assert_same(result.tables, again.tables)


def pyx12_findings(data):
    """pyx12's findings on ``data``, by everything but the segment index (the
    written file holds fewer segments)."""
    from oxedi.pyx12 import validate

    return collections.Counter(
        (d.level, d.kind, d.code, d.rule, d.element, d.component, d.datum)
        for d in validate(data)
    )


@requires_pyx12
@pytest.mark.parametrize("name", CLEAN)
def test_pyx12_finds_in_the_written_file_exactly_what_it_finds_in_the_original(name):
    original = pyx12_findings(read(name))
    written = pyx12_findings(oxedi.write(parse_named(name).tables, envelope()))
    element = re.compile(r"\(([A-Z0-9]{2,3}\d{2})\)")

    def named(finding):
        found = element.findall(finding[3])
        return found[-1] if found else (finding[6] or b"").decode()

    assert sorted(named(f) for f in original.elements()) == PYX12_FINDINGS.get(name, [])
    kept = collections.Counter({f: n for f, n in original.items() if named(f) not in UNWRITTEN})
    assert written == kept


@pytest.mark.parametrize("name", REFUSED)
def test_a_file_with_findings_is_refused_and_written_only_when_allowed(name):
    tables = parse_named(name).tables
    with pytest.raises(oxedi.WriteError) as info:
        oxedi.write(tables, envelope())
    refused = info.value.findings
    assert refused
    assert str(info.value).startswith(
        f"the tables do not make a valid file ({len(refused)} finding"
    )
    for finding in refused:
        assert finding.table is not None and finding.row is not None
        assert f'table "{finding.table}" row {finding.row}' in str(finding)
    data, findings = oxedi.write(tables, envelope(), allow_findings=True)
    assert [str(f) for f in findings] == [str(f) for f in refused]
    again = oxedi.parse(data)
    assert {table: len(again.tables[table]) for table in again.tables.keys()} == {
        table: len(tables[table]) for table in tables.keys()
    }
    if name in EXCERPTS:
        assert_same(tables, again.tables)
        assert [(f.kind, f.diagnostic.kind) for f in findings] == [
            ("ReadBack", "BalanceMismatch")
        ] * len(findings)


def test_an_unbalanced_claim_is_refused_with_the_rule_it_breaks():
    tables = parse_named("multi_claim_sample.txt").tables
    given = envelope(delimiters=oxedi.Delimiters(component=b">", repetition=b"^"))
    with pytest.raises(oxedi.WriteError) as info:
        oxedi.write(tables, given)
    unbalanced = [
        f
        for f in info.value.findings
        if f.diagnostic is not None and f.diagnostic.kind == "BalanceMismatch"
    ]
    assert [(f.table, f.row, f.column) for f in unbalanced] == [
        ("claims", 0, "charge_amount"),
        ("claims", 1, "charge_amount"),
    ]
    assert all('balancing rule "claim_balance"' in str(f) for f in unbalanced)
    _, findings = oxedi.write(tables, given, allow_findings=True)
    assert [str(f) for f in findings] == [str(f) for f in info.value.findings]


@pytest.mark.parametrize("kind", ["pyarrow", "polars", "pandas"])
def test_a_mapping_of_frames_writes_like_the_tables(kind):
    result = parse_named("emedny_sample.txt")
    tables = result.tables
    if kind == "pyarrow":
        frames = {name: pyarrow.table(tables[name]) for name in tables.keys()}
    elif kind == "polars":
        frames = {name: tables[name].to_polars() for name in tables.keys()}
    else:
        frames = {name: tables[name].to_pandas() for name in tables.keys()}
    data = oxedi.write(frames, envelope())
    assert data == oxedi.write(tables, envelope())


def test_the_spec_that_parsed_the_tables_writes_them():
    result = parse_named("edi835_test_davisvision.RMT")
    data = oxedi.write(result.tables, envelope())
    assert b"*00401*" in data
    assert b"~GS*HP*SENDER*RECEIVER*20240101*1230*1*X*004010X091A1~" in data
    frames = {name: result.tables[name].to_polars() for name in result.tables.keys()}
    assert oxedi.write(frames, envelope(), spec=oxedi.Spec.builtin("4010")) == data


def test_the_result_and_its_tables_expose_the_spec_that_parsed_them():
    result = parse_named("edi835_test_davisvision.RMT")
    assert result.spec.to_json() == result.tables.spec.to_json()
    assert result.spec.to_json() == oxedi.Spec.builtin("4010").to_json()
    frames = {name: result.tables[name].to_polars() for name in result.tables.keys()}
    assert oxedi.write(frames, envelope(), spec=result.spec) == oxedi.write(
        result.tables, envelope()
    )


def test_a_given_spec_is_the_one_the_result_exposes():
    given = oxedi.Spec.builtin("4010").patch({"name": "mine"})
    result = oxedi.parse(read("emedny_sample.txt"), spec=given)
    assert result.spec.to_json() == given.to_json()


def test_the_batches_of_a_stream_expose_their_spec():
    data = read("emedny_sample.txt")
    [batch, *_] = oxedi.stream(data)
    assert batch.tables.spec.to_json() == oxedi.Spec.builtin().to_json()


def test_the_batches_of_a_stream_write_each_transaction():
    data = parse_named("emedny_sample.txt").document
    batches = list(oxedi.stream(bytes(b"".join(s.raw for s in data))))
    written = oxedi.write(batches[0].tables, envelope())
    assert written.count(b"~ST*835*") == 1


def test_the_envelope_holds_its_fields():
    given = envelope(control_number=7, usage_indicator="T", line_break=True)
    assert given.sender_id == "SENDER"
    assert given.sender_qualifier == "ZZ"
    assert given.receiver_id == "RECEIVER"
    assert given.receiver_qualifier == "ZZ"
    assert given.usage_indicator == "T"
    assert given.control_number == 7
    assert given.delimiters == oxedi.Delimiters(repetition=b"^")
    assert repr(given) == 'Envelope(sender_id="SENDER", receiver_id="RECEIVER", control_number=7)'
    data = oxedi.write(parse_named("emedny_sample.txt").tables, given)
    first = data.split(b"\n")[0]
    assert first == (
        b"ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       "
        b"*240101*1230*^*00501*000000007*0*T*:~"
    )
    with pytest.raises(ValueError, match="fraction of a second"):
        envelope(time=datetime.time(12, 30, 0, 5))


def test_a_finding_raises_with_every_finding_and_allow_findings_returns_them():
    result = parse_named("emedny_sample.txt")
    payments = result.tables["payments"].to_polars()
    import polars

    off = payments.with_columns(
        polars.lit(1, dtype=polars.Decimal(38, 2)).alias("total_payment_amount")
    )
    frames = {name: result.tables[name].to_polars() for name in result.tables.keys()}
    frames["payments"] = off
    with pytest.raises(oxedi.WriteError) as info:
        oxedi.write(frames, envelope())
    error = info.value
    assert isinstance(error, ValueError)
    assert str(error).startswith(
        "the tables do not make a valid file (1 finding); nothing was written\n"
        '1. table "payments" row 0 column "total_payment_amount": SNIP 3 · balancing rule'
    )
    [finding] = error.findings
    assert finding.kind == "ReadBack"
    assert (finding.table, finding.row, finding.column) == ("payments", 0, "total_payment_amount")
    assert finding.field is None
    assert finding.diagnostic.kind == "BalanceMismatch"
    assert finding.message == str(finding)
    assert repr(finding).startswith("WriteFinding(")
    data, findings = oxedi.write(frames, envelope(), allow_findings=True)
    assert [str(f) for f in findings] == [str(finding)]
    assert b"~BPR*I*1*C*" in data


def test_an_envelope_finding_names_its_field():
    tables = parse_named("emedny_sample.txt").tables
    data, findings = oxedi.write(tables, envelope(sender_id="SEND~ER"), allow_findings=True)
    assert [f.field for f in findings] == ["sender_id", "application_sender"]
    assert findings[0].kind == "DelimiterInValue"
    assert findings[0].table is None
    assert data.startswith(b"ISA*")


def test_tables_that_do_not_fit_the_spec_are_refused():
    tables = parse_named("emedny_sample.txt").tables
    with pytest.raises(oxedi.WriteError) as info:
        oxedi.write({"notes": pyarrow.table({"a": [1]})}, envelope())
    assert str(info.value) == (
        'table "notes" is not a table of the spec, whose tables are '
        '"adjustments", "claims", "payments", "provider_adjustments", "services"'
    )
    assert info.value.findings == []
    with pytest.raises(oxedi.WriteError, match='has no column "color" in the spec'):
        oxedi.write({"claims": pyarrow.table({"color": ["red"]})}, envelope())
    with pytest.raises(oxedi.WriteError, match=r"column \"charge_amount\" row 0: an Arrow Float64"):
        oxedi.write(
            {"claims": pyarrow.table({"charge_amount": pyarrow.array([1.234])})},
            envelope(),
        )
    with pytest.raises(oxedi.WriteError, match="tables must be the tables of a parse or a mapping"):
        oxedi.write([tables], envelope())
    with pytest.raises(oxedi.WriteError, match="both"):
        oxedi.write(
            tables, envelope(delimiters=oxedi.Delimiters(component=b"*", repetition=b"^"))
        )
    with pytest.raises(oxedi.WriteError, match='"delimiters.repetition" is not set'):
        oxedi.write(tables, envelope(delimiters=oxedi.Delimiters()))


def test_the_envelope_returns_its_date_time_and_options():
    given = envelope(application_sender="APP", line_break=True)
    assert given.date == datetime.date(2024, 1, 1)
    assert given.time == datetime.time(12, 30)
    assert (given.application_sender, given.application_receiver) == ("APP", None)
    assert given.line_break is True
    with pytest.raises(ValueError, match="is a datetime; give a datetime.date"):
        envelope(date=datetime.datetime(2024, 1, 1, 12, 30))
    with pytest.raises(ValueError, match="has a time zone"):
        envelope(time=datetime.time(12, 30, tzinfo=datetime.timezone.utc))


def test_the_interchange_header_drops_the_century_and_the_seconds():
    tables = parse_named("emedny_sample.txt").tables
    data = oxedi.write(
        tables, envelope(date=datetime.date(2050, 6, 15), time=datetime.time(8, 5, 9))
    )
    assert b"*500615*0805*^*" in data
    assert b"~GS*HP*SENDER*RECEIVER*20500615*080509*1*X*" in data


def test_frames_with_categories_and_timestamps_write_like_the_tables():
    import polars

    result = parse_named("emedny_sample.txt")
    tables = result.tables
    frames = {name: tables[name].to_polars() for name in tables.keys()}
    claims = frames["claims"]
    frames["claims"] = claims.with_columns(
        polars.col("claim_status").cast(polars.String).cast(polars.Categorical),
        polars.col("statement_from").cast(polars.Datetime("ns")),
    )
    assert oxedi.write(frames, envelope()) == oxedi.write(tables, envelope())
    pandas_claims = tables["claims"].to_pandas()
    pandas_claims["statement_from"] = pandas_claims["statement_from"].astype("datetime64[ns]")
    pandas_frames = {name: tables[name].to_pandas() for name in tables.keys()}
    pandas_frames["claims"] = pandas_claims
    assert oxedi.write(pandas_frames, envelope()) == oxedi.write(tables, envelope())


def test_a_float_for_a_decimal_column_is_refused():
    with pytest.raises(oxedi.WriteError, match=r"float may not hold the amount exactly; give decimal values \(decimal.Decimal"):
        oxedi.write({"claims": pyarrow.table({"charge_amount": [1.5]})}, envelope())
    with pytest.raises(oxedi.WriteError, match="is not at midnight"):
        oxedi.write(
            {"claims": pyarrow.table({"statement_from": [datetime.datetime(2024, 1, 1, 1)]})},
            envelope(),
        )


def test_a_decimal_of_negative_scale_that_overflows_is_reported_as_overflow():
    raw = (1).to_bytes(16, "little", signed=True)
    amount = pyarrow.Array.from_buffers(
        pyarrow.decimal128(38, -40), 1, [None, pyarrow.py_buffer(raw)]
    )
    with pytest.raises(oxedi.WriteError) as info:
        oxedi.write({"claims": pyarrow.table({"charge_amount": amount})}, envelope())
    assert str(info.value) == (
        'table "claims" column "charge_amount" row 0: 1 at scale -40 overflows scale 2'
    )


def test_a_zero_decimal_of_negative_scale_writes_as_zero():
    raw = (0).to_bytes(16, "little", signed=True)
    amount = pyarrow.Array.from_buffers(
        pyarrow.decimal128(38, -40), 1, [None, pyarrow.py_buffer(raw)]
    )
    _, findings = oxedi.write(
        {"claims": pyarrow.table({"charge_amount": amount})}, envelope(), allow_findings=True
    )
    assert not any("charge_amount" in f.message for f in findings)


def test_a_dictionary_key_outside_its_values_is_refused():
    keys = pyarrow.array([0, 5], type=pyarrow.int32())
    status = pyarrow.DictionaryArray.from_arrays(keys, pyarrow.array([b"1", b"2"]), safe=False)
    with pytest.raises(oxedi.WriteError) as info:
        oxedi.write({"claims": pyarrow.table({"claim_status": status})}, envelope())
    assert str(info.value) == (
        'table "claims" column "claim_status" row 1: '
        "the dictionary key 5 is outside the column's 2 dictionary values"
    )


def test_a_reference_against_the_parent_chain_is_a_finding():
    result = parse_named("emedny_sample.txt")
    frames = {name: result.tables[name].to_polars() for name in result.tables.keys()}
    import polars

    services = frames["services"]
    frames["services"] = services.with_columns(
        polars.when(polars.int_range(polars.len()) == 0)
        .then(5)
        .otherwise(polars.col("payment"))
        .alias("payment")
    )
    with pytest.raises(oxedi.WriteError) as info:
        oxedi.write(frames, envelope())
    [finding] = info.value.findings
    assert finding.kind == "MismatchedReference"
    assert (finding.table, finding.row, finding.column) == ("services", 0, "payment")


def test_envelope_defaults_are_the_core_constants():
    envelope_ = oxedi.Envelope(
        sender_id="S", receiver_id="R", date=datetime.date(2024, 1, 1), time=datetime.time(1)
    )
    assert oxedi.Envelope.DEFAULT_QUALIFIER == "ZZ"
    assert oxedi.Envelope.DEFAULT_USAGE_INDICATOR == "P"
    assert oxedi.Envelope.DEFAULT_CONTROL_NUMBER == 1
    assert envelope_.sender_qualifier == oxedi.Envelope.DEFAULT_QUALIFIER
    assert envelope_.receiver_qualifier == oxedi.Envelope.DEFAULT_QUALIFIER
    assert envelope_.usage_indicator == oxedi.Envelope.DEFAULT_USAGE_INDICATOR
    assert envelope_.control_number == oxedi.Envelope.DEFAULT_CONTROL_NUMBER
    assert envelope_.date == datetime.date(2024, 1, 1)
    assert envelope_.time == datetime.time(1)


class _Stamp:
    """A date or time the way a `datetime` exposes it, with values `datetime` refuses."""

    tzinfo = None
    microsecond = 0

    def __init__(self, shown, **parts):
        self.shown = shown
        self.__dict__.update(parts)

    def __str__(self):
        return self.shown


def test_a_time_past_the_end_of_the_day_is_out_of_range():
    stamp = _Stamp("24:00:00", hour=24, minute=0, second=0)
    with pytest.raises(ValueError, match="time 24:00:00 is out of range"):
        oxedi.Envelope(sender_id="S", receiver_id="R", date=datetime.date(2024, 1, 1), time=stamp)


def test_a_date_past_year_9999_is_out_of_range():
    stamp = _Stamp("10000-01-01", year=10000, month=1, day=1)
    with pytest.raises(ValueError, match="date 10000-01-01 is out of range"):
        oxedi.Envelope(sender_id="S", receiver_id="R", date=stamp, time=datetime.time(1))


def test_envelope_runtime_parameters_are_pinned():
    # stubtest allows the defaults' runtime text (`...`); the parameters stay pinned here.
    parameters = inspect.signature(oxedi.Envelope).parameters
    assert list(parameters) == [
        "sender_id",
        "receiver_id",
        "date",
        "time",
        "sender_qualifier",
        "receiver_qualifier",
        "usage_indicator",
        "control_number",
        "application_sender",
        "application_receiver",
        "delimiters",
        "line_break",
    ]
    assert {p.kind for p in parameters.values()} == {inspect.Parameter.KEYWORD_ONLY}
    # The four defaults backed by core constants show as `...`; the rest show their value.
    defaults = {name: parameter.default for name, parameter in parameters.items()}
    for name in ("sender_qualifier", "receiver_qualifier", "usage_indicator", "control_number"):
        assert defaults[name] is Ellipsis, name
    assert defaults["application_sender"] is None
    assert defaults["application_receiver"] is None
    assert defaults["delimiters"] is None
    assert defaults["line_break"] is False
