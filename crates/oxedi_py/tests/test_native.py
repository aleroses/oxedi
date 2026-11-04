import json
import sys
import warnings
from decimal import Decimal

import pytest

import oxedi
from conftest import LARGEST, SAMPLES, parse_named, path_of, read
from oxedi import Spec


def old(name):
    """edi-835-parser's reading of a sample (the test is skipped when the
    library is not installed), with the N104 shim applied."""
    edi_835_parser = pytest.importorskip("edi_835_parser")
    import n104_shim

    n104_shim.apply()
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        return edi_835_parser.parse(str(path_of(name)))


@pytest.mark.parametrize("sample", SAMPLES)
def test_native_counts_equal_edi_835_parser(sample):
    result = parse_named(sample)
    expected = old(sample)
    assert result.count_claims() == expected.count_claims()
    assert result.count_patients() == expected.count_patients()
    assert isinstance(result.sum_payments(), Decimal)
    assert abs(float(result.sum_payments()) - expected.sum_payments()) < 0.005


def test_patient_ids_are_counted_as_text_so_leading_zeros_make_a_different_patient():
    isa = ("ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       "
           "*240101*1200*^*00501*000000001*0*P*:")
    claims = []
    for number, patient in enumerate(["0123", "123", "0123"], start=1):
        claims += [f"CLP*C{number}*1*100*80**MC*ICN{number}", f"NM1*QC*1*DOE*JANE****MI*{patient}"]
    segments = [isa, "GS*HP*S*R*20240101*1200*1*X*005010X221A1", "ST*835*0001",
                "BPR*I*100*C*CHK************20240102", "TRN*1*12345*1512345678",
                "N1*PR*PAYER", "N1*PE*CLINIC*XX*1234567890", "LX*1", *claims]
    segments += [f"SE*{len(segments) - 1}*0001", "GE*1*1", "IEA*1*000000001"]
    result = oxedi.parse("~".join(segments).encode() + b"~")
    rows = [row for row in result.tables["claims"].render().splitlines()[2:] if row]
    assert [row.count("| 0123 |") + row.count("| 123 |") for row in rows] == [1, 1, 1]
    assert result.count_claims() == 3
    assert result.count_patients() == 2


def test_the_largest_sum_keeps_the_column_scale():
    assert str(parse_named(LARGEST).sum_payments()) == "173305.00"


def text(value):
    return None if value is None else str(value)


@pytest.mark.parametrize("sample", SAMPLES)
def test_native_payer_and_payee_equal_edi_835_parser(sample):
    result = parse_named(sample)
    (transaction,) = list(old(sample))
    for role in ("payer", "payee"):
        organization = getattr(transaction, role)
        location = organization.location
        assert getattr(result, role) == {
            "name": organization.organization.name,
            "identification_code": text(organization.organization.identification_code),
            "address": organization.address.address if organization.address else None,
            "city": location.city if location else None,
            "state": location.state if location else None,
            "zip_code": location.zip_code if location else None,
        }


def test_a_file_without_a_payee_loop_has_none():
    data = read("edi835_test_file.RMT")
    result = oxedi.parse(data.replace(b"N1*PE*", b"N1*XX*"))
    assert result.payee is None
    assert result.payer is not None


def test_a_patch_that_removes_a_summarized_table_fails_to_load():
    with pytest.raises(oxedi.SpecError) as info:
        Spec.builtin().patch({"tables": {"payments": None}})
    assert str(info.value) == (
        'applying patch: summary "payee": row.table names the table "payments", which the spec '
        "does not define; its tables are: adjustments, claims, provider_adjustments, services"
    )


def test_an_undeclared_summary_is_named_with_the_declared_ones():
    spec = Spec.builtin().patch({
        "tables": {"payments": None},
        "summaries": {"sum_payments": None, "payer": None, "payee": None},
    })
    result = oxedi.parse(read("edi835_test_file.RMT"), spec=spec)
    with pytest.raises(KeyError) as info:
        result.sum_payments()
    assert info.value.args[0] == (
        'Result.sum_payments: the spec declares no summary "sum_payments"; '
        "it declares: count_claims, count_patients"
    )


def test_a_summary_of_another_kind_is_named():
    spec = Spec.builtin().patch({"summaries": {"count_claims": {
        "count": None, "sum": {"table": "payments", "column": "total_payment_amount"}}}})
    result = oxedi.parse(read("edi835_test_file.RMT"), spec=spec)
    with pytest.raises(TypeError) as info:
        result.count_claims()
    assert str(info.value) == (
        'Result.count_claims reads the summary "count_claims" as a count; '
        "the spec declares a decimal"
    )


def test_a_patch_that_removes_a_summarized_column_fails_to_load():
    declared = json.loads(Spec.builtin().to_json())["tables"]["claims"]["columns"]
    columns = sorted(name for name in declared if name != "patient_id")
    with pytest.raises(oxedi.SpecError) as info:
        Spec.builtin().patch(
            {"tables": {"claims": {"columns": {"patient_id": None}, "phi_seed": None}}}
        )
    assert str(info.value) == (
        'applying patch: summary "count_patients": distinct.column names the column "patient_id", '
        'which the table "claims" does not declare; its columns are: ' + ", ".join(columns)
    )


def test_a_patch_that_makes_a_summed_column_text_fails_to_load():
    # The balancing rule that reads BPR02 as an amount goes with the change.
    with pytest.raises(oxedi.SpecError) as info:
        Spec.builtin().patch({"segments": {"BPR": {"elements": {"2": {
            "name": "total_actual_provider_payment_amount", "type": "AN", "min": 1, "max": 18}}}},
            "balancing": {"transaction_balance": None}})
    assert str(info.value) == (
        'applying patch: summary "sum_payments": sum.column names the column '
        '"total_payment_amount" of the table "payments", which reads AN (string); sum needs a '
        "decimal (R) column"
    )


def test_payer_reads_one_transaction():
    data = read("edi835_test_file.RMT")
    with pytest.raises(ValueError) as info:
        oxedi.parse(data + data).payer
    assert str(info.value) == (
        'Result.payer: summary "payer" reads the one row of the table "payments", which has 2 rows'
    )


def test_payee_reads_one_transaction():
    data = read("edi835_test_file.RMT")
    with pytest.raises(ValueError) as info:
        oxedi.parse(data + data).payee
    assert str(info.value) == (
        'Result.payee: summary "payee" reads the one row of the table "payments", which has 2 rows'
    )


def test_a_table_reaches_polars_and_pandas():
    pl = pytest.importorskip("polars")
    pytest.importorskip("pandas")
    tables = parse_named(LARGEST).tables
    frame = tables["claims"].to_polars()
    assert frame.height == 1332
    assert frame.schema["charge_amount"] == pl.Decimal(38, 2)
    assert tables["claims"].to_pandas().shape == (1332, 28)
    frames = tables.to_polars()
    assert list(frames) == tables.keys()
    assert frames["services"].height == 6192
    assert list(tables.to_pandas()) == tables.keys()


@pytest.mark.parametrize("method, module, extra", [
    ("to_polars", "polars", "polars"),
    ("to_pandas", "pandas", "pandas"),
    ("to_pandas", "pyarrow", "pandas"),
])
@pytest.mark.parametrize("owner", ["Table", "Tables"])
def test_a_missing_extra_is_named(monkeypatch, owner, method, module, extra):
    tables = parse_named("edi835_test_file.RMT").tables
    target = tables["claims"] if owner == "Table" else tables
    monkeypatch.setitem(sys.modules, module, None)
    with pytest.raises(ImportError) as info:
        getattr(target, method)()
    assert str(info.value) == (
        f'{owner}.{method} needs {module}, which is not installed; '
        f'install it with: pip install "oxedi[{extra}]"'
    )
    assert isinstance(info.value.__cause__, ImportError)


def test_a_sum_that_overflows_names_the_table_row_and_amount():
    data = read("edi835_test_file.RMT").decode()
    nines = "9" * 36
    data = data.replace("BPR*I*8982*", f"BPR*I*{nines}*").encode()
    spec = Spec.builtin().patch({"segments": {"BPR": {"elements": {"2": {
        "name": "total_actual_provider_payment_amount", "type": "R", "min": 1, "max": 40}}}}})
    result = oxedi.parse(data + data, spec=spec)
    with pytest.raises(ValueError) as info:
        result.sum_payments()
    assert str(info.value) == (
        'Result.sum_payments: summary "sum_payments": the sum of "total_payment_amount" in the '
        'table "payments" '
        f"overflows a 128-bit decimal at row 1 (amount {nines}.00)"
    )
