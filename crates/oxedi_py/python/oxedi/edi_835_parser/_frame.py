"""edi-835-parser's frames, built column by column from a transaction's tables.

Each column is computed for every row at once: Arrow casts decode text and
read amounts and dates, joins between loop levels are position lookups on
the parent-row columns, the long tables (adjustments, references, remarks)
are pivoted into numbered columns, and codes are mapped once per distinct
value. Only cells whose value depends on how the segment was written (an
element written empty versus one the segment stops before, a date the spec
could not read) are looked up in the document. The columns hold the Python
values the library puts in its records (``None`` where it writes ``None``,
NaN where its record lacks the key), and pandas infers each column's dtype
from them as it does for the library's list of records.
"""

from __future__ import annotations

import codecs
import functools

import numpy as np

from . import _codes
from ._convert import ENCODING, element_date
from ._tables import ints, position

UTF8 = codecs.lookup(ENCODING).name == "utf-8"
LIBRARY_COLUMNS = (
    "marker", "patient", "code", "modifier", "qualifier", "allowed_units", "billed_units",
    "transaction_date", "icn", "charge_amount", "allowed_amount", "paid_amount", "payer",
    "start_date", "end_date", "rendering_provider", "payer_classification", "was_forwarded",
)
# The library columns a claim without services has no value for.
SERVICE_ONLY = ("code", "modifier", "qualifier", "allowed_units", "billed_units",
                "charge_amount", "allowed_amount", "paid_amount")
# (prefix, table, ((suffix, table column, reader), ...)) of the numbered columns.
NUMBERED = (
    ("adj", "rows_adjustments", (("group", "group", "text"), ("code", "code", "text"),
                                 ("amount", "amount", "money"))),
    ("ref", "rows_references", (("qual", "qual", "text"), ("value", "value", "text"))),
    ("rem", "rows_remarks", (("qual", "qual", "text"), ("code", "code", "text"))),
)
GROUP = (("group", "group", "text"), ("code", "code", "text"),
         ("amount", "amount", "money"), ("quantity", "quantity", "money"))
AMOUNT = (("qual", "qualifier", "text"), ("amount", "amount", "money"))
CLAIM_EXTRAS = (
    ("x_claim_adj", "rows_claim_adjustments", GROUP),
    ("x_claim_ref", "rows_claim_references", (("qual", "qualifier", "text"), ("value", "value", "text"))),
    ("x_claim_amt", "rows_claim_amounts", AMOUNT),
)
SERVICE_EXTRAS = (
    ("x_svc_adj", "rows_adjustment_groups", GROUP),
    ("x_svc_amt", "rows_service_amounts", AMOUNT),
)
SERVICE_DATE, _ = position("rows_service_dates", "date")
CLAIM_DATE, _ = position("rows_claim_dates", "date")


def column_of(n, value):
    """An object column of ``n`` copies of ``value``."""
    out = np.empty(n, dtype=object)
    out[:] = [value] * n
    return out


def objects(values):
    """A list of Python values as an object column."""
    out = np.empty(len(values), dtype=object)
    out[:] = values
    return out


def text(column):
    """A binary column as text decoded as the library's ``open()`` decodes
    (``None`` for null and for empty; ``empty`` tells an element written
    empty from one the segment stops before)."""
    import pyarrow as pa

    if UTF8:
        try:
            values = column.cast(pa.string()).to_numpy(zero_copy_only=False)
            values[np.equal(values, "")] = None
            return values
        except pa.ArrowInvalid:
            pass
    return objects([v.decode(ENCODING) if v else None for v in column.to_pylist()])


def empty(column):
    """For each cell of a binary column, whether it holds an empty value: an
    element the segment has but writes empty, as opposed to a null cell, an
    element the segment stops before."""
    import pyarrow.compute as pc

    blank = pc.equal(pc.binary_length(column), 0)
    return pc.fill_null(blank, False).to_numpy(zero_copy_only=False)


def money(column):
    """A float column as Python floats (``None`` for null)."""
    values = column.to_numpy(zero_copy_only=False).astype(object)
    values[column.is_null().to_numpy(zero_copy_only=False)] = None
    return values


def moments(column):
    """A date column as midnight ``datetime`` values (``None`` for null)."""
    import pyarrow as pa

    return column.cast(pa.timestamp("us")).to_numpy(zero_copy_only=False).astype(object)


def read(table, column, reader):
    return text(table.column(column)) if reader == "text" else money(table.column(column))


def positions(keys, values):
    """For each key (-1 for null), the position of the equal value in
    ``values`` (unique), or -1."""
    if not len(values):
        return np.full(len(keys), -1)
    order = np.argsort(values, kind="stable")
    ordered = values[order]
    at = np.minimum(np.searchsorted(ordered, keys), len(ordered) - 1)
    return np.where((ordered[at] == keys) & (keys >= 0), order[at], -1)


def take(values, at, default=None):
    """``values`` at each position, ``default`` where the position is -1."""
    out = column_of(len(at), default)
    found = at >= 0
    out[found] = values[at[found]]
    return out


def ranks(groups):
    """Each row's position among the rows of its group, in table order."""
    order = np.argsort(groups, kind="stable")
    ordered = groups[order]
    index = np.arange(len(ordered))
    first = np.r_[True, ordered[1:] != ordered[:-1]] if len(ordered) else index.astype(bool)
    starts = np.maximum.accumulate(np.where(first, index, 0)) if len(ordered) else index
    out = np.empty(len(groups), dtype=np.int64)
    out[order] = index - starts
    return out


def lookup(values, function):
    """``function`` of each value, called once per distinct value."""
    import pandas as pd

    codes, uniques = pd.factorize(values, use_na_sentinel=True)
    mapped = np.empty(len(uniques) + 1, dtype=object)
    for at, value in enumerate(uniques):
        mapped[at] = function(value)
    mapped[-1] = function(None)
    return mapped[codes]


def as_integer(value):
    """The library's ``int()`` with its fallback to the text."""
    if value is None:
        return None
    try:
        return int(value)
    except ValueError:
        return value


def numbered(rows, parent, keys, prefix, fields):
    """The rows of ``rows`` under each parent key as columns numbered from 0
    (``prefix_n_suffix``, NaN where a parent has fewer rows), with how many
    rows each parent has."""
    table = rows.table
    at = positions(ints(table.column(parent)), keys)
    counts = np.bincount(at[at >= 0], minlength=len(keys)) if len(keys) else np.zeros(0, np.int64)
    out = {}
    if not (at >= 0).any():
        return out, counts
    rank = ranks(at)
    values = [(suffix, read(table, column, reader)) for suffix, column, reader in fields]
    for n in range(int(rank[at >= 0].max()) + 1):
        chosen = (rank == n) & (at >= 0)
        for suffix, column in values:
            cells = column_of(len(keys), np.nan)
            cells[at[chosen]] = column[chosen]
            out[f"{prefix}_{n}_{suffix}"] = cells
    return out, counts


def appearance(counts, fields):
    """The numbered columns in the order a list of records first shows them:
    each row adds its adjustments, then references, then remarks."""
    if not counts:
        return []
    stacked = np.stack(counts)
    padded = np.concatenate([np.zeros((len(counts), 1), np.int64), stacked], axis=1)
    before = np.maximum.accumulate(padded, axis=1)[:, :-1]
    order = []
    for row in np.flatnonzero((stacked > before).any(axis=0)):
        for kind, (prefix, suffixes) in enumerate(fields):
            for n in range(before[kind, row], stacked[kind, row]):
                order.extend(f"{prefix}_{n}_{suffix}" for suffix in suffixes)
    return order


class Transaction:
    """The frame columns of one transaction set."""

    def __init__(self, transaction_set):
        self.set = transaction_set
        self.tables = transaction_set._t
        self.document = transaction_set._d
        self._dates = {}

    def table(self, name):
        return self.tables[name].table

    @functools.cached_property
    def claims(self):
        return self.table("rows_claims")

    @functools.cached_property
    def claim_rows(self):
        return ints(self.claims.column("row"))

    @functools.cached_property
    def services(self):
        return self.table("rows")

    @functools.cached_property
    def service_rows(self):
        return ints(self.services.column("row"))

    @functools.cached_property
    def service_claims(self):
        """Each service's claim position."""
        return positions(ints(self.services.column("claim")), self.claim_rows)

    @functools.cached_property
    def order(self):
        """The services in the library's order: claim by claim, each claim's
        services in file order."""
        claims = self.service_claims
        order = np.argsort(claims, kind="stable")
        return order[claims[order] >= 0]

    def dates(self, name, element):
        """A date table's values: a midnight ``datetime``, or for a date the
        spec could not read, the library's reading of the element text,
        which raises for text of a date's length that is not a date."""
        if name not in self._dates:
            self._dates[name] = self._read_dates(name, element)
        return self._dates[name]

    def _read_dates(self, name, element):
        table = self.table(name)
        column = table.column("date")
        values = moments(column)
        segments = table.column("segment").to_pylist()
        for row in np.flatnonzero(column.is_null().to_numpy(zero_copy_only=False)).tolist():
            if segments[row] is None:
                continue
            values[row] = element_date(
                self.document[segments[row]], segments[row], element, self.set.file_path)
        return values

    def date_at(self, name, element, segments):
        """For each segment index, whether the date table has a row read
        from it, and that row's value."""
        table = self.table(name)
        at = positions(segments, ints(table.column("segment")))
        return at >= 0, take(self.dates(name, element), at)

    @functools.cached_property
    def entity_names(self):
        """Each claim entity's ``name``, as the library's property spells it."""
        table = self.table("rows_claim_entities")
        firsts = text(table.column("first_name"))
        firsts[empty(table.column("first_name"))] = ""
        lasts = text(table.column("last_name")).tolist()
        names = [f"{first} {last or ''}".title() for first, last in zip(firsts.tolist(), lasts)]
        return objects(names), ints(table.column("segment"))

    def entity(self, column):
        """Per claim, the name of the entity whose segment ``column`` names."""
        names, segments = self.entity_names
        return take(names, positions(ints(self.claims.column(column)), segments))

    @functools.cached_property
    def claim_columns(self):
        """The library columns' values per claim."""
        statuses = text(self.claims.column("status"))
        status = lookup(statuses, lambda code: _codes.STATUSES.get(code, _codes.UNKNOWN_STATUS))
        start, end = (
            self.date_at("rows_claim_dates", CLAIM_DATE, ints(self.claims.column(f"statement_period_{edge}_segment")))
            for edge in ("start", "end"))
        return {
            "marker": text(self.claims.column("marker")),
            "patient": self.entity("patient_segment"),
            "icn": text(self.claims.column("icn")),
            "start_date": start,
            "end_date": end,
            "rendering_provider": self.entity("rendering_provider_segment"),
            "payer_classification": objects([s[1] for s in status]),
            "was_forwarded": objects([s[2] for s in status]),
        }

    @functools.cached_property
    def allowed_amount(self):
        """The service's last AMT amount when its qualifier is the allowed
        amount: the library keeps the last AMT it reads."""
        table = self.table("rows_service_amounts")
        at = positions(ints(table.column("service")), self.service_rows)
        last = np.full(len(self.service_rows), -1)
        np.maximum.at(last, at[at >= 0], np.flatnonzero(at >= 0))
        qualifier = lookup(text(table.column("qualifier")),
                           lambda q: _codes.AMOUNT_QUALIFIERS.get(q, q) == _codes.ALLOWED_ACTUAL)
        amounts = money(table.column("amount"))
        allowed = np.where(np.equal(qualifier, True), amounts, None)
        return take(allowed, last)

    def service_dates(self, *columns):
        """Per service, the first of the service dates the columns name that
        has a row (the period date, then the service date)."""
        found = np.zeros(len(self.service_rows), dtype=bool)
        values = column_of(len(self.service_rows), None)
        for column in columns:
            here, value = self.date_at("rows_service_dates", SERVICE_DATE, ints(self.services.column(column)))
            chosen = here & ~found
            values[chosen] = value[chosen]
            found |= here
        return found, values

    def units(self, column, absent):
        """The service units of ``column``: a unit count, ``None`` for units
        the SVC segment holds empty and ``absent`` for units it stops before."""
        cells = self.services.column(column)
        units = lookup(text(cells), as_integer)
        return np.where(np.equal(units, None), np.where(empty(cells), None, absent), units).astype(object)

    @functools.cached_property
    def service_columns(self):
        """The library columns' service values, in table order."""
        services = self.services
        paid = money(services.column("paid_amount"))
        modifier = text(services.column("modifier"))
        modifier[empty(services.column("modifier"))] = ""
        absent = np.where(np.equal(paid, 0) & ~np.equal(paid, None), 0, 1).astype(object)
        allowed = self.units("allowed_units", absent)
        billed = self.units("billed_units", allowed)
        return {
            "code": text(services.column("code")),
            "modifier": modifier,
            "qualifier": text(services.column("qualifier")),
            "allowed_units": allowed,
            "billed_units": billed,
            "charge_amount": money(services.column("charge_amount")),
            "allowed_amount": self.allowed_amount,
            "paid_amount": paid,
            "start": self.service_dates("service_period_start_segment", "service_date_segment"),
            "end": self.service_dates("service_period_end_segment", "service_date_segment"),
        }

    def library(self, transaction_date, payer):
        """The library columns of the service rows, in its order."""
        order = self.order
        claim = self.service_claims[order]
        claims = {key: value[claim] if not isinstance(value, tuple) else (value[0][claim], value[1][claim])
                  for key, value in self.claim_columns.items()}
        services = {key: value[order] if not isinstance(value, tuple) else (value[0][order], value[1][order])
                    for key, value in self.service_columns.items()}
        columns = {}
        for name in LIBRARY_COLUMNS:
            if name == "transaction_date":
                columns[name] = column_of(len(order), transaction_date)
            elif name == "payer":
                columns[name] = column_of(len(order), payer)
            elif name in ("start_date", "end_date"):
                found, value = services["start" if name == "start_date" else "end"]
                claim_found, claim_value = claims[name]
                columns[name] = np.where(found, value, np.where(claim_found, claim_value, None)).astype(object)
            elif name in services:
                columns[name] = services[name]
            else:
                columns[name] = claims[name]
        return columns

    def numbered(self):
        """The adjustment, reference and remark columns per service in the
        library's order of services, with each kind's count per service."""
        columns, counts = {}, []
        for prefix, table, fields in NUMBERED:
            found, count = numbered(self.tables[table], "service", self.service_rows, prefix, fields)
            columns.update({key: value[self.order] for key, value in found.items()})
            counts.append(count[self.order])
        return columns, counts

    def strict(self):
        """The library's frame of this transaction set. A claim's dates are
        read even when no service gives it a row, so a date the library's
        parser rejects raises with or without services."""
        import pandas as pd

        if not len(self.tables["rows_claims"]) or not len(self.order):
            if len(self.tables["rows_claims"]):
                self.dates("rows_claim_dates", CLAIM_DATE)
            return pd.DataFrame([])
        financial_information, payer = self.set.financial_information, self.set.payer
        columns = self.library(financial_information.transaction_date, payer.organization.name)
        extra, counts = self.numbered()
        fields = [(prefix, [suffix for suffix, _, _ in f]) for prefix, _, f in NUMBERED]
        for key in appearance(counts, fields):
            columns[key] = extra[key]
        return frame(columns)

    def extended(self):
        """The extended frame's rows of this transaction set, as columns:
        each claim's services (or the claim alone when it has none), then the
        provider adjustments. Without a payer loop, a transaction with
        services raises as the library's frame does; its claims without
        services and its provider adjustments, rows that frame leaves out,
        take ``None`` as the payer."""
        plb = self.tables["rows_provider_adjustments"]
        claims = len(self.tables["rows_claims"])
        if not claims and not len(plb):
            return 0, {}
        financial_information = self.set.financial_information
        has_services = bool(claims) and bool(len(self.order))
        if has_services:
            payer = self.set.payer.organization.name
        else:
            try:
                payer = self.set.payer.organization.name
            except ValueError:
                payer = None
        transaction_date = financial_information.transaction_date if financial_information else None
        blocks = []
        if claims:
            claim_extra = {"x_claim": objects(self.claim_rows.tolist())}
            for prefix, table, fields in CLAIM_EXTRAS:
                claim_extra.update(numbered(self.tables[table], "claim", self.claim_rows, prefix, fields)[0])
            if has_services:
                columns = self.library(financial_information.transaction_date, payer)
                columns.update(self.numbered()[0])
                claim = self.service_claims[self.order]
                columns.update({key: value[claim] for key, value in claim_extra.items()})
                for prefix, table, fields in SERVICE_EXTRAS:
                    found = numbered(self.tables[table], "service", self.service_rows, prefix, fields)[0]
                    columns.update({key: value[self.order] for key, value in found.items()})
                columns["x_row_kind"] = column_of(len(claim), "service")
                blocks.append((claim, columns))
            alone = np.flatnonzero(np.bincount(self.service_claims[self.order], minlength=claims) == 0)
            if len(alone):
                values = self.claim_columns
                columns = {}
                for name in LIBRARY_COLUMNS:
                    if name in SERVICE_ONLY:
                        columns[name] = column_of(len(alone), None)
                    elif name == "transaction_date":
                        columns[name] = column_of(len(alone), transaction_date)
                    elif name == "payer":
                        columns[name] = column_of(len(alone), payer)
                    elif name in ("start_date", "end_date"):
                        found, value = values[name]
                        columns[name] = np.where(found[alone], value[alone], None).astype(object)
                    else:
                        columns[name] = values[name][alone]
                columns.update({key: value[alone] for key, value in claim_extra.items()})
                columns["x_row_kind"] = column_of(len(alone), "claim")
                blocks.append((alone, columns))
        rows, columns = stack(blocks)
        if blocks:
            order = np.argsort(np.concatenate([claim for claim, _ in blocks]), kind="stable")
            columns = {key: value[order] for key, value in columns.items()}
        if len(plb):
            table = plb.table
            plb_columns = {
                "transaction_date": column_of(len(plb), transaction_date),
                "payer": column_of(len(plb), payer),
                "x_row_kind": column_of(len(plb), "provider_adjustment"),
                "x_plb_provider_id": text(table.column("provider_id")),
                "x_plb_fiscal_period_date": moments(table.column("fiscal_period_date")),
                "x_plb_reason_code": text(table.column("reason_code")),
                "x_plb_reference_id": text(table.column("reference_id")),
                "x_plb_amount": money(table.column("amount")),
            }
            rows, columns = stack([(range(rows), columns), (range(len(plb)), plb_columns)])
        return rows, columns


def stack(blocks):
    """Blocks of rows (each a length-giving sequence and its columns) as one,
    NaN where a block lacks a column."""
    rows = sum(len(size) for size, _ in blocks)
    names = list(dict.fromkeys(name for _, columns in blocks for name in columns))
    return rows, {
        name: np.concatenate([columns.get(name, column_of(len(size), np.nan)) for size, columns in blocks])
        for name in names
    }


def infers_strings():
    """Whether pandas infers ``str`` values as its string dtype rather than
    ``object``."""
    import pandas as pd

    try:
        return bool(pd.get_option("future.infer_string"))
    except (KeyError, ValueError):
        return False


def frame(columns):
    """A DataFrame of object columns, each with the dtype pandas infers for
    the same values in a list: columns whose dtype the values settle (text,
    floats, and booleans and integers without nulls) are built typed, the
    rest go through pandas' inference."""
    import pandas as pd
    from pandas.api.types import infer_dtype

    keep_text = not infers_strings()
    data = {}
    for name, values in columns.items():
        nulls = pd.isna(values)
        present = values[~nulls]
        kind = infer_dtype(present, skipna=False) if len(present) else "empty"
        if kind == "string" and keep_text:
            data[name] = pd.Series(values, dtype=object, copy=False)
        elif kind == "floating":
            data[name] = np.where(nulls, np.nan, values).astype(np.float64)
        elif kind == "boolean" and not nulls.any():
            data[name] = values.astype(bool)
        elif kind == "integer" and not nulls.any() and -(2**63) <= values.min() and values.max() < 2**63:
            data[name] = values.astype(np.int64)
        else:
            data[name] = values.tolist()
    return pd.DataFrame(data)


def strict(transaction_set):
    """``TransactionSet.to_dataframe()``: one row per service."""
    return Transaction(transaction_set).strict()


def extended(transaction_sets):
    """The extended frame's rows of every transaction set, as columns."""
    blocks = [Transaction(transaction_set).extended() for transaction_set in transaction_sets]
    return stack([(range(rows), columns) for rows, columns in blocks if rows])
