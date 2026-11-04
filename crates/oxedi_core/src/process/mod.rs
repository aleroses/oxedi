//! One pass over a segment stream with every consumer of the engine's events.
//!
//! The processor feeds each segment to the loop engine and hands the events
//! it returns to the structure checker and to the projector, so the loop
//! structure, the structural and element diagnostics and the table rows all
//! come from the same walk. [`Processor::run`] does the same over a whole
//! document and is the same code path. Given `Some(mask)`, [`Processor::run`]
//! and [`Processor::with_mask`] mask the columns the spec flags as patient
//! data and the datums of every diagnostic, the checker's included, read from
//! a place such a column reads (see [`Projector::with_mask`]); `None` masks
//! nothing.

use crate::check::StructureChecker;
use crate::column::Tables;
use crate::delimiters::Delimiters;
use crate::diagnostic::Diagnostic;
use crate::document::Document;
use crate::engine::{Event, LoopEngine};
use crate::mask::Mask;
use crate::project::{Projector, UnknownTable};
use crate::segment::Segment;
use crate::spec::Spec;

/// What one segment, or the end of the stream, produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    events: Vec<Event>,
    diagnostics: Vec<Diagnostic>,
}

impl Output {
    /// The engine's events, in order.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// The structure checker's diagnostics, then the projector's.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

/// The loop engine, the structure checker and the projector, fed together.
#[derive(Debug, Clone)]
pub struct Processor<'s> {
    engine: LoopEngine<'s>,
    checker: StructureChecker<'s>,
    projector: Projector<'s>,
    output: Output,
}

impl<'s> Processor<'s> {
    /// A processor at the root with empty tables; `delimiters` are those the
    /// segments were read with.
    pub fn new(spec: &'s Spec, delimiters: &Delimiters) -> Self {
        Self {
            engine: LoopEngine::new(spec),
            checker: StructureChecker::new(spec, delimiters),
            projector: Projector::new(spec, delimiters),
            output: Output::default(),
        }
    }

    /// A processor whose projector fills only the tables named in `names`;
    /// see [`Projector::with_tables`]. The events and the diagnostics are
    /// those of [`Processor::new`], except a text value too large to store
    /// in a table left out, which is not reported. A name the spec does not
    /// define is an [`UnknownTable`].
    pub fn with_tables(
        spec: &'s Spec,
        delimiters: &Delimiters,
        names: &[&str],
    ) -> Result<Self, UnknownTable> {
        Ok(Self {
            engine: LoopEngine::new(spec),
            checker: StructureChecker::new(spec, delimiters),
            projector: Projector::with_tables(spec, delimiters, names)?,
            output: Output::default(),
        })
    }

    /// The same processor; with `Some(mask)`, masking the columns the spec
    /// flags as patient data and the datums read from them (see
    /// [`Projector::with_mask`]). With `None` it is returned unchanged.
    pub fn with_mask(mut self, mask: Option<&'s Mask>) -> Self {
        if let Some(mask) = mask {
            self.projector = self.projector.with_mask(mask);
        }
        self
    }

    /// Consumes one segment. The output is valid until the next call.
    pub fn feed(&mut self, segment: &Segment<'_>) -> &Output {
        self.output.events.clear();
        self.output.diagnostics.clear();
        let events = self.engine.feed(segment);
        self.output.events.extend_from_slice(events);
        // The projector matches the captured segment to its occurrence and
        // the checker reuses the match; the checker's findings still come
        // first.
        self.projector.on(segment, events);
        let matched = self.projector.matched();
        self.output
            .diagnostics
            .extend_from_slice(self.checker.on_matched(segment, events, matched));
        self.projector
            .mask_datums(&mut self.output.diagnostics, Some(segment));
        self.output
            .diagnostics
            .extend_from_slice(self.projector.diagnostics());
        &self.output
    }

    /// Closes every loop still open and appends the rows they were
    /// collecting. The processor is then back at the root, as a new one;
    /// take the tables before feeding it another stream.
    pub fn finish(&mut self) -> &Output {
        self.output.events.clear();
        self.output.diagnostics.clear();
        self.output.events.extend_from_slice(self.engine.finish());
        self.output
            .diagnostics
            .extend_from_slice(self.checker.finish());
        self.projector
            .mask_datums(&mut self.output.diagnostics, None);
        self.output
            .diagnostics
            .extend_from_slice(self.projector.finish());
        &self.output
    }

    /// The diagnostics of the latest `feed` or `finish`.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        self.output.diagnostics()
    }

    /// Moves every appended row out; see [`Projector::take_tables`]. A text
    /// column holds at most `i32::MAX` bytes until drained (a value past that
    /// is reported and its cell is null), so drain per transaction. Each
    /// stream's tables stand on their own: take them before feeding another
    /// stream, because `finish` restarts row numbers.
    pub fn take_tables(&mut self) -> Tables {
        self.projector.take_tables()
    }

    /// Processes a whole document: every segment, then `finish`. Returns
    /// the tables and every diagnostic in stream order. With `Some(mask)`,
    /// the columns the spec flags as patient data and the datums read from
    /// them are masked.
    pub fn run(
        spec: &Spec,
        document: &Document<'_>,
        mask: Option<&Mask>,
    ) -> (Tables, Vec<Diagnostic>) {
        Processor::new(spec, document.delimiters())
            .with_mask(mask)
            .run_document(document)
    }

    /// Processes a whole document as [`Processor::run`] does, filling only
    /// the tables named in `names` and collecting no diagnostics. Each table
    /// it returns holds the rows `run` gives that table with the `mask`
    /// given to `run`. A name the spec does not define is an [`UnknownTable`].
    pub fn run_tables(
        spec: &Spec,
        document: &Document<'_>,
        names: &[&str],
        mask: Option<&Mask>,
    ) -> Result<Tables, UnknownTable> {
        let processor = Processor::with_tables(spec, document.delimiters(), names)?;
        Ok(processor.with_mask(mask).run_document_tables(document))
    }

    /// Feeds every segment of `document`, then finishes, collecting the
    /// diagnostics.
    fn run_document(mut self, document: &Document<'_>) -> (Tables, Vec<Diagnostic>) {
        let mut diagnostics = Vec::new();
        // One segment buffer for the whole pass: each segment is parsed into
        // the previous one's element and composite vectors.
        let mut segment = Segment::empty();
        let mut index = 0;
        while document.segment_into(index, &mut segment) {
            diagnostics.extend_from_slice(self.feed(&segment).diagnostics());
            index += 1;
        }
        diagnostics.extend_from_slice(self.finish().diagnostics());
        (self.take_tables(), diagnostics)
    }

    /// Feeds every segment of `document`, then finishes, keeping only the
    /// tables.
    fn run_document_tables(mut self, document: &Document<'_>) -> Tables {
        let mut segment = Segment::empty();
        let mut index = 0;
        while document.segment_into(index, &mut segment) {
            self.feed(&segment);
            index += 1;
        }
        self.finish();
        self.take_tables()
    }
}

#[cfg(test)]
mod mask_tests;
#[cfg(test)]
mod tests;
