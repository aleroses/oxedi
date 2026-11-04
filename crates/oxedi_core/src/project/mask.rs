//! Masking of the columns the spec flags as patient data, in the rows the
//! projector appends and in the datums of the diagnostics it hands out.
//!
//! A row is masked just before it is appended, so every column has its final
//! value whatever order the segments came in. Text of a `name` or `id` column
//! becomes its keyed token; an empty text stays empty and a null stays null.
//! A `date` column follows the mask's date mode: the year keeps January 1 of
//! the same year, a shift adds the days the row's seed gives, and keep leaves
//! it. The seed is the unmasked value of the table's seed column, or, for a
//! table that declares none, of the nearest table above it that declares one
//! (its row is still open, so still unmasked). A row with no seed value, or
//! whose shift would leave the date range, gets the year instead.
//!
//! A diagnostic's datum is masked when the place it names (segment id,
//! element, component) is one a flagged column can read: a `name` or `id`
//! datum becomes its token, a `date` datum that is a valid date becomes the
//! masked date written `CCYYMMDD` (the year, as no row is at hand to seed a
//! shift), and one that is not becomes its token. An element read whole
//! covers its components, and a finding about a whole element covers the
//! components read from it. Places are matched by segment id alone, so the
//! same element of a segment in another loop, or with another qualifier, is
//! masked too. The segment id is the one the rule names, or the segment being
//! fed when the diagnostic is about it; a balancing finding names an earlier
//! segment, but its datum is an `R` amount, which no flagged column reads.
//! Findings another validator raised about a document are matched the same
//! way, by the segment their index names in it. The message of such a finding
//! may quote values of that segment whatever element it names, or none, so
//! the original datum, when the datum was masked, and each value the segment
//! holds at a flagged place are replaced by their masked forms where the
//! message quotes them; the module notes of `quote.rs` say where.

use crate::diagnostic::{Diagnostic, Rule};
use crate::document::Document;
use crate::element::Element;
use crate::mask::{DateMode, Mask, Phi};
use crate::segment::Segment;
use crate::spec::Spec;
use crate::value::civil_from_days;
use crate::value::parse_dt;

use super::plan::Plans;
use super::quote::unquoted;
use super::{Projector, Row, Slot};

/// The places flagged columns read, and the mask that rewrites the datums
/// found there.
#[derive(Debug, Clone)]
pub(super) struct DatumMask<'s> {
    mask: &'s Mask,
    places: Vec<(&'s [u8], usize, Option<usize>, Phi)>,
}

impl<'s> DatumMask<'s> {
    /// The places every flagged column of `spec` reads, as the projector's
    /// plans resolve them.
    pub(super) fn new(spec: &'s Spec, plans: &Plans<'_>, mask: &'s Mask) -> Self {
        let mut places = Vec::new();
        for table in spec.tables() {
            for (index, (_, source)) in table.columns.iter().enumerate() {
                let Some(kind) = table.phi(index) else {
                    continue;
                };
                for (id, element, component) in plans.positions(table, source) {
                    places.push((id, element, component, kind));
                }
            }
        }
        Self { mask, places }
    }

    /// Masks the datum of each diagnostic about a flagged place; `current`
    /// is the segment being fed, if any. An external finding about `current`
    /// also has its message rewritten: the original datum, when it was
    /// masked, and the value of every flagged place of `current` are replaced
    /// by their masked forms.
    pub(super) fn apply(&self, diagnostics: &mut [Diagnostic], current: Option<&Segment<'_>>) {
        for diagnostic in diagnostics {
            let mut quoted = Vec::new();
            if let Some(masked) = self.masked_datum(diagnostic, current) {
                let original = std::mem::replace(&mut diagnostic.datum, masked.clone());
                quoted.push((original, masked));
            }
            let about = current.filter(|segment| diagnostic.segment == Some(segment.index));
            if let Rule::External { message, .. } = &mut diagnostic.rule {
                if let Some(segment) = about {
                    quoted.extend(self.values(segment));
                }
                if !quoted.is_empty() {
                    *message = unquoted(message, &quoted);
                }
            }
        }
    }

    /// The masked form of the datum of `diagnostic` when the place it names
    /// is flagged, or `None` when the datum stays as written.
    fn masked_datum(
        &self,
        diagnostic: &Diagnostic,
        current: Option<&Segment<'_>>,
    ) -> Option<Vec<u8>> {
        if diagnostic.datum.is_empty() {
            return None;
        }
        let element = diagnostic.element?;
        let id = match diagnostic.rule.segment_id() {
            Some(id) => id,
            None => match current {
                Some(segment) if diagnostic.segment == Some(segment.index) => segment.id,
                _ => return None,
            },
        };
        let component = diagnostic.component;
        let &(.., kind) = self.places.iter().find(|(at, position, part, _)| {
            *at == id
                && *position == element
                && (part.is_none() || component.is_none() || *part == component)
        })?;
        self.datum(kind, &diagnostic.datum)
    }

    /// Each non-empty value `segment` holds at a flagged place, with its
    /// masked form; a place read whole gives every component of a composite.
    fn values(&self, segment: &Segment<'_>) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut values = Vec::new();
        for &(id, element, component, kind) in &self.places {
            if id != segment.id {
                continue;
            }
            let read: Vec<&[u8]> = match (component, segment.element(element)) {
                (Some(component), _) => {
                    segment.leaf(element, Some(component)).into_iter().collect()
                }
                (None, Some(Element::Simple(value))) => vec![value.as_ref()],
                (None, Some(Element::Composite(parts))) => {
                    parts.iter().map(AsRef::as_ref).collect()
                }
                (None, None) => Vec::new(),
            };
            for value in read {
                if value.is_empty() {
                    continue;
                }
                if let Some(masked) = self.datum(kind, value) {
                    values.push((value.to_vec(), masked));
                }
            }
        }
        values
    }

    /// The masked form of a datum, or `None` when it stays as written.
    fn datum(&self, kind: Phi, datum: &[u8]) -> Option<Vec<u8>> {
        match kind {
            Phi::Name | Phi::Id => Some(self.mask.text(kind, datum).to_vec()),
            Phi::Date if self.mask.dates() == DateMode::Keep => None,
            Phi::Date => Some(date_datum(self.mask, parse_dt(datum), datum)),
        }
    }
}

impl<'s> Projector<'s> {
    /// The same projector, masking the columns the spec flags as patient
    /// data in every row it appends and the datums of its diagnostics read
    /// from them. With dates shifted, a table left out by
    /// [`Projector::with_tables`] that declares a seed column still reads
    /// its rows, so the tables below it shift as in a full projection.
    pub fn with_mask(mut self, mask: &'s Mask) -> Self {
        let spec = self.spec;
        for (state, def) in self.tables.iter_mut().zip(spec.tables()) {
            state.masked = (0..def.columns.len()).any(|column| def.phi(column).is_some());
            if mask.dates() == DateMode::Shift && def.phi_seed().is_some() {
                state.collect = true;
            }
        }
        self.datums = Some(DatumMask::new(spec, &self.plans, mask));
        self.mask = Some(mask);
        self
    }

    /// Masks the datums of diagnostics another consumer raised, as this
    /// projector masks its own; `current` is the segment being fed, if any.
    pub(crate) fn mask_datums(
        &self,
        diagnostics: &mut [Diagnostic],
        current: Option<&Segment<'_>>,
    ) {
        if let Some(datums) = &self.datums {
            datums.apply(diagnostics, current);
        }
    }

    /// Masks the datums of diagnostics another validator raised about
    /// `document`, as this projector masks its own: each is matched by the
    /// segment its index names. An external finding also has the original
    /// datum, when the datum is masked, and each value that segment holds at
    /// a flagged place, read as UTF-8 or as Latin-1, replaced by its masked
    /// form where its message quotes them: a value of 4 characters or more
    /// wherever it occurs, a shorter one only where it, or the group of
    /// short values it overlaps or touches, stands apart from the message's
    /// words and numbers. Without a mask it does nothing.
    pub fn mask_found(&self, diagnostics: &mut [Diagnostic], document: &Document<'_>) {
        let Some(datums) = &self.datums else {
            return;
        };
        for diagnostic in diagnostics {
            let current = diagnostic.segment.and_then(|index| document.segment(index));
            datums.apply(std::slice::from_mut(diagnostic), current.as_ref());
        }
    }

    /// Masks the flagged columns of `row`, a row of table `index` about to
    /// be appended.
    pub(super) fn mask_row(&self, index: usize, row: &mut Row) {
        let Some(mask) = self.mask else {
            return;
        };
        let def = &self.spec.tables()[index];
        let shift = match mask.dates() {
            DateMode::Shift => self.seed(index, row).map(|seed| mask.shift_days(seed)),
            DateMode::Year | DateMode::Keep => None,
        };
        let Row { cells, bytes, .. } = row;
        for (column, slot) in cells.iter_mut().enumerate() {
            match (def.phi(column), *slot) {
                (Some(kind @ (Phi::Name | Phi::Id)), Slot::Bytes(start, end)) if end > start => {
                    let token = mask.text(kind, bytes.get(start..end).unwrap_or_default());
                    let from = bytes.len();
                    bytes.extend_from_slice(&token);
                    *slot = Slot::Bytes(from, bytes.len());
                }
                (Some(Phi::Date), Slot::Date(days)) => {
                    *slot = masked_date(mask.dates(), days, shift).map_or(Slot::Null, Slot::Date);
                }
                _ => {}
            }
        }
    }

    /// The unmasked seed of `row`, a row of table `index`: its own seed
    /// column, or the open row of the nearest table above that declares one.
    fn seed<'r>(&'r self, index: usize, row: &'r Row) -> Option<&'r [u8]> {
        let tables = self.spec.tables();
        let def = &tables[index];
        let (source, column) = match def.phi_seed() {
            Some(column) => (row, column),
            None => {
                let above = def
                    .ancestors
                    .iter()
                    .rev()
                    .copied()
                    .find(|&above| tables[above].phi_seed().is_some())?;
                let state = &self.tables[above];
                (state.open.then_some(&state.row)?, tables[above].phi_seed()?)
            }
        };
        match source.cells.get(column)? {
            Slot::Bytes(start, end) if end > start => source.bytes.get(*start..*end),
            _ => None,
        }
    }
}

/// A date datum under the mask's year start: `CCYYMMDD` of January 1, or the
/// keyed token of the datum when it is not a date or its year start has no
/// civil form.
pub(super) fn date_datum(mask: &Mask, days: Option<i32>, datum: &[u8]) -> Vec<u8> {
    match days.and_then(Mask::year_start).and_then(civil_from_days) {
        Some((year, month, day)) => format!("{year:04}{month:02}{day:02}").into_bytes(),
        None => mask.text(Phi::Date, datum).to_vec(),
    }
}

/// A date of a flagged column under `dates`, with the row's `shift` in days
/// when one could be seeded; `None` when the date has no year start to fall
/// back on, so the cell holds no date rather than an unmasked one.
pub(super) fn masked_date(dates: DateMode, days: i32, shift: Option<i32>) -> Option<i32> {
    match dates {
        DateMode::Keep => Some(days),
        DateMode::Year => Mask::year_start(days),
        DateMode::Shift => shift
            .and_then(|shift| days.checked_add(shift))
            .or_else(|| Mask::year_start(days)),
    }
}
