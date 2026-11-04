//! Loop specifications: the data that tells the engine how a file is structured.
//!
//! A spec is plain JSON: a map of loops, each naming its parent, the segment
//! (and optional element conditions) that opens it, the occurrences of
//! segments it may hold (named, with position, usage, maximum repeat,
//! qualifier and own code lists) and an optional segment that closes it. An
//! optional `segments` section names and types the elements of each segment
//! id, wherever the segment appears. An optional `balancing` section lists
//! amounts that must add up inside each instance of a loop. An optional
//! `summaries` section names figures computed over the tables, and an
//! optional `default_table` names the table a reader shows when none is
//! named. Loading compiles that into index-based definitions so the engine
//! never compares strings, and keeps the JSON value so patches can be applied
//! on top. Patches follow RFC 7386: objects merge key by key, while arrays and
//! scalars replace wholesale; see [`Spec::merge_patch`] for what that means
//! for a loop's occurrences.
//!
//! The module is split by responsibility: `loops`, `occurrences`, `segments`,
//! `tables`, `balance` and `summaries` hold the definitions,
//! `occurrence_error`, `table_error`, `balance_error` and `summary_error` why
//! an occurrence, a table, a balancing rule or a summary is rejected; `raw`
//! the deserialization shapes; `schema` the JSON shape as data and `shape`
//! the walk that checks a value against it; `build` the construction of a
//! [`Spec`] as a pipeline of steps, `build_loops` the compilation of one loop,
//! `compile` that of the tables and `columns` that of one table column;
//! `phi` the check of the columns flagged as patient data and of the seed
//! column; `error` the load error and `error_text` its `source` and the parts
//! of its text built from lists; `render` the text used in messages; `patch`
//! the merge patch; `version` the version declaration and the choice of a
//! spec by it; `builtin` the registry of shipped specs by transaction and
//! version.

mod balance;
mod balance_error;
mod build;
mod build_loops;
mod builtin;
mod columns;
mod compile;
mod error;
mod error_text;
mod loops;
mod occurrence_error;
mod occurrences;
mod patch;
mod phi;
mod raw;
mod render;
mod schema;
mod segments;
mod shape;
mod summaries;
mod summary_error;
mod table_error;
mod tables;
#[cfg(test)]
mod tests;
mod version;

pub use balance::{BalanceRule, BalanceTerm};
pub use balance_error::BalanceError;
pub use builtin::{Builtin, UnknownVersion, Version};
pub use error::SpecError;
pub use loops::{Control, ControlCount, ControlError, LoopDef, LoopId, Trigger};
pub use occurrence_error::OccurrenceError;
pub use occurrences::{OccurrenceDef, Qualifier, Usage};
pub use patch::merge_patch;
pub use phi::ColumnKind;
pub use segments::{ElementDef, ElementDefError, ROW_COLUMN, SEGMENT_COLUMN, SegmentDef};
pub use summaries::Summary;
pub use summary_error::SummaryDefError;
pub use table_error::{AnchorChains, AnchorPositions, LoopSegments, TableDefError};
pub use tables::{ColumnSource, Pick, Repeat, TableDef};
pub use version::{DeclaredVersion, VersionError};

pub(crate) use balance::render_terms;
pub(crate) use render::{render_key, render_selector, render_trigger};
pub(crate) use segments::rejects_code;

use std::cmp::Reverse;
use std::collections::BTreeMap;

use serde_json::Value;

use crate::segment::Segment;

/// A loaded, validated loop structure.
#[derive(Debug, Clone)]
pub struct Spec {
    name: String,
    loops: Vec<LoopDef>,
    roots: Vec<LoopId>,
    segments: BTreeMap<Vec<u8>, SegmentDef>,
    tables: Vec<TableDef>,
    balancing: Vec<BalanceRule>,
    summaries: Vec<(String, Summary)>,
    default_table: Option<String>,
    version: Option<DeclaredVersion>,
    source: Value,
}

impl Spec {
    /// The built-in 835 structure as shipped, in the same JSON a user would write.
    pub const BUILTIN_835_JSON: &'static str = include_str!("../../specs/835.json");

    /// The built-in 835 structure.
    pub fn builtin_835() -> Spec {
        Spec::from_json(Self::BUILTIN_835_JSON).expect(
            "the built-in 835 spec is valid; spec::tests::loading::builtin_835_loads checks it",
        )
    }

    /// The merge patch that turns the built-in 835 into its 4010 version.
    pub const BUILTIN_835_4010_PATCH: &'static str = include_str!("../../specs/835.4010.json");

    /// The built-in 835 for version 4010: [`Spec::builtin_835`] with
    /// [`Spec::BUILTIN_835_4010_PATCH`] merged over it.
    pub fn builtin_835_4010() -> Spec {
        Spec::builtin_835()
            .merge_patch(Self::BUILTIN_835_4010_PATCH)
            .expect(
                "the built-in 4010 patch is valid and merges into the built-in 835 as a valid spec",
            )
    }

    /// Loads and validates a spec from JSON text.
    pub fn from_json(json: &str) -> Result<Spec, SpecError> {
        let source: Value = serde_json::from_str(json).map_err(SpecError::Json)?;
        Spec::from_value(source)
    }

    /// Applies a JSON Merge Patch (RFC 7386) to this spec's JSON and loads the
    /// result: objects merge recursively, arrays and scalars are replaced,
    /// `null` deletes. The result is validated like any spec; every failure,
    /// including unparsable patch text, is wrapped in [`SpecError::Patch`].
    ///
    /// A loop's `occurrences` is an object keyed by occurrence name, so a
    /// patch adds, changes or removes (with `null`) one occurrence by its name
    /// and leaves the others as they are. Arrays, such as an element's
    /// `codes` or a qualifier's `codes`, are replaced whole. The current spec
    /// is visible through [`Self::to_json()`] or [`Self::get()`].
    ///
    /// # Example
    ///
    /// ```
    /// # use oxedi_core::Spec;
    /// let spec = Spec::builtin_835();
    /// let patched = spec.merge_patch(
    ///     r#"{"loops":{"1000A":{"occurrences":{"extra":{"segment":"XX","pos":11400}}}}}"#
    /// ).unwrap();
    /// let loop_1000a = patched.get(patched.loop_id("1000A").unwrap());
    /// assert!(loop_1000a.segments.contains(&b"XX".to_vec()));
    /// assert!(loop_1000a.occurrences.iter().any(|o| o.name == "extra"));
    /// ```
    pub fn merge_patch(&self, patch_json: &str) -> Result<Spec, SpecError> {
        let patched = serde_json::from_str::<Value>(patch_json)
            .map_err(SpecError::Json)
            .and_then(|patch| {
                let mut source = self.source.clone();
                merge_patch(&mut source, &patch);
                Spec::from_value(source)
            });
        patched.map_err(|e| SpecError::Patch {
            source: Box::new(e),
        })
    }

    /// The spec as JSON text, including any patches applied to it.
    pub fn to_json(&self) -> String {
        // A `Value` always serializes; a failure here would be a bug in serde_json.
        serde_json::to_string_pretty(&self.source).unwrap_or_default()
    }

    /// The spec's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Every loop, in spec order. Spec order is alphabetical by loop name
    /// (the JSON map is read into a `BTreeMap`), and it breaks ties between
    /// equally specific triggers.
    pub fn loops(&self) -> &[LoopDef] {
        &self.loops
    }

    /// The loop behind an id.
    pub fn get(&self, id: LoopId) -> &LoopDef {
        &self.loops[id.0]
    }

    /// The id of a loop by name.
    pub fn loop_id(&self, name: &str) -> Option<LoopId> {
        self.loops
            .iter()
            .position(|def| def.name == name)
            .map(LoopId)
    }

    /// The name of a loop by id.
    pub fn loop_name(&self, id: LoopId) -> &str {
        &self.loops[id.0].name
    }

    /// Top-level loops (no parent), in spec order.
    pub fn roots(&self) -> &[LoopId] {
        &self.roots
    }

    /// The element definitions of a segment id, if the spec has any.
    pub fn segment(&self, id: &[u8]) -> Option<&SegmentDef> {
        self.segments.get(id)
    }

    /// Every defined segment id with its definition, ordered by id.
    pub fn segments(&self) -> impl Iterator<Item = (&[u8], &SegmentDef)> {
        self.segments.iter().map(|(id, def)| (id.as_slice(), def))
    }

    /// The definition of an element, or of one of its components; `None`
    /// when the spec does not define it.
    pub fn element_def(
        &self,
        segment: &[u8],
        element: usize,
        component: Option<usize>,
    ) -> Option<&ElementDef> {
        occurrences::element_def(&self.segments, segment, element, component)
    }

    /// Whether any column of any table is flagged as patient data.
    pub fn masks_anything(&self) -> bool {
        self.tables
            .iter()
            .any(|table| table.phi.iter().any(Option::is_some))
    }

    /// Every table, in name order.
    pub fn tables(&self) -> &[TableDef] {
        &self.tables
    }

    /// The tables and loops, editable past the loader's checks, to build
    /// specs the loader refuses. It edits the loader's output without
    /// updating anything derived from it (loop children and segment lists,
    /// table parents and ancestors, the occurrence order): use it only for
    /// write plan tests, which read the edited parts directly.
    #[cfg(test)]
    pub(crate) fn parts_mut(&mut self) -> (&mut Vec<TableDef>, &mut Vec<LoopDef>) {
        (&mut self.tables, &mut self.loops)
    }

    /// A table by name.
    pub fn table(&self, name: &str) -> Option<&TableDef> {
        self.tables.iter().find(|table| table.name == name)
    }

    /// Every balancing rule, in name order.
    pub fn balancing(&self) -> &[BalanceRule] {
        &self.balancing
    }

    /// Every summary with its name, in name order.
    pub fn summaries(&self) -> &[(String, Summary)] {
        &self.summaries
    }

    /// The summary `name`, when the spec declares it.
    pub fn summary(&self, name: &str) -> Option<&Summary> {
        self.summaries
            .iter()
            .find_map(|(declared, summary)| (declared == name).then_some(summary))
    }

    /// The table a reader shows when the caller names none, when the spec
    /// declares one (`default_table`); it is always one of [`Self::tables`].
    pub fn default_table(&self) -> Option<&str> {
        self.default_table.as_deref()
    }

    /// Children of a loop, or the top-level loops for `None`.
    pub fn children(&self, parent: Option<LoopId>) -> &[LoopId] {
        match parent {
            Some(id) => &self.loops[id.0].children,
            None => &self.roots,
        }
    }

    /// Ancestors of a loop, root-most first, excluding the loop itself.
    pub fn ancestors(&self, id: LoopId) -> Vec<LoopId> {
        let mut chain = Vec::new();
        self.ancestors_into(id, &mut chain);
        chain
    }

    /// Like [`ancestors`](Spec::ancestors), writing the chain into `chain`
    /// after clearing it, so a caller can reuse one buffer.
    pub fn ancestors_into(&self, id: LoopId, chain: &mut Vec<LoopId>) {
        chain.clear();
        let mut current = self.loops[id.0].parent;
        while let Some(parent) = current {
            chain.push(parent);
            current = self.loops[parent.0].parent;
        }
        chain.reverse();
    }

    /// The child of `parent` that `segment` triggers, preferring the trigger
    /// with the most conditions; ties go to spec order.
    pub fn matching_child(&self, parent: Option<LoopId>, segment: &Segment<'_>) -> Option<LoopId> {
        self.best_match(self.children(parent).iter().copied(), segment)
    }

    /// Any loop that `segment` triggers, wherever it sits in the structure.
    pub fn matching_any(&self, segment: &Segment<'_>) -> Option<LoopId> {
        self.best_match((0..self.loops.len()).map(LoopId), segment)
    }

    fn best_match(
        &self,
        candidates: impl Iterator<Item = LoopId>,
        segment: &Segment<'_>,
    ) -> Option<LoopId> {
        candidates
            .filter(|&id| self.loops[id.0].trigger.matches(segment))
            .min_by_key(|&id| Reverse(self.loops[id.0].trigger.conditions.len()))
    }
}
