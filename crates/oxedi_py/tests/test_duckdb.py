from decimal import Decimal

import pytest

duckdb = pytest.importorskip("duckdb")

from conftest import LARGEST, parse_named  # noqa: E402


def test_duckdb_queries_the_tables_through_the_capsule():
    result = parse_named(LARGEST)
    claims, services = result.tables["claims"], result.tables["services"]  # noqa: F841
    assert duckdb.sql(
        "select count(*), count(distinct patient_id), sum(payment_amount) from claims"
    ).fetchall() == [(1332, 1212, Decimal("173305.00"))]
    assert duckdb.sql(
        "select count(*) from services s join claims c on s.claim = c.row"
    ).fetchall() == [(6192,)]
