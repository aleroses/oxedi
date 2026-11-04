//! Compilation of the columns flagged as patient data and of the seed column.

use std::fmt;

use super::Spec;
use super::error::SpecError;
use super::table_error::TableDefError;
use super::tables::ColumnSource;
use super::tables::TableDef;
use crate::mask::Phi;
use crate::value::ElementType;

/// What a column reads, in the spec's own terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnKind {
    /// The index of a segment.
    SegmentIndex,
    /// An element or component; `None` when the spec does not define it.
    Element(Option<ElementType>),
}

impl fmt::Display for ColumnKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ColumnKind::SegmentIndex => write!(f, "a segment index"),
            ColumnKind::Element(Some(kind)) => write!(f, "{kind}"),
            ColumnKind::Element(None) => write!(f, "an element the spec does not define"),
        }
    }
}

impl Spec {
    /// What `source` of a column of `table` reads: a segment index, or the
    /// element type the spec declares for the element (`None` when it
    /// declares none).
    pub(crate) fn column_kind(&self, table: &TableDef, source: &ColumnSource) -> ColumnKind {
        let def = match source {
            ColumnSource::Element {
                segment,
                element,
                component,
                ..
            } => self.element_def(segment, *element, *component),
            ColumnSource::SegmentIndex { .. } => return ColumnKind::SegmentIndex,
            ColumnSource::GroupElement { offset, component } => {
                match (&table.segment, table.repeat) {
                    (Some(segment), Some(repeat)) => repeat
                        .from
                        .checked_add(*offset)
                        .and_then(|position| self.element_def(segment, position, *component)),
                    _ => None,
                }
            }
        };
        ColumnKind::Element(def.map(|def| def.kind))
    }
}

impl ColumnKind {
    /// `true` for an element of a text type, or one the spec does not
    /// define (its bytes are kept as text).
    fn is_text(self) -> bool {
        matches!(self, ColumnKind::Element(kind) if kind.is_none_or(ElementType::is_text))
    }

    /// `true` for a date element.
    fn is_date(self) -> bool {
        self == ColumnKind::Element(Some(ElementType::Dt))
    }
}

/// Whether a column flagged `phi` reads what the flag masks: a date for
/// `date`, text for `name` and `id`.
fn phi_fits(kind: Phi, found: ColumnKind) -> Result<(), TableDefError> {
    match kind {
        Phi::Date if found.is_date() => Ok(()),
        Phi::Date => Err(TableDefError::PhiNeedsDate { found }),
        Phi::Name | Phi::Id if found.is_text() => Ok(()),
        Phi::Name | Phi::Id => Err(TableDefError::PhiNeedsText {
            kind: kind.word(),
            found,
        }),
    }
}

/// The segment id and 1-based element `source` reads when an envelope loop
/// uses that element as a control number or count. A repeated group reads
/// its element in every group, so each of those positions is checked.
fn control_element(
    spec: &Spec,
    table: &TableDef,
    source: &ColumnSource,
) -> Option<(String, usize)> {
    let (segment, first, step) = match source {
        ColumnSource::Element {
            segment, element, ..
        } => (segment, *element, 0),
        ColumnSource::GroupElement { offset, .. } => {
            let repeat = table.repeat?;
            (
                table.segment.as_ref()?,
                repeat.from.checked_add(*offset)?,
                repeat.step,
            )
        }
        ColumnSource::SegmentIndex { .. } => return None,
    };
    let reads = |position: usize| match position.checked_sub(first) {
        Some(0) => true,
        Some(distance) => step > 0 && distance % step == 0,
        None => false,
    };
    let mut controls = Vec::new();
    for def in spec.loops() {
        let Some(control) = def.control else {
            continue;
        };
        controls.push((def.trigger.segment.as_slice(), control.opener_element));
        if let Some(end) = def.end.as_deref() {
            controls.push((end, control.closer_element));
            controls.push((end, control.count_element));
        }
    }
    let (_, element) = controls
        .into_iter()
        .find(|&(id, position)| id == segment.as_slice() && reads(position))?;
    Some((String::from_utf8_lossy(segment).into_owned(), element))
}

/// Fills `table.phi` from the `phi` word each column wrote (parallel to
/// `table.columns`) and `table.phi_seed` from the `phi_seed` column name.
pub(super) fn compile_phi(
    spec: &Spec,
    table: &mut TableDef,
    written: &[Option<String>],
    seed: Option<&str>,
) -> Result<(), SpecError> {
    let fail = |column: Option<&str>, reason| SpecError::BadTable {
        table: table.name.clone(),
        column: column.map(str::to_string),
        reason,
    };
    let mut flags = Vec::with_capacity(table.columns.len());
    for ((name, source), word) in table.columns.iter().zip(written) {
        let Some(word) = word else {
            flags.push(None);
            continue;
        };
        let Some(kind) = Phi::from_word(word) else {
            return Err(fail(
                Some(name),
                TableDefError::UnknownPhi {
                    given: word.clone(),
                },
            ));
        };
        if let Some((segment, element)) = control_element(spec, table, source) {
            return Err(fail(
                Some(name),
                TableDefError::PhiOnControlElement { segment, element },
            ));
        }
        phi_fits(kind, spec.column_kind(table, source))
            .map_err(|reason| fail(Some(name), reason))?;
        flags.push(Some(kind));
    }
    let seed_index = match seed {
        None => None,
        Some(name) => {
            let Some(index) = table.columns.iter().position(|(column, _)| column == name) else {
                return Err(fail(
                    None,
                    TableDefError::UnknownPhiSeed {
                        name: name.to_string(),
                    },
                ));
            };
            if flags.get(index).copied().flatten() != Some(Phi::Id) {
                return Err(fail(
                    Some(name),
                    TableDefError::PhiSeedNotId {
                        name: name.to_string(),
                    },
                ));
            }
            Some(index)
        }
    };
    table.phi = flags;
    table.phi_seed = seed_index;
    Ok(())
}
