"""Read-only objects shaped like edi-835-parser's, over the compat tables."""

from __future__ import annotations

import dataclasses
import enum
import functools
from typing import TYPE_CHECKING, Dict, List, Optional

from . import _codes
from ._convert import date, integer, money, name, text, written
from ._tables import position

if TYPE_CHECKING:
    from .. import Document
    from ._tables import Rows

SERVICE_DATE, _ = position("rows_service_dates", "date")
CLAIM_DATE, _ = position("rows_claim_dates", "date")


class PayerClassification(enum.Enum):
    PRIMARY = enum.auto()
    SECONDARY = enum.auto()
    TERTIARY = enum.auto()
    UNSPECIFIED = enum.auto()
    UNKNOWN = enum.auto()

    def __str__(self) -> str:
        return self.name.lower()


@dataclasses.dataclass(frozen=True)
class Code:
    code: Optional[str]
    description: Optional[str]

    def __str__(self) -> str:
        return str({"code": self.code, "description": self.description})


@dataclasses.dataclass(frozen=True)
class Status:
    code: str
    description: str
    payer_classification: PayerClassification
    was_forwarded: bool


def status(code):
    description, classification, forwarded = _codes.STATUSES.get(code, _codes.UNKNOWN_STATUS)
    return Status(code, description, PayerClassification[classification.upper()], forwarded)


def coded(table, value):
    return Code(value, table.get(value))


def mapped(table, value):
    return table.get(value, value)


@dataclasses.dataclass(frozen=True)
class Interchange:
    authorization_information_qualifier: Optional[str]
    sender: str
    receiver: str
    transmission_date: object


@dataclasses.dataclass(frozen=True)
class FinancialInformation:
    amount_paid: Optional[float]
    payment_method: Optional[str]
    routing_number: object
    transaction_date: object


@dataclasses.dataclass(frozen=True)
class OrganizationSegment:
    type: str
    name: Optional[str]
    identification_code: Optional[str]


@dataclasses.dataclass(frozen=True)
class Address:
    address: Optional[str]


@dataclasses.dataclass(frozen=True)
class Location:
    city: Optional[str]
    state: Optional[str]
    zip_code: Optional[str]


@dataclasses.dataclass(frozen=True)
class Organization:
    organization: OrganizationSegment
    location: Optional[Location]
    address: Optional[Address]
    index: Optional[int] = dataclasses.field(default=None, compare=False, repr=False)


@dataclasses.dataclass(frozen=True)
class Entity:
    entity: str
    type: str
    last_name: Optional[str]
    first_name: Optional[str]
    identification_code_qualifier: Optional[str]
    identification_code: Optional[str]
    index: Optional[int] = dataclasses.field(default=None, compare=False, repr=False)

    @property
    def name(self) -> str:
        return name(self.first_name, self.last_name)


@dataclasses.dataclass(frozen=True)
class Reference:
    qualifier: Code
    value: Optional[str]

    def __str__(self) -> str:
        return f"{self.qualifier}: {self.value}"


@dataclasses.dataclass(frozen=True)
class Date:
    qualifier: str
    date: object
    index: Optional[int] = dataclasses.field(default=None, compare=False, repr=False)


@dataclasses.dataclass(frozen=True)
class Amount:
    qualifier: str
    amount: Optional[float]


@dataclasses.dataclass(frozen=True)
class Remark:
    qualifier: Code
    code: Code


@dataclasses.dataclass(frozen=True)
class ServiceAdjustment:
    group_code: Code
    reason_code: Code
    amount: Optional[float]


@dataclasses.dataclass(frozen=True)
class ClaimSegment:
    marker: str
    status: Status
    charge_amount: Optional[float]
    paid_amount: Optional[float]
    claim_type: Optional[str]
    icn: Optional[str]


@dataclasses.dataclass(frozen=True)
class ServiceSegment:
    code: Optional[str]
    qualifier: Optional[str]
    modifier: Optional[str]
    charge_amount: Optional[float]
    paid_amount: Optional[float]
    allowed_units: object
    billed_units: object


def at_segment(items, index):
    """The item read from segment ``index``; ``None`` when ``index`` is."""
    if index is None:
        return None
    return next((item for item in items if item.index == index), None)


class Service:
    """One service line (loop 2110)."""

    def __init__(self, tables: Dict[str, Rows], document: Optional[Document], at: int, file_path: str) -> None:
        self._t, self._d, self._at, self._file_path = tables, document, at, file_path
        self._row = tables["rows"]["row"][at]

    def _under(self, table):
        return self._t[table], self._t[table].under("service", self._row)

    @functools.cached_property
    def service(self) -> ServiceSegment:
        """The SVC fields. Units the segment stops before take the library's
        defaults (allowed 0 or 1 by the paid amount, billed equal to allowed);
        units written empty are ``None``, as in the library."""
        rows, at = self._t["rows"], self._at
        paid = money(rows["paid_amount"][at])
        allowed = written(rows["allowed_units"][at], integer,
                          empty=None, absent=0 if paid == 0 else 1)
        billed = written(rows["billed_units"][at], integer, empty=None, absent=allowed)
        return ServiceSegment(
            code=text(rows["code"][at]),
            qualifier=text(rows["qualifier"][at]),
            modifier=written(rows["modifier"][at]),
            charge_amount=money(rows["charge_amount"][at]),
            paid_amount=paid,
            allowed_units=allowed,
            billed_units=billed,
        )

    @functools.cached_property
    def dates(self) -> List[Date]:
        t, ats = self._under("rows_service_dates")
        return [Date(mapped(_codes.DATE_QUALIFIERS, text(t["qualifier"][i])),
                     date(t["date"][i], self._d, t["segment"][i], SERVICE_DATE, self._file_path), t["segment"][i]) for i in ats]

    @functools.cached_property
    def references(self) -> List[Reference]:
        t, ats = self._under("rows_references")
        return [Reference(coded(_codes.REFERENCE_QUALIFIERS, text(t["qual"][i])), text(t["value"][i])) for i in ats]

    @functools.cached_property
    def remarks(self) -> List[Remark]:
        t, ats = self._under("rows_remarks")
        return [Remark(coded(_codes.REMARK_QUALIFIERS, text(t["qual"][i])), coded(_codes.REMARK_CODES, text(t["code"][i]))) for i in ats]

    @functools.cached_property
    def amount(self) -> Optional[Amount]:
        """The service's last AMT: the library keeps the last one it reads,
        while a spec column takes the first match."""
        t, ats = self._under("rows_service_amounts")
        if not ats:
            return None
        i = ats[-1]
        return Amount(mapped(_codes.AMOUNT_QUALIFIERS, text(t["qualifier"][i])), money(t["amount"][i]))

    @functools.cached_property
    def adjustments(self) -> List[ServiceAdjustment]:
        t, ats = self._under("rows_adjustments")
        return [ServiceAdjustment(coded(_codes.ADJUSTMENT_GROUPS, text(t["group"][i])),
                                  coded(_codes.ADJUSTMENT_REASONS, text(t["code"][i])), money(t["amount"][i])) for i in ats]

    @property
    def allowed_amount(self) -> Optional[float]:
        if self.amount and self.amount.qualifier == _codes.ALLOWED_ACTUAL:
            return self.amount.amount
        return None

    def _date(self, column):
        return at_segment(self.dates, self._t["rows"][column][self._at])

    @property
    def service_date(self) -> Optional[Date]:
        return self._date("service_date_segment")

    @property
    def service_period_start(self) -> Optional[Date]:
        return self._date("service_period_start_segment") or self.service_date

    @property
    def service_period_end(self) -> Optional[Date]:
        return self._date("service_period_end_segment") or self.service_date


class Claim:
    """One claim (loop 2100) with its services."""

    def __init__(self, tables: Dict[str, Rows], document: Optional[Document], at: int, file_path: str) -> None:
        self._t, self._d, self._at, self._file_path = tables, document, at, file_path
        self._row = tables["rows_claims"]["row"][at]

    def _under(self, table):
        return self._t[table], self._t[table].under("claim", self._row)

    @functools.cached_property
    def claim(self) -> ClaimSegment:
        c, at = self._t["rows_claims"], self._at
        return ClaimSegment(
            marker=text(c["marker"][at]),
            status=status(text(c["status"][at])),
            charge_amount=money(c["charge_amount"][at]),
            paid_amount=money(c["paid_amount"][at]),
            claim_type=text(c["claim_type"][at]),
            icn=text(c["icn"][at]),
        )

    @functools.cached_property
    def entities(self) -> List[Entity]:
        t, ats = self._under("rows_claim_entities")
        out = []
        for i in ats:
            segment = t["segment"][i]
            out.append(Entity(
                entity=mapped(_codes.ENTITY_CODES, text(t["entity"][i])),
                type=mapped(_codes.ENTITY_TYPES, text(t["type"][i])),
                last_name=text(t["last_name"][i]) or "",
                first_name=written(t["first_name"][i]),
                identification_code_qualifier=mapped(_codes.IDENTIFICATION_QUALIFIERS,
                                                     written(t["identification_code_qualifier"][i])),
                identification_code=written(t["identification_code"][i]),
                index=segment,
            ))
        return out

    @functools.cached_property
    def services(self) -> List[Service]:
        rows = self._t["rows"]
        return [Service(self._t, self._d, i, self._file_path) for i in rows.under("claim", self._row)]

    @functools.cached_property
    def references(self) -> List[Reference]:
        t, ats = self._under("rows_claim_references")
        return [Reference(coded(_codes.REFERENCE_QUALIFIERS, text(t["qualifier"][i])), text(t["value"][i])) for i in ats]

    @functools.cached_property
    def dates(self) -> List[Date]:
        t, ats = self._under("rows_claim_dates")
        return [Date(mapped(_codes.DATE_QUALIFIERS, text(t["qualifier"][i])),
                     date(t["date"][i], self._d, t["segment"][i], CLAIM_DATE, self._file_path), t["segment"][i]) for i in ats]

    @functools.cached_property
    def amount(self) -> Optional[Amount]:
        """The claim's last AMT: the library keeps the last one it reads,
        while a spec column takes the first match."""
        t, ats = self._under("rows_claim_amounts")
        if not ats:
            return None
        i = ats[-1]
        return Amount(mapped(_codes.AMOUNT_QUALIFIERS, text(t["qualifier"][i])), money(t["amount"][i]))

    def _pointer(self, items, column):
        return at_segment(items, self._t["rows_claims"][column][self._at])

    @property
    def patient(self) -> Optional[Entity]:
        return self._pointer(self.entities, "patient_segment")

    @property
    def rendering_provider(self) -> Optional[Entity]:
        return self._pointer(self.entities, "rendering_provider_segment")

    @property
    def claim_statement_period_start(self) -> Optional[Date]:
        return self._pointer(self.dates, "statement_period_start_segment")

    @property
    def claim_statement_period_end(self) -> Optional[Date]:
        return self._pointer(self.dates, "statement_period_end_segment")
