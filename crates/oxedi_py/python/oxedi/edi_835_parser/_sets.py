"""``parse``, ``TransactionSets`` and ``TransactionSet``."""

from __future__ import annotations

import datetime
import functools
import os
import warnings
from typing import IO, TYPE_CHECKING, Dict, Iterable, Iterator, List, Optional, Union, cast

from .. import ParseError
from . import _codes
from ._convert import date, integer, money, readable, text, written
from ._frame import strict
from ._tables import load, position
from ._views import (
    Address, Claim, FinancialInformation, Interchange, Location, Organization,
    OrganizationSegment, Service, at_segment, mapped,
)

if TYPE_CHECKING:
    import pandas as pd

    from .. import Document
    from ._tables import Rows

# What ``parse_bytes`` reads: bytes or a buffer of them.
BytesLike = Union[bytes, bytearray, memoryview]

_SUFFIXES = (".txt", ".835", ".DAT")
TRANSACTION_DATE, _ = position("rows_payments", "transaction_date")
LOCATION = ("city", "state", "zip_code")


class TransactionSet:
    """One transaction (ST to SE) of one file."""

    def __init__(self, document: Optional[Document], tables: Dict[str, Rows], file_path: str) -> None:
        self._d, self._t, self.file_path = document, tables, file_path

    @functools.cached_property
    def interchange(self) -> Optional[Interchange]:
        """This transaction's ISA (the last one when the file has no transaction)."""
        t = self._t["rows_interchanges"]
        if not len(t):
            return None
        day, clock = t["transmission_date"][-1], t["transmission_time"][-1]
        sent = None if day is None or clock is None else datetime.datetime.combine(day, clock)
        qualifier = text(t["authorization_information_qualifier"][-1])
        return Interchange(
            authorization_information_qualifier=None if qualifier == "00" else qualifier,
            sender=mapped(_codes.ORGANIZATIONS, text(t["sender"][-1]).strip()),
            receiver=mapped(_codes.ORGANIZATIONS, text(t["receiver"][-1]).strip()),
            transmission_date=sent,
        )

    @functools.cached_property
    def financial_information(self) -> Optional[FinancialInformation]:
        """The BPR fields, or ``None`` without a BPR, as in the library."""
        t = self._t["rows_payments"]
        if not len(t) or t["bpr_segment"][0] is None:
            return None
        return FinancialInformation(
            amount_paid=money(t["amount_paid"][0]),
            payment_method=mapped(_codes.PAYMENT_METHODS, text(t["payment_method"][0])),
            routing_number=integer(t["routing_number"][0]),
            transaction_date=date(t["transaction_date"][0], self._d, t["bpr_segment"][0], TRANSACTION_DATE,
                            self.file_path),
        )

    @functools.cached_property
    def organizations(self) -> List[Organization]:
        """The N1 loops, with an address and a location when the loop has an
        N3 or N4 segment; an element the segment holds empty reads ``""``,
        as in the library."""
        t = self._t["rows_organizations"]
        out = []
        for i in range(len(t)):
            location = address = None
            at = t["location_segment"][i]
            if at is not None:
                location = Location(*(written(t[k][i]) for k in LOCATION))
            at = t["address_segment"][i]
            if at is not None:
                address = Address(written(t["address"][i]))
            identification_code = written(t["identification_code"][i], integer)
            out.append(Organization(
                OrganizationSegment(mapped(_codes.ORGANIZATION_TYPES, text(t["type"][i])),
                                    text(t["name"][i]), identification_code),
                location, address, t["segment"][i]))
        return out

    @functools.cached_property
    def claims(self) -> List[Claim]:
        return [Claim(self._t, self._d, i, self.file_path) for i in range(len(self._t["rows_claims"]))]

    def _organization(self, role):
        payments = self._t["rows_payments"]
        if not len(payments):
            raise ValueError(f"{self.file_path}: the file has no transaction (ST), so no {role} loop (N1)")
        found = at_segment(self.organizations, payments[f"{role}_segment"][0])
        if found is None:
            raise ValueError(
                f"{self.file_path}: the transaction at segment "
                f"{self._t['rows_payments']['segment'][0]} has no {role} loop (N1)"
            )
        return found

    @property
    def payer(self) -> Organization:
        return self._organization("payer")

    @property
    def payee(self) -> Organization:
        return self._organization("payee")

    def __repr__(self) -> str:
        return "\n".join(str(item) for item in (
            ("interchange", self.interchange), ("financial_information", self.financial_information),
            ("claims", self.claims), ("organizations", self.organizations), ("file_path", self.file_path)))

    @staticmethod
    def serialize_service(
        financial_information: FinancialInformation, payer: Organization, claim: Claim, service: Service,
    ) -> Dict[str, object]:
        """The library's columns for one service, in its order."""
        start = service.service_period_start or claim.claim_statement_period_start
        end = service.service_period_end or claim.claim_statement_period_end
        rendering = claim.rendering_provider
        status = claim.claim.status
        return {
            "marker": claim.claim.marker,
            "patient": claim.patient.name if claim.patient else None,
            "code": service.service.code,
            "modifier": service.service.modifier,
            "qualifier": service.service.qualifier,
            "allowed_units": service.service.allowed_units,
            "billed_units": service.service.billed_units,
            "transaction_date": financial_information.transaction_date,
            "icn": claim.claim.icn,
            "charge_amount": service.service.charge_amount,
            "allowed_amount": service.allowed_amount,
            "paid_amount": service.service.paid_amount,
            "payer": payer.organization.name,
            "start_date": start.date if start else None,
            "end_date": end.date if end else None,
            "rendering_provider": rendering.name if rendering else None,
            "payer_classification": str(status.payer_classification),
            "was_forwarded": status.was_forwarded,
        }

    def to_dataframe(self, extended: bool = False) -> pd.DataFrame:
        """One row per service, as edi-835-parser builds it; with ``extended``,
        also the rows and ``x_`` columns that frame leaves out (claim-only and
        provider-adjustment rows can widen strict columns' dtypes: ``int`` to
        ``float``, ``bool`` to ``object``, and an ``object`` column holding
        only ``None`` to ``float`` with NaN)."""
        if extended:
            from ._extended import frame

            return frame([self])
        return strict(self)


class TransactionSets:
    """Every transaction of the files parsed."""

    def __init__(self, transaction_sets: Iterable[TransactionSet]) -> None:
        self.transaction_sets = list(transaction_sets)

    def __iter__(self) -> Iterator[TransactionSet]:
        yield from self.transaction_sets

    def __len__(self) -> int:
        return len(self.transaction_sets)

    def __repr__(self) -> str:
        return "\n".join(str(t) for t in self)

    def to_dataframe(self, extended: bool = False) -> pd.DataFrame:
        """Every transaction's rows, with the numbered columns sorted last.
        The frames are concatenated one at a time onto an empty frame, as the
        library does, so empty transactions give the same dtypes. With
        ``extended``, also the rows and ``x_`` columns the frame leaves out
        (claim-only and provider-adjustment rows can widen strict columns'
        dtypes: ``int`` to ``float``, ``bool`` to ``object``, and an
        ``object`` column holding only ``None`` to ``float`` with NaN)."""
        import pandas as pd

        if extended:
            from ._extended import frame

            return frame(self.transaction_sets)
        data = pd.DataFrame()
        for transaction_set in self:
            data = pd.concat([data, transaction_set.to_dataframe()])
        return TransactionSets.sort_columns(data)

    @staticmethod
    def sort_columns(data: pd.DataFrame) -> pd.DataFrame:
        variable = sorted(c for c in data.columns if any(s in c for s in ("adj", "ref", "rem")))
        static = [c for c in data.columns if c not in variable]
        return data[static + variable]

    def sum_payments(self) -> float:
        # As in the library, a transaction without a BPR or an amount fails here.
        return sum((t.financial_information.amount_paid for t in self), 0)  # type: ignore[union-attr, misc]

    def count_claims(self) -> int:
        return sum(len(t.claims) for t in self)

    def count_patients(self) -> int:
        return len({
            c.patient.identification_code if c.patient else None for t in self for c in t.claims
        })


def _sets(data, file_path) -> List[TransactionSet]:
    readable(data)
    document, parts = load(data)
    return [TransactionSet(document, tables, file_path) for tables in parts]


def parse_bytes(data: BytesLike, file_path: Optional[str] = None) -> TransactionSets:
    """Parses one file held in ``bytes``, ``bytearray`` or ``memoryview``;
    ``file_path`` is what each ``TransactionSet.file_path`` reports."""
    return TransactionSets(_sets(data, "<bytes>" if file_path is None else file_path))


def parse_file_obj(file: IO[bytes], file_path: Optional[str] = None) -> TransactionSets:
    """Parses what ``file.read()`` returns (a binary file object)."""
    return parse_bytes(file.read(), file_path)


def parse_many(items: Iterable[Union[BytesLike, IO[bytes]]]) -> TransactionSets:
    """Parses each item (bytes-like or binary file object) in order."""
    sets = []
    for item in items:
        if hasattr(item, "read"):
            sets.extend(parse_file_obj(cast(IO[bytes], item)).transaction_sets)
        else:
            sets.extend(parse_bytes(cast(BytesLike, item)).transaction_sets)
    return TransactionSets(sets)


def _path(path) -> List[TransactionSet]:
    with open(path, "rb") as handle:
        return _sets(handle.read(), str(path))


def parse(path: Union[str, "os.PathLike[str]"], debug: bool = False) -> TransactionSets:
    """Parses a file path, or every ``.txt``, ``.835`` and ``.DAT`` file of a
    directory, with the same signature and behaviour as ``edi_835_parser.parse``.
    Data already in memory goes through ``parse_bytes``, ``parse_file_obj`` or
    ``parse_many``.

    Beyond the library: any path-like (``pathlib.Path``) is accepted, ``""``
    raises ``FileNotFoundError`` where the library raises ``IndexError``, and a
    file whose ISA cannot be read (with no byte order mark before it) raises
    ``ParseError`` naming the file where the library raises ``IndexError``."""
    path = os.path.expanduser(os.fspath(path))
    if not os.path.isdir(path):
        try:
            return TransactionSets(_path(path))
        except ParseError as error:
            raise ParseError(f"{path}: {error}") from error
    sets = []
    for name in os.listdir(path):
        if not name.endswith(_SUFFIXES):
            continue
        file_path = f"{path}/{name}"
        try:
            sets.extend(_path(file_path))
        except Exception as error:
            if debug:
                raise
            warnings.warn(f"Failed to build a transaction set from {file_path} with error: {error}")
    return TransactionSets(sets)
