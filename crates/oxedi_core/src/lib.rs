//! Lossless, fast, data-driven EDI 835 parser core.
//!
//! Bytes in, structure out, in layers that each point back at the input:
//!
//! - [`Tokenizer`] reads the delimiters from the ISA and yields generic
//!   [`Segment`]s lazily; [`Segment::write_to`] writes them back.
//! - [`Document`] indexes every segment of a buffer once so any segment can be
//!   reached by index or byte span.
//! - [`Spec`] is a JSON loop structure (parents, triggers, held segments, end
//!   segments, envelope controls), the names and types of each segment's
//!   elements, and the tables to project, and can be patched with JSON Merge
//!   Patch; [`Spec::builtin_835`] ships the 835 as such data, and
//!   [`Spec::builtin_835_4010`] its 4010 version. A spec may declare the
//!   version it covers, and [`Spec::select`] picks among specs by it.
//! - [`LoopEngine`] interprets a segment stream against a spec and emits
//!   [`Event`]s (loops opened and closed, segments captured or unmatched);
//!   [`LoopTree`] collects those events into a tree of loop instances.
//! - [`StructureChecker`] reads the same events and reports structural
//!   [`Diagnostic`]s: unknown segments, implicit or unterminated loops, and
//!   envelope counts or control numbers that do not match.
//! - [`Projector`] reads the same events too: it checks every element the
//!   spec defines (required, type, length, code list, components) and fills the spec's
//!   tables, typed [`Table`]s of [`ColumnData`] laid out as Apache Arrow
//!   lays out its arrays, with each row pointing at the rows that enclose it.
//!   [`spec_schema`] gives those tables' columns, types and references from
//!   the spec alone, without a file. [`summarize`] computes the summaries
//!   the spec declares (counts, sums, the fields of one row) over them.
//! - [`write::write()`] turns tables with a spec's schema back into an
//!   interchange: the spec's projection inverted, the loop tree rebuilt from
//!   the tables' references, the envelope's counts and control numbers
//!   computed, and the written file read back so every finding names the
//!   cell it comes from.
//! - [`Processor`] feeds one segment at a time to the engine, the checker and
//!   the projector; [`Processor::run`] does it over a whole [`Document`].
//!
//! The structure is data: no code in this crate is specific to the 835 beyond
//! the built-in spec it loads.

pub mod check;
pub mod column;
pub mod delimiters;
pub mod diagnostic;
pub mod document;
pub mod element;
pub mod engine;
pub mod frame;
pub mod mask;
pub mod process;
pub mod project;
pub mod segment;
pub mod spec;
pub mod summary;
pub mod tokenizer;
pub mod tree;
pub mod trivia;
pub mod value;
pub mod write;

pub use check::StructureChecker;
pub use column::{Bitmap, Cell, CellError, Column, ColumnData, RowError, Table, Tables};
pub use delimiters::{Delimiters, IsaError};
pub use diagnostic::{Diagnostic, FlatDiagnostic, LoopRef, PARSER_ORIGIN, Rule, SnipLevel};
pub use document::{Document, DocumentError, Segments, SizeError, Span, Spans};
pub use element::{Element, Value};
pub use engine::{Event, LoopEngine};
pub use frame::{Frame, next_frame};
pub use mask::{DateMode, Mask, MaskError, Phi};
pub use process::{Output, Processor};
pub use project::{ColumnSchema, Projector, TableSchema, UnknownTable, spec_schema};
pub use segment::{Segment, SegmentWriteError};
pub use spec::{
    AnchorChains, AnchorPositions, BalanceError, BalanceRule, BalanceTerm, Builtin, ColumnSource,
    Control, ControlCount, ControlError, DeclaredVersion, ElementDef, ElementDefError, LoopDef,
    LoopId, LoopSegments, Pick, Repeat, SegmentDef, Spec, SpecError, Summary, SummaryDefError,
    TableDef, TableDefError, Trigger, UnknownVersion, Version, VersionError, merge_patch,
};
pub use summary::{SummaryError, SummaryValue, summarize};
pub use tokenizer::Tokenizer;
pub use tree::{LoopTree, Node, NodeId, TreeBuilder};
pub use value::{ColumnType, ConvertError, ElementType, parse_dt, parse_n, parse_r, parse_tm};
pub use write::{PlanError, Refusal, WritePlan};
