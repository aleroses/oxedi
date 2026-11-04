//! The entry points: tables and an envelope in, the file's bytes out.
//!
//! Writing compiles the spec's write plan, binds the caller's tables to the
//! spec's, nests the rows by their references, walks the loops and then
//! reads the written file back with the spec: every diagnostic of that read
//! is a finding, named by the cell, row or envelope field behind it. The
//! read is skipped when a value holds a delimiter, because the file then
//! splits where the data does not.

use crate::column::{Cell, Tables};
use crate::diagnostic::{Diagnostic, Rule};
use crate::document::Document;
use crate::process::Processor;
use crate::spec::Spec;

use super::data::{Data, bind};
use super::emit::Emitter;
use super::envelope::{Envelope, carries_repetition, delimiters};
use super::finding::{Finding, ValueOrigin, WriteError};
use super::layout::Layout;
use super::nest::nest;
use super::plan::WritePlan;
use super::render::{Separators, cell_display};
use super::trace::Traces;

/// Writes `tables` as one interchange of `spec`, or returns every finding
/// that keeps them from making a valid file, and nothing else.
///
/// `tables` hold the spec's tables by name, with the columns and types a
/// parse with the spec gives; a table left out has no rows and a column
/// left out is null.
///
/// A null cell writes nothing, so its element is left out at the end of a
/// segment; in the middle of a segment it is written as an empty element,
/// which a parse reads back as empty text, not null. Only trailing nulls
/// read back as null.
pub fn write(spec: &Spec, tables: &Tables, envelope: &Envelope) -> Result<Vec<u8>, WriteError> {
    let (bytes, findings) = write_with_findings(spec, tables, envelope)?;
    if findings.is_empty() {
        Ok(bytes)
    } else {
        Err(WriteError::Findings(findings))
    }
}

/// Writes `tables` as [`write()`] does, but returns the bytes together with
/// the findings instead of refusing them. Errors that leave nothing to
/// write (a spec that cannot be written, tables or delimiters that do not
/// fit) are still returned.
///
/// When a value holds a delimiter the written file splits where the data
/// does not, so it is not read back: the findings then list what the
/// writer itself found, without those of reading the file.
pub fn write_with_findings(
    spec: &Spec,
    tables: &Tables,
    envelope: &Envelope,
) -> Result<(Vec<u8>, Vec<Finding>), WriteError> {
    let plan = WritePlan::new(spec)?;
    let data = bind(spec, tables)?;
    let repetition = spec
        .roots()
        .iter()
        .find(|&&root| spec.get(root).control.is_some())
        .is_some_and(|&root| carries_repetition(spec, &spec.get(root).trigger.segment));
    let delimiters = delimiters(envelope, repetition)?;
    let mut findings = Vec::new();
    unwritten(spec, &plan, &data, &mut findings);
    let nest = nest(spec, &plan, &data, &mut findings);
    let layout = Layout::new(spec, &plan);
    let mut emitter = Emitter {
        spec,
        plan: &plan,
        layout: &layout,
        data: &data,
        nest: &nest,
        envelope,
        separators: Separators {
            element: envelope.delimiters.element,
            component: envelope.delimiters.component,
            segment: envelope.delimiters.segment,
            line_break: envelope.line_break,
        },
        delimiters,
        out: Vec::new(),
        traces: Traces::default(),
        findings: Vec::new(),
        last: vec![None; spec.tables().len()],
        counters: vec![envelope.control_number; spec.loops().len()],
        parts: Vec::new(),
        texts: Vec::new(),
    };
    emitter.run();
    let Emitter {
        out,
        traces,
        findings: found,
        ..
    } = emitter;
    findings.extend(found);
    let split = findings
        .iter()
        .any(|finding| matches!(finding, Finding::DelimiterInValue { .. }));
    if !split {
        read_back(spec, &data, &out, &traces, &mut findings)?;
    }
    Ok((out, findings))
}

/// Reports every value in a column that no valid file fills.
fn unwritten(spec: &Spec, plan: &WritePlan, data: &[Data<'_>], findings: &mut Vec<Finding>) {
    for reason in &plan.unwritten {
        let Some(table) = data.get(reason.table) else {
            continue;
        };
        let Some(column) = table.column(reason.column) else {
            continue;
        };
        let name = spec
            .tables()
            .get(reason.table)
            .and_then(|def| def.columns.get(reason.column))
            .map(|(name, _)| name.clone())
            .unwrap_or_default();
        for row in 0..table.rows {
            if column.get(row).is_none_or(|cell| cell == Cell::Null) {
                continue;
            }
            findings.push(Finding::UnwrittenValue {
                origin: ValueOrigin::Cell {
                    table: table.name.clone(),
                    row,
                    column: name.clone(),
                },
                value: cell_display(column, row),
                place: reason.place.clone(),
                code: reason.code.clone(),
                codes: reason.codes.clone(),
            });
        }
    }
}

/// Reads the written file back and reports its diagnostics.
fn read_back(
    spec: &Spec,
    data: &[Data<'_>],
    bytes: &[u8],
    traces: &Traces,
    findings: &mut Vec<Finding>,
) -> Result<(), WriteError> {
    let document = match Document::parse(bytes) {
        Ok(document) => document,
        Err(_) if !findings.is_empty() => return Ok(()),
        Err(error) => return Err(WriteError::Unreadable(error)),
    };
    let (_, diagnostics) = Processor::run(spec, &document, None);
    let names = |table: usize, column: Option<usize>| match column {
        None => data
            .get(table)
            .map(|data| data.name.clone())
            .unwrap_or_default(),
        Some(column) => spec
            .tables()
            .get(table)
            .and_then(|def| def.columns.get(column))
            .map(|(name, _)| name.clone())
            .unwrap_or_default(),
    };
    for diagnostic in diagnostics {
        let origin = origin_of(&diagnostic, traces, &names);
        findings.push(Finding::ReadBack { origin, diagnostic });
    }
    Ok(())
}

/// The cell, row or envelope field behind a diagnostic about the written
/// file. A rule about a whole instance, reported where it closes, names the
/// row that opened it; a control number mismatch names the closer's
/// element, which the envelope wrote.
pub(super) fn origin_of(
    diagnostic: &Diagnostic,
    traces: &Traces,
    names: &dyn Fn(usize, Option<usize>) -> String,
) -> Option<ValueOrigin> {
    let opened = match &diagnostic.rule {
        Rule::RequiredOccurrenceMissing { opened_at, .. }
        | Rule::RequiredLoopMissing { opened_at, .. }
        | Rule::UnterminatedLoop { opened_at, .. } => Some(*opened_at),
        _ => None,
    };
    match opened {
        Some(opened_at) => opened_at.and_then(|segment| traces.origin(segment, None, None, names)),
        None => diagnostic.segment.and_then(|segment| {
            traces.origin(segment, diagnostic.element, diagnostic.component, names)
        }),
    }
}
