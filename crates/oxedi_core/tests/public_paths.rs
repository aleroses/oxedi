// Each `use` names a public item by its module path; the build fails if one disappears.
#![allow(unused_imports)]

mod by_module {
    use oxedi_core::check::StructureChecker;
    use oxedi_core::column::Bitmap;
    use oxedi_core::column::Cell;
    use oxedi_core::column::CellError;
    use oxedi_core::column::Column;
    use oxedi_core::column::ColumnData;
    use oxedi_core::column::RowError;
    use oxedi_core::column::Table;
    use oxedi_core::column::Tables;
    use oxedi_core::delimiters::Delimiters;
    use oxedi_core::delimiters::IsaError;
    use oxedi_core::diagnostic::Diagnostic;
    use oxedi_core::diagnostic::FlatDiagnostic;
    use oxedi_core::diagnostic::LoopRef;
    use oxedi_core::diagnostic::PARSER_ORIGIN;
    use oxedi_core::diagnostic::Rule;
    use oxedi_core::diagnostic::SnipLevel;
    use oxedi_core::document::Document;
    use oxedi_core::document::DocumentError;
    use oxedi_core::document::Segments;
    use oxedi_core::document::SizeError;
    use oxedi_core::document::Span;
    use oxedi_core::document::Spans;
    use oxedi_core::element::Element;
    use oxedi_core::element::Value;
    use oxedi_core::element::split_raw;
    use oxedi_core::element::unescape;
    use oxedi_core::engine::Event;
    use oxedi_core::engine::LoopEngine;
    use oxedi_core::frame::Frame;
    use oxedi_core::frame::find_unescaped;
    use oxedi_core::frame::first_frame;
    use oxedi_core::frame::next_frame;
    use oxedi_core::process::Output;
    use oxedi_core::process::Processor;
    use oxedi_core::project::ColumnSchema;
    use oxedi_core::project::Projector;
    use oxedi_core::project::TableSchema;
    use oxedi_core::project::spec_schema;
    use oxedi_core::project::table_columns;
    use oxedi_core::project::table_schema;
    use oxedi_core::segment::Segment;
    use oxedi_core::segment::SegmentWriteError;
    use oxedi_core::spec::AnchorChains;
    use oxedi_core::spec::AnchorPositions;
    use oxedi_core::spec::Builtin;
    use oxedi_core::spec::ColumnSource;
    use oxedi_core::spec::Control;
    use oxedi_core::spec::ControlCount;
    use oxedi_core::spec::ControlError;
    use oxedi_core::spec::DeclaredVersion;
    use oxedi_core::spec::ElementDef;
    use oxedi_core::spec::ElementDefError;
    use oxedi_core::spec::LoopDef;
    use oxedi_core::spec::LoopId;
    use oxedi_core::spec::LoopSegments;
    use oxedi_core::spec::Pick;
    use oxedi_core::spec::ROW_COLUMN;
    use oxedi_core::spec::Repeat;
    use oxedi_core::spec::SEGMENT_COLUMN;
    use oxedi_core::spec::SegmentDef;
    use oxedi_core::spec::Spec;
    use oxedi_core::spec::SpecError;
    use oxedi_core::spec::Summary;
    use oxedi_core::spec::SummaryDefError;
    use oxedi_core::spec::TableDef;
    use oxedi_core::spec::TableDefError;
    use oxedi_core::spec::Trigger;
    use oxedi_core::spec::UnknownVersion;
    use oxedi_core::spec::Version;
    use oxedi_core::spec::VersionError;
    use oxedi_core::spec::merge_patch;
    use oxedi_core::summary::SummaryError;
    use oxedi_core::summary::SummaryValue;
    use oxedi_core::summary::summarize;
    use oxedi_core::tokenizer::Tokenizer;
    use oxedi_core::tree::LoopTree;
    use oxedi_core::tree::Node;
    use oxedi_core::tree::NodeId;
    use oxedi_core::tree::TreeBuilder;
    use oxedi_core::trivia::BYTE_ORDER_MARK;
    use oxedi_core::trivia::LeadingTrivia;
    use oxedi_core::trivia::is_trivia;
    use oxedi_core::value::ColumnType;
    use oxedi_core::value::ConvertError;
    use oxedi_core::value::DECIMAL_PRECISION;
    use oxedi_core::value::ElementTextError;
    use oxedi_core::value::ElementType;
    use oxedi_core::value::LengthRange;
    use oxedi_core::value::MAX_SCALE;
    use oxedi_core::value::Parsed;
    use oxedi_core::value::TimeUnit;
    use oxedi_core::value::TypeCodeError;
    use oxedi_core::value::date_display;
    use oxedi_core::value::date_element;
    use oxedi_core::value::date_from_midnight;
    use oxedi_core::value::date_from_millis;
    use oxedi_core::value::days_from_civil;
    use oxedi_core::value::decimal_display;
    use oxedi_core::value::decimal_element;
    use oxedi_core::value::integer_from_f64;
    use oxedi_core::value::integer_from_i128;
    use oxedi_core::value::integer_from_u128;
    use oxedi_core::value::parse_dt;
    use oxedi_core::value::parse_n;
    use oxedi_core::value::parse_r;
    use oxedi_core::value::parse_tm;
    use oxedi_core::value::rescale;
    use oxedi_core::value::seconds_from;
    use oxedi_core::value::seconds_of_day;
    use oxedi_core::value::time_display;
    use oxedi_core::value::time_element;
    use oxedi_core::value::time_of_day;
}

// Root re-exports from lib.rs
use oxedi_core::ConvertError;
use oxedi_core::Projector;
use oxedi_core::StructureChecker;
use oxedi_core::Tokenizer;
use oxedi_core::{
    AnchorChains, AnchorPositions, Builtin, ColumnSource, Control, ControlCount, ControlError,
    DeclaredVersion, ElementDef, ElementDefError, ElementType, LoopDef, LoopId, LoopSegments, Pick,
    Repeat, SegmentDef, Spec, SpecError, TableDef, TableDefError, Trigger, UnknownVersion, Version,
    VersionError, merge_patch,
};
use oxedi_core::{
    Bitmap, Cell, CellError, Column, ColumnData, ColumnType, RowError, Table, Tables,
};
use oxedi_core::{ColumnSchema, TableSchema, spec_schema};
use oxedi_core::{Delimiters, IsaError};
use oxedi_core::{Diagnostic, FlatDiagnostic, LoopRef, PARSER_ORIGIN, Rule, SnipLevel};
use oxedi_core::{Document, DocumentError, Segments, SizeError, Span, Spans};
use oxedi_core::{Element, Value};
use oxedi_core::{Event, LoopEngine};
use oxedi_core::{Frame, next_frame};
use oxedi_core::{LoopTree, Node, NodeId, TreeBuilder};
use oxedi_core::{Output, Processor};
use oxedi_core::{Segment, SegmentWriteError};
use oxedi_core::{Summary, SummaryDefError, SummaryError, SummaryValue, summarize};
use oxedi_core::{parse_dt, parse_n, parse_r, parse_tm};

#[test]
fn every_public_path_resolves() {}

// Associated items added to `Spec`, named so the build fails if one disappears.
#[allow(dead_code)]
fn spec_items(spec: &Spec) {
    let _: &str = Spec::BUILTIN_835_4010_PATCH;
    let _: fn() -> Spec = Spec::builtin_835_4010;
    let _: for<'a> fn(&'a Spec) -> Option<&'a DeclaredVersion> = Spec::version;
    let _: &Spec = Spec::select(&[spec], spec, Vec::<Segment<'static>>::new());
    let _: &[(String, Summary)] = spec.summaries();
    let _: Option<&Summary> = spec.summary("count_claims");
    let _: Option<&str> = spec.default_table();
}

// Associated items of `Builtin`, named so the build fails if one disappears.
#[allow(dead_code)]
fn builtin_items() {
    let _: Version = Builtin::DEFAULT_VERSION;
    let _: fn() -> &'static [Builtin] = Builtin::all;
    let _: fn() -> &'static Builtin = Builtin::fallback;
    let _: fn(&str, Version) -> Result<&'static Builtin, UnknownVersion> = Builtin::find;
    let _: fn(&str, &str) -> Result<&'static Builtin, UnknownVersion> = Builtin::find_text;
    let _: fn(&str) -> Vec<Version> = Builtin::versions;
    let _: [Version; 2] = Version::ALL;
    let _: fn(Version) -> &'static str = Version::text;
    let _: fn(&str) -> Option<Version> = Version::parse;
    let _: &Builtin = Builtin::select(Vec::<Segment<'static>>::new());
}

// Associated items the bindings read their defaults, names and units from,
// named so the build fails if one disappears.
#[allow(dead_code)]
fn binding_items(diagnostic: &Diagnostic) {
    use oxedi_core::DateMode;
    use oxedi_core::value::TimeUnit;
    use oxedi_core::write::Envelope;
    let _: Envelope = Envelope::with_ids("SENDER", "RECEIVER", 0, 0);
    let _: &str = Envelope::DEFAULT_QUALIFIER;
    let _: &str = Envelope::DEFAULT_USAGE_INDICATOR;
    let _: u64 = Envelope::DEFAULT_CONTROL_NUMBER;
    let _: [&str; 10] = FlatDiagnostic::FIELDS;
    let _: FlatDiagnostic<'_> = diagnostic.flat();
    let _: for<'a> fn(&'a Diagnostic) -> &'a str = Diagnostic::origin;
    let _: for<'a> fn(&'a Diagnostic) -> Option<&'a str> = Diagnostic::code;
    let _: fn(&Diagnostic) -> String = Diagnostic::path_text;
    let _: fn(SnipLevel) -> u8 = SnipLevel::number;
    let _: fn(DateMode) -> &'static str = DateMode::word;
    let _: [TimeUnit; 4] = [
        TimeUnit::Second,
        TimeUnit::Millisecond,
        TimeUnit::Microsecond,
        TimeUnit::Nanosecond,
    ];
    let _: fn(TimeUnit) -> i64 = TimeUnit::ticks_per_second;
    let _: fn(TimeUnit) -> i64 = TimeUnit::ticks_per_day;
    let _: usize = IsaError::ISA_LEN;
    let _: fn(&IsaError) -> usize = IsaError::skipped;
    let _: for<'a> fn(&'a IsaError, &'a [u8]) -> &'a [u8] = IsaError::datum;
    let _: for<'a> fn(&'a Segment<'static>, usize, usize) -> Option<&'a [u8]> = Segment::component;
}
