//! The walk that writes the file: every loop instance the rows give, its
//! segments and child loops in position order, and the envelope's counts
//! and control numbers.
//!
//! An instance's context holds the row of each table it sits in, and, inside
//! an instance given by a run of grouped rows, the run. A segment a row
//! writes is written when one of its columns has a value, or when it opens
//! the instance. The rows of a table anchored on a segment are cut
//! into segments where the values outside their groups (or their segment
//! index) change, or when a segment holds as many groups as it can.

use crate::column::Cell;
use crate::spec::{ControlCount, LoopId, Spec};
use crate::value::{date_display, time_display};

use super::data::Data;
use super::envelope::{Envelope, Field, control_text, trigger_values};
use super::finding::{Finding, ValueOrigin};
use super::layout::{Item, Layout};
use super::nest::Nest;
use super::plan::{Instances, SegmentPlan, SegmentSource, ValueSource, WritePlan};
use super::refusal::place;
use super::render::{Part, Separators};
use super::trace::{Src, Traces};
use super::walk::Context;

/// The writer's state while it walks the loops.
pub(super) struct Emitter<'a> {
    pub(super) spec: &'a Spec,
    pub(super) plan: &'a WritePlan,
    pub(super) layout: &'a Layout,
    pub(super) data: &'a [Data<'a>],
    pub(super) nest: &'a Nest,
    pub(super) envelope: &'a Envelope,
    pub(super) separators: Separators,
    /// Every delimiter byte the file uses, with its role.
    pub(super) delimiters: Vec<(u8, &'static str)>,
    pub(super) out: Vec<u8>,
    pub(super) traces: Traces,
    pub(super) findings: Vec<Finding>,
    /// By table: the last row written.
    pub(super) last: Vec<Option<usize>>,
    /// By loop: the next control number.
    pub(super) counters: Vec<u64>,
    pub(super) parts: Vec<Part>,
    pub(super) texts: Vec<u8>,
}

impl Emitter<'_> {
    /// Writes every instance of the spec's top-level loops.
    pub(super) fn run(&mut self) {
        let context = Context {
            rows: vec![None; self.spec.tables().len()],
            run: None,
        };
        for &root in self.spec.roots() {
            for child in self.instances(root, &context) {
                self.instance(root, &child);
            }
        }
    }

    fn instance(&mut self, id: LoopId, context: &Context) {
        if let Some(Instances::Rows { table }) = self.loop_instances(id)
            && let Some(row) = context.rows.get(table).copied().flatten()
        {
            self.enter(table, row);
        }
        let start = self.traces.len();
        let mut children = 0;
        let mut control = None;
        let (layout, plan) = (self.layout, self.plan);
        let order = layout.order.get(id.index()).map_or(&[][..], Vec::as_slice);
        for &item in order {
            match item {
                Item::Segment(at) => {
                    let Some(segment) = plan
                        .loops
                        .get(id.index())
                        .and_then(|plan| plan.segments.get(at))
                    else {
                        continue;
                    };
                    match segment.source {
                        SegmentSource::Envelope => control = Some(self.envelope_trigger(id)),
                        SegmentSource::Row { table, .. } => {
                            self.row_segment(id, at, segment, table, context);
                        }
                        SegmentSource::Repeat { table } => {
                            self.repeat_segments(id, segment, table, context);
                        }
                    }
                }
                Item::Loop(child) => {
                    for inner in self.instances(child, context) {
                        self.instance(child, &inner);
                        children += 1;
                    }
                }
            }
        }
        let def = self.spec.get(id);
        if let (Some(end), Some(rule)) = (&def.end, def.control) {
            let count = match rule.count {
                ControlCount::Segments => self.traces.len() - start + 1,
                ControlCount::Children => children,
            };
            self.parts.clear();
            self.texts.clear();
            let mut values = vec![(rule.count_element, count.to_string().into_bytes())];
            values.push((rule.closer_element, control.unwrap_or_default()));
            values.sort_by_key(|(element, _)| *element);
            for (element, text) in values {
                if element == rule.closer_element {
                    self.traces
                        .entry(element, None, Src::Field(Field::ControlNumber));
                }
                self.push_text(element, None, &text);
            }
            let id = end.clone();
            self.finish_segment(&id, None);
        }
    }

    /// Marks a row of `table` as written, and reports it when the table
    /// lists it before a row already written.
    pub(super) fn enter(&mut self, table: usize, row: usize) {
        let Some(slot) = self.last.get_mut(table) else {
            return;
        };
        let previous = *slot;
        *slot = Some(previous.map_or(row, |previous| previous.max(row)));
        let Some(previous) = previous.filter(|&previous| row < previous) else {
            return;
        };
        let def = self.spec.tables().get(table);
        let above = match def.and_then(|def| def.segment.as_ref()) {
            Some(_) => self
                .nest
                .placed_by
                .get(table)
                .and_then(|rows| rows.get(row).copied().flatten()),
            None => def.and_then(|def| def.parent),
        };
        let (Some(above), Some(data)) = (above, self.data.get(table)) else {
            return;
        };
        self.findings.push(Finding::OutOfOrder {
            table: data.name.clone(),
            row,
            column: self
                .spec
                .tables()
                .get(above)
                .map(|def| def.reference.clone())
                .unwrap_or_default(),
            value: data.reference(above, row).unwrap_or_default(),
            previous,
        });
    }

    /// Writes the trigger of an envelope loop and returns its control number.
    fn envelope_trigger(&mut self, id: LoopId) -> Vec<u8> {
        let def = self.spec.get(id);
        let Some(rule) = def.control else {
            return Vec::new();
        };
        let trigger = def.trigger.segment.clone();
        let number = self
            .counters
            .get(id.index())
            .copied()
            .unwrap_or(self.envelope.control_number);
        if let Some(counter) = self.counters.get_mut(id.index()) {
            *counter = counter.saturating_add(1);
        }
        let control = control_text(
            number,
            self.spec.element_def(&trigger, rule.opener_element, None),
        );
        let depth = self
            .layout
            .depth
            .get(id.index())
            .copied()
            .flatten()
            .unwrap_or(0);
        let values = trigger_values(
            self.spec,
            &trigger,
            depth,
            rule.opener_element,
            &control,
            self.envelope,
        );
        self.parts.clear();
        self.texts.clear();
        for value in values {
            if let Some(field) = value.field {
                let origin = ValueOrigin::Envelope {
                    field: field.name().to_string(),
                };
                if value.checked {
                    self.check_delimiters(&origin, &trigger, value.element, None, &value.bytes);
                }
                if let Some(reason) = value.refused {
                    let shown = match field {
                        Field::Time => time_display(self.envelope.time),
                        _ => date_display(self.envelope.date),
                    };
                    self.findings.push(Finding::NotWritable {
                        origin: origin.clone(),
                        place: place(&trigger, value.element, None),
                        value: shown,
                        reason,
                    });
                }
                self.traces.entry(value.element, None, Src::Field(field));
            }
            self.push_text(value.element, None, &value.bytes);
        }
        self.finish_segment(&trigger, None);
        control
    }

    /// Writes the segment a row gives, when it has a value or is required.
    fn row_segment(
        &mut self,
        id: LoopId,
        at: usize,
        segment: &SegmentPlan,
        table: usize,
        context: &Context,
    ) {
        let Some(row) = context.rows.get(table).copied().flatten() else {
            return;
        };
        let Some(data) = self.data.get(table) else {
            return;
        };
        let forced = self
            .layout
            .forced
            .get(id.index())
            .and_then(|forced| forced.get(at))
            .copied()
            .unwrap_or(false);
        let any = segment.elements.iter().any(|element| {
            matches!(element.value, ValueSource::Column { column } if data.cell(column, row) != Cell::Null)
        });
        if !forced && !any {
            return;
        }
        let spec = self.spec;
        let Some(occurrence) = spec.get(id).occurrences.get(segment.occurrence) else {
            return;
        };
        let id_bytes = occurrence.segment.as_slice();
        self.parts.clear();
        self.texts.clear();
        for element in &segment.elements {
            match &element.value {
                ValueSource::Column { column } => {
                    self.push_cell(
                        id_bytes,
                        element.element,
                        element.component,
                        table,
                        *column,
                        row,
                    );
                }
                ValueSource::Code(code) => {
                    self.push_text(element.element, element.component, code);
                }
                ValueSource::Group { .. } => {}
            }
        }
        self.finish_segment(id_bytes, Some((table, row)));
    }

    /// Writes the segments the rows of a table anchored on a segment give
    /// in this instance.
    fn repeat_segments(
        &mut self,
        id: LoopId,
        segment: &SegmentPlan,
        table: usize,
        context: &Context,
    ) {
        let Some(Instances::Rows { table: owner }) = self.loop_instances(id) else {
            return;
        };
        let Some(owner_row) = context.rows.get(owner).copied().flatten() else {
            return;
        };
        let (spec, nest, all) = (self.spec, self.nest, self.data);
        let rows = nest.repeat_rows(table, id.index(), owner_row);
        let (Some(data), Some(def)) = (all.get(table), spec.tables().get(table)) else {
            return;
        };
        let Some(occurrence) = spec.get(id).occurrences.get(segment.occurrence) else {
            return;
        };
        let id_bytes = occurrence.segment.as_slice();
        let step = def.repeat.map_or(0, |repeat| repeat.step);
        let capacity = self.layout.capacity.get(table).copied().unwrap_or(1).max(1);
        let outside: Vec<usize> = segment
            .elements
            .iter()
            .filter_map(|element| match element.value {
                ValueSource::Column { column } => Some(column),
                _ => None,
            })
            .collect();
        let same = |a: usize, b: usize| {
            let index = |row| data.segment.and_then(|column| column.get(row));
            index(a) == index(b)
                && outside
                    .iter()
                    .all(|&column| data.cell(column, a) == data.cell(column, b))
        };
        let mut from = 0;
        while let Some(&first) = rows.get(from) {
            let to = rows
                .iter()
                .skip(from)
                .take(capacity)
                .position(|&row| !same(first, row))
                .map_or((from + capacity).min(rows.len()), |at| from + at);
            let chunk = rows.get(from..to).unwrap_or(&[]);
            from = to.max(from + 1);
            self.parts.clear();
            self.texts.clear();
            for (k, &row) in chunk.iter().enumerate() {
                self.enter(table, row);
                for element in &segment.elements {
                    match &element.value {
                        ValueSource::Column { column } if k == 0 => {
                            self.push_cell(
                                id_bytes,
                                element.element,
                                element.component,
                                table,
                                *column,
                                row,
                            );
                        }
                        ValueSource::Code(code) if k == 0 => {
                            self.push_text(element.element, element.component, code);
                        }
                        ValueSource::Group { column, .. } => {
                            let position = element.element.saturating_add(k.saturating_mul(step));
                            self.push_cell(
                                id_bytes,
                                position,
                                element.component,
                                table,
                                *column,
                                row,
                            );
                        }
                        _ => {}
                    }
                }
            }
            self.parts
                .sort_by_key(|part| (part.element, part.component));
            self.finish_segment(id_bytes, Some((table, first)));
        }
    }
}
