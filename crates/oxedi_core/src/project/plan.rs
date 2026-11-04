//! The per-segment plans the projector builds once from the spec.

use std::collections::BTreeMap;

use super::carry::Carried;
use crate::spec::{
    ColumnKind, ColumnSource, ElementDef, OccurrenceDef, Pick, Qualifier, Spec, TableDef,
};
use crate::value::ColumnType;

/// One defined element, with what checking it needs worked out once.
#[derive(Debug, Clone)]
pub(super) struct ElementPlan<'s> {
    pub(super) position: usize,
    pub(super) def: &'s ElementDef,
    /// The column type the definition maps to.
    pub(super) kind: ColumnType,
    /// Highest component position the definition declares (0 for a simple
    /// element).
    pub(super) declared: usize,
    /// The components by position, with their column types and whether a
    /// column reads them.
    pub(super) components: Vec<(usize, &'s ElementDef, ColumnType, bool)>,
    /// Whether a column reads the element as a whole. An element or
    /// component no column reads is validated without keeping its value.
    pub(super) read: bool,
}

impl<'s> ElementPlan<'s> {
    pub(super) fn new(position: usize, def: &'s ElementDef) -> Self {
        Self {
            position,
            def,
            kind: ColumnType::of(Some(def.kind)),
            declared: def.composite.keys().copied().max().unwrap_or_default(),
            components: def
                .composite
                .iter()
                .map(|(&at, part)| (at, part, ColumnType::of(Some(part.kind)), false))
                .collect(),
            read: false,
        }
    }

    /// Records that a column reads this element (`component` `None`) or one
    /// of its declared components.
    fn mark_read(&mut self, component: Option<usize>) {
        match component {
            None => self.read = true,
            Some(wanted) => self
                .components
                .iter_mut()
                .filter(|(at, ..)| *at == wanted)
                .for_each(|(.., read)| *read = true),
        }
    }
}

/// What the projector does with one segment id, worked out once.
#[derive(Debug, Clone)]
pub(super) struct SegmentPlan<'s> {
    /// The defined elements to check, by position.
    pub(super) elements: Vec<ElementPlan<'s>>,
    /// Per loop: the columns that read this segment when it is captured in
    /// that loop.
    pub(super) watchers: Vec<Vec<Watcher>>,
    /// Per loop: the loop's occurrences of this segment id, the only ones a
    /// captured segment can match.
    pub(super) occurrences: Vec<Vec<Candidate<'s>>>,
}

/// A column that reads the segments of one id a loop captures.
#[derive(Debug, Clone, Copy)]
pub(super) struct Watcher {
    /// The table, as an index into the spec's tables.
    pub(super) table: usize,
    /// The column, as an index into the table's declared columns.
    pub(super) column: usize,
    /// For a column that names an occurrence, its index in the capturing
    /// loop's occurrences: the segment must match it, and the column's
    /// conditions are not read.
    pub(super) occurrence: Option<usize>,
    /// Which matching segment gives the value.
    pub(super) pick: Pick,
    /// For a column that reads a loop above the table's anchor, its index
    /// in the table's carried values: the value is kept there until a row
    /// opens, instead of going to the open row.
    pub(super) carried: Option<usize>,
}

/// An occurrence a segment id can match in a loop, with what matching it
/// and checking its elements read.
#[derive(Debug, Clone, Copy)]
pub(super) struct Candidate<'s> {
    /// Index in the loop's occurrences.
    pub(super) index: usize,
    /// The qualifier the segment must hold; `None` matches any segment of
    /// the id.
    pub(super) qualifier: Option<&'s Qualifier>,
    /// The occurrence, when it has code lists of its own.
    pub(super) own_codes: Option<&'s OccurrenceDef>,
}

/// The plans of every segment id. Ids of up to seven bytes are keyed by their
/// packed bytes, which compare faster than slices.
#[derive(Debug, Clone, Default)]
pub(super) struct Plans<'s> {
    short: BTreeMap<u64, SegmentPlan<'s>>,
    long: BTreeMap<Vec<u8>, SegmentPlan<'s>>,
}

impl<'s> Plans<'s> {
    /// The packed key of an id, or `None` when it is too long to pack. A
    /// leading 1 bit keeps ids of different lengths apart.
    fn key(id: &[u8]) -> Option<u64> {
        (id.len() <= 7).then(|| {
            id.iter()
                .fold(1u64, |key, &byte| key << 8 | u64::from(byte))
        })
    }

    pub(super) fn get(&self, id: &[u8]) -> Option<&SegmentPlan<'s>> {
        match Self::key(id) {
            Some(key) => self.short.get(&key),
            None => self.long.get(id),
        }
    }

    /// Records that a column reads `element` (or its `component`) of the
    /// segments with `id`, when the spec defines that element.
    fn mark_read(&mut self, id: &[u8], element: usize, component: Option<usize>) {
        let plan = match Self::key(id) {
            Some(key) => self.short.get_mut(&key),
            None => self.long.get_mut(id),
        };
        plan.into_iter()
            .flat_map(|plan| plan.elements.iter_mut())
            .filter(|plan| plan.position == element)
            .for_each(|plan| plan.mark_read(component));
    }

    /// Marks every element and component some column of `table` can read.
    pub(super) fn mark_columns(&mut self, table: &TableDef) {
        for (_, source) in &table.columns {
            for (id, element, component) in self.positions(table, source) {
                self.mark_read(id, element, component);
            }
        }
    }

    /// The segment id, element and component of every place `source`, a
    /// column of `table`, can read. A column of a table anchored on a
    /// segment reads that segment too, and a group column reads its offset
    /// in every group of the defined elements the segment can hold.
    pub(super) fn positions<'t>(
        &self,
        table: &'t TableDef,
        source: &'t ColumnSource,
    ) -> Vec<(&'t [u8], usize, Option<usize>)> {
        let mut positions = Vec::new();
        match source {
            ColumnSource::Element {
                segment,
                element,
                component,
                ..
            } => {
                positions.push((segment.as_slice(), *element, *component));
                if let Some(anchor) = &table.segment {
                    positions.push((anchor.as_slice(), *element, *component));
                }
            }
            ColumnSource::GroupElement { offset, component } => {
                let (Some(anchor), Some(repeat)) = (&table.segment, table.repeat) else {
                    return positions;
                };
                let Some(first) = repeat.from.checked_add(*offset) else {
                    return positions;
                };
                let defined = self
                    .get(anchor)
                    .into_iter()
                    .flat_map(|plan| plan.elements.iter().map(|e| e.position));
                for position in defined {
                    let in_group = position
                        .checked_sub(first)
                        .is_some_and(|gap| gap.checked_rem(repeat.step).unwrap_or(gap) == 0);
                    if in_group {
                        positions.push((anchor.as_slice(), position, *component));
                    }
                }
            }
            ColumnSource::SegmentIndex { .. } => {}
        }
        positions
    }

    /// Makes every column of table `index` (one without `segment`) watch the
    /// segment id it reads in each loop it reads from. A column that reads
    /// a loop above the anchor gets a carried value, which an instance of
    /// that loop opening starts over (recorded in `resets`, per loop); the
    /// carried values are returned in column order.
    pub(super) fn watch_columns(
        &mut self,
        spec: &'s Spec,
        index: usize,
        resets: &mut [Vec<(usize, usize)>],
    ) -> Vec<Carried> {
        let def = &spec.tables()[index];
        let loops = spec.loops().len();
        let mut carried = Vec::new();
        for (column, (_, source)) in def.columns.iter().enumerate() {
            let (reader, segment, occurrence, pick) = match source {
                ColumnSource::Element {
                    loop_id,
                    segment,
                    occurrence,
                    pick,
                    ..
                }
                | ColumnSource::SegmentIndex {
                    loop_id,
                    segment,
                    occurrence,
                    pick,
                    ..
                } => (*loop_id, segment, occurrence.as_deref(), *pick),
                ColumnSource::GroupElement { .. } => continue,
            };
            let above = reader.filter(|&id| {
                def.loops
                    .iter()
                    .any(|&anchor| spec.ancestors(anchor).contains(&id))
            });
            let slot = match above.and_then(|id| resets.get_mut(id.index())) {
                Some(reset) => {
                    reset.push((index, carried.len()));
                    carried.push(Carried::new(column));
                    Some(carried.len() - 1)
                }
                None => None,
            };
            let readers = reader.map_or_else(|| def.loops.clone(), |id| vec![id]);
            let plan = self.entry(segment, loops);
            for id in readers {
                let occurrence = match occurrence {
                    None => None,
                    // Compilation checked that every reading loop declares
                    // the occurrence.
                    Some(name) => {
                        match spec.get(id).occurrences.iter().position(|o| o.name == name) {
                            Some(found) => Some(found),
                            None => continue,
                        }
                    }
                };
                if let Some(watchers) = plan.watchers.get_mut(id.index()) {
                    watchers.push(Watcher {
                        table: index,
                        column,
                        occurrence,
                        pick,
                        carried: slot,
                    });
                }
            }
        }
        carried
    }

    /// The plan of `id`, created empty (with room for `loops` watcher lists)
    /// when there is none.
    pub(super) fn entry(&mut self, id: &[u8], loops: usize) -> &mut SegmentPlan<'s> {
        let empty = || SegmentPlan {
            elements: Vec::new(),
            watchers: vec![Vec::new(); loops],
            occurrences: vec![Vec::new(); loops],
        };
        match Self::key(id) {
            Some(key) => self.short.entry(key).or_insert_with(empty),
            None => self.long.entry(id.to_vec()).or_insert_with(empty),
        }
    }
}

/// The column type of a declared column: its element's type, `Int64` for a
/// segment index, `Binary` for an element the spec does not define.
pub(super) fn column_type(spec: &Spec, table: &TableDef, source: &ColumnSource) -> ColumnType {
    match spec.column_kind(table, source) {
        ColumnKind::SegmentIndex => ColumnType::Int64 { scale: 0 },
        ColumnKind::Element(kind) => ColumnType::of(kind),
    }
}
