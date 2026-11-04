//! The deserialization shapes of a spec's JSON entries.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::de::IgnoredAny;

// The loop, segment and table sections are only checked for their outer shape
// here; each entry is then deserialized on its own, straight from the spec's
// JSON, so a schema error can name the entry it came from.
#[derive(Debug, Deserialize)]
#[serde(
    deny_unknown_fields,
    expecting = "a spec object with \"name\" and \"loops\""
)]
pub(super) struct RawSpec {
    pub(super) name: String,
    pub(super) loops: BTreeMap<String, IgnoredAny>,
    #[serde(default, rename = "segments")]
    pub(super) _segments: BTreeMap<String, IgnoredAny>,
    #[serde(default, rename = "tables")]
    pub(super) _tables: BTreeMap<String, IgnoredAny>,
    #[serde(default, rename = "balancing")]
    pub(super) _balancing: BTreeMap<String, IgnoredAny>,
    #[serde(default, rename = "summaries")]
    pub(super) _summaries: BTreeMap<String, IgnoredAny>,
    pub(super) default_table: Option<String>,
    pub(super) version: Option<RawVersion>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, expecting = "a version object")]
pub(super) struct RawVersion {
    pub(super) segment: String,
    pub(super) element: usize,
    pub(super) values: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, expecting = "a loop object")]
pub(super) struct RawLoop {
    pub(super) parent: Option<String>,
    pub(super) trigger: RawTrigger,
    #[serde(default)]
    pub(super) occurrences: BTreeMap<String, RawOccurrence>,
    pub(super) max: Option<usize>,
    pub(super) usage: Option<String>,
    pub(super) end: Option<String>,
    pub(super) control: Option<RawControl>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, expecting = "an occurrence object")]
pub(super) struct RawOccurrence {
    pub(super) segment: String,
    pub(super) pos: usize,
    pub(super) usage: Option<String>,
    pub(super) max: Option<usize>,
    pub(super) qualifier: Option<RawQualifier>,
    #[serde(default)]
    pub(super) codes: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, expecting = "a qualifier object")]
pub(super) struct RawQualifier {
    pub(super) element: usize,
    pub(super) component: Option<usize>,
    pub(super) codes: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, expecting = "a control object")]
pub(super) struct RawControl {
    pub(super) opener_element: usize,
    pub(super) closer_element: usize,
    pub(super) count_element: usize,
    pub(super) count: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, expecting = "a trigger object")]
pub(super) struct RawTrigger {
    pub(super) segment: String,
    #[serde(default, rename = "where")]
    pub(super) conditions: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, expecting = "a segment object")]
pub(super) struct RawSegment {
    #[serde(default)]
    pub(super) elements: BTreeMap<String, RawElement>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, expecting = "an element object")]
pub(super) struct RawElement {
    pub(super) name: String,
    #[serde(rename = "type")]
    pub(super) kind: String,
    #[serde(default)]
    pub(super) required: bool,
    pub(super) min: Option<usize>,
    pub(super) max: Option<usize>,
    pub(super) scale: Option<u8>,
    pub(super) codes: Option<Vec<String>>,
    #[serde(default)]
    pub(super) composite: BTreeMap<String, RawElement>,
}

// Columns are checked for their outer shape here and deserialized one by one,
// so a schema error can name the column.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, expecting = "a table object")]
pub(super) struct RawTable {
    pub(super) loops: Vec<String>,
    #[serde(rename = "ref")]
    pub(super) reference: Option<String>,
    pub(super) segment: Option<String>,
    pub(super) repeat: Option<RawRepeat>,
    #[serde(default)]
    pub(super) columns: BTreeMap<String, IgnoredAny>,
    pub(super) phi_seed: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, expecting = "a repeat object")]
pub(super) struct RawRepeat {
    pub(super) from: usize,
    pub(super) step: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, expecting = "a column object")]
pub(super) struct RawColumn {
    #[serde(rename = "loop")]
    pub(super) loop_name: Option<String>,
    pub(super) segment: Option<String>,
    #[serde(default, rename = "where")]
    pub(super) conditions: BTreeMap<String, String>,
    pub(super) occurrence: Option<String>,
    pub(super) pick: Option<RawPick>,
    pub(super) element: Option<usize>,
    pub(super) component: Option<usize>,
    pub(super) group_element: Option<usize>,
    #[serde(default)]
    pub(super) segment_index: bool,
    pub(super) phi: Option<String>,
}

// A pick is a name (`first`, `last`) or a 1-based position; which names are
// valid is checked when the column compiles, so the error can show the value.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged, expecting = "a pick")]
pub(super) enum RawPick {
    Nth(usize),
    Named(String),
}

impl RawPick {
    /// The pick as the spec writes it, in JSON.
    pub(super) fn written(&self) -> String {
        match self {
            RawPick::Nth(nth) => nth.to_string(),
            RawPick::Named(name) => serde_json::to_string(name).unwrap_or_default(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, expecting = "a balancing rule object")]
pub(super) struct RawBalance {
    pub(super) per: String,
    pub(super) target: Vec<RawTerm>,
    pub(super) sum: Vec<RawTerm>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, expecting = "a balancing value object")]
pub(super) struct RawTerm {
    #[serde(rename = "loop")]
    pub(super) loop_name: Option<String>,
    pub(super) occurrence: String,
    pub(super) elements: Vec<usize>,
    pub(super) component: Option<usize>,
    pub(super) sign: Option<String>,
}

// A summary holds exactly one operation; which ones it holds is checked when
// it compiles, so the error can list them.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, expecting = "a summary object")]
pub(super) struct RawSummary {
    pub(super) count: Option<RawSummaryTable>,
    pub(super) distinct: Option<RawSummaryColumn>,
    pub(super) sum: Option<RawSummaryColumn>,
    pub(super) row: Option<RawSummaryRow>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, expecting = "a summary operation object")]
pub(super) struct RawSummaryTable {
    pub(super) table: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, expecting = "a summary operation object")]
pub(super) struct RawSummaryColumn {
    pub(super) table: String,
    pub(super) column: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, expecting = "a summary operation object")]
pub(super) struct RawSummaryRow {
    pub(super) table: String,
    pub(super) fields: Vec<RawSummaryField>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, expecting = "a summary field object")]
pub(super) struct RawSummaryField {
    pub(super) name: String,
    pub(super) column: String,
}
