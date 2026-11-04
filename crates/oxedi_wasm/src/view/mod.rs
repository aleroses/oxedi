//! The viewer's picture of one file: segments with their byte ranges and
//! owning loop instance, the loop tree, the projected tables as text and
//! the diagnostics with their location.

use oxedi_core::{
    Builtin, Cell, DateMode, Diagnostic, Document, Event, LoopTree, Mask, NodeId, Processor, Spec,
    Tables,
};
use serde::Serialize;

#[cfg(test)]
mod tests;

/// The built-in spec of the version `document` declares, else the default.
pub(crate) fn select(document: &Document<'_>) -> &'static Spec {
    &Builtin::select(document.segments()).spec
}

/// One file as the viewer shows it.
#[derive(Debug, Serialize)]
pub struct View {
    /// The declared version the chosen spec covers, e.g. `005010X221A1`.
    pub spec_version: Option<String>,
    /// The file's separators, as text.
    pub delimiters: DelimitersView,
    /// Every segment, in file order.
    pub segments: Vec<SegmentView>,
    /// Loop instances; index 0 is the root.
    pub nodes: Vec<NodeView>,
    /// The spec's tables, ordered by name.
    pub tables: Vec<TableView>,
    /// Every finding, in the order the core reports them.
    pub diagnostics: Vec<DiagnosticView>,
}

/// The separators of a file. Each is one byte written as the `char` with
/// that code point (Latin-1), so `charCodeAt(0)` in JS gives the byte back;
/// encoding the string as UTF-8 does not for bytes from 0x80 up.
#[derive(Debug, Serialize)]
pub struct DelimitersView {
    /// Separates the elements of a segment.
    pub element: String,
    /// Separates the components of a composite element.
    pub component: String,
    /// Ends a segment.
    pub segment: String,
    /// Separates repeated values of an element; `None` when the file declares none.
    pub repetition: Option<String>,
}

/// One segment: its id, where its raw bytes live and the loop instance that holds it.
#[derive(Debug, Serialize)]
pub struct SegmentView {
    /// The segment id as text; empty for a segment with no content.
    pub id: String,
    /// First byte of the segment's raw range (trivia, body, terminator).
    pub start: usize,
    /// One past its last byte.
    pub end: usize,
    /// Index into `View::nodes` of the innermost loop instance open when
    /// the segment was read, including a segment with no content; 0 (the
    /// root) outside every loop.
    pub node: usize,
}

/// One loop instance.
#[derive(Debug, Serialize)]
pub struct NodeView {
    /// The loop's name as the spec writes it; `None` for the root.
    pub loop_name: Option<String>,
    /// 1 for the first instance of its loop in the file, 2 for the second…; 0 for the root.
    pub ordinal: usize,
    /// `true` when no segment of its own opened it.
    pub implicit: bool,
    /// Index into `View::nodes` of the enclosing instance; `None` for the root.
    pub parent: Option<usize>,
    /// Indices into `View::nodes` of the nested instances, in file order.
    pub children: Vec<usize>,
}

/// One projected table, column by column.
#[derive(Debug, Serialize)]
pub struct TableView {
    /// The table's name in the spec.
    pub name: String,
    /// Number of rows; every column holds this many values.
    pub rows: usize,
    /// The columns, in the spec's order.
    pub columns: Vec<ColumnView>,
}

/// One column: each row as text, `None` for a null.
#[derive(Debug, Serialize)]
pub struct ColumnView {
    /// The column's name in the spec.
    pub name: String,
    /// Each row as the core renders it: decimals in fixed point, dates as
    /// `YYYY-MM-DD`, times as `HH:MM:SS`, text with invalid UTF-8 replaced.
    pub values: Vec<Option<String>>,
}

/// One finding with its full text and where it points.
#[derive(Debug, Serialize)]
pub struct DiagnosticView {
    /// The finding's full text: rule, location and offending datum.
    pub text: String,
    /// `SNIP 1`, `SNIP 2` or `SNIP 3`.
    pub level: String,
    /// 0-based index into `View::segments` of the segment at fault; `None`
    /// at the end of the stream.
    pub segment: Option<usize>,
    /// 1-based element position, when the finding has one.
    pub element: Option<usize>,
    /// 1-based component position, when the finding has one.
    pub component: Option<usize>,
    /// Open loops, outermost first, as `(name, ordinal)`.
    pub path: Vec<(String, usize)>,
}

/// Parses `bytes` with the built-in spec of their declared version; the
/// error is the core's message for bytes that are not an X12 interchange.
pub fn view(bytes: &[u8]) -> Result<View, String> {
    view_with(bytes, None)
}

/// [`view`] with the columns the spec flags as patient data masked by
/// `mask_key` (dates reduced to the year) when a key is given; a key the
/// core refuses is returned as its message.
pub fn view_masked(bytes: &[u8], mask_key: Option<&[u8]>) -> Result<View, String> {
    let mask = mask_key
        .map(|key| Mask::new(key, DateMode::Year).map_err(|err| err.to_string()))
        .transpose()?;
    view_with(bytes, mask.as_ref())
}

fn view_with(bytes: &[u8], mask: Option<&Mask>) -> Result<View, String> {
    let document = Document::parse(bytes).map_err(|err| err.to_string())?;
    let spec = select(&document);
    let Walk {
        tables,
        diagnostics,
        tree,
        owners,
    } = walk(spec, &document, mask);
    let delimiters = document.delimiters();
    let text = |byte: u8| char::from(byte).to_string();
    Ok(View {
        spec_version: spec
            .version()
            .and_then(|version| version.values.first())
            .map(|value| String::from_utf8_lossy(value).into_owned()),
        delimiters: DelimitersView {
            element: text(delimiters.element),
            component: text(delimiters.component),
            segment: text(delimiters.segment),
            repetition: delimiters.repetition.map(text),
        },
        segments: segments(&document, &owners),
        nodes: nodes(spec, &tree),
        tables: tables_view(&tables),
        diagnostics: diagnostics.iter().map(diagnostic).collect(),
    })
}

/// [`view_masked`] as the JSON the page reads.
pub fn to_json(bytes: &[u8], mask_key: Option<&[u8]>) -> Result<String, String> {
    serde_json::to_string(&view_masked(bytes, mask_key)?).map_err(|err| err.to_string())
}

/// What one pass of the processor over a document leaves.
struct Walk {
    tables: Tables,
    diagnostics: Vec<Diagnostic>,
    tree: LoopTree,
    /// For each segment, the index of the tree node open when it was read.
    owners: Vec<usize>,
}

/// Feeds every segment to one processor, folding its events into the loop
/// tree and following the open instances, so each segment, a segment with
/// no content included, gets the innermost instance open when it was read.
fn walk(spec: &Spec, document: &Document<'_>, mask: Option<&Mask>) -> Walk {
    let mut processor = Processor::new(spec, document.delimiters()).with_mask(mask);
    let mut builder = LoopTree::builder();
    let mut owners = Owners::new(document.len());
    let mut diagnostics = Vec::new();
    for segment in document.segments() {
        let output = processor.feed(&segment);
        owners.on(output.events());
        builder.on(output.events());
        diagnostics.extend_from_slice(output.diagnostics());
    }
    let output = processor.finish();
    owners.on(output.events());
    builder.on(output.events());
    diagnostics.extend_from_slice(output.diagnostics());
    Walk {
        tables: processor.take_tables(),
        diagnostics,
        tree: builder.finish(),
        owners: owners.finish(),
    }
}

/// The stack of open tree nodes, numbered as [`oxedi_core::TreeBuilder`] numbers them
/// (in the order loops open, the root being 0), and the node each segment
/// was first reported in.
struct Owners {
    open: Vec<usize>,
    next: usize,
    owner: Vec<Option<usize>>,
}

impl Owners {
    fn new(segments: usize) -> Owners {
        Owners {
            open: vec![0],
            next: 1,
            owner: vec![None; segments],
        }
    }

    fn on(&mut self, events: &[Event]) {
        for event in events {
            match *event {
                Event::LoopOpened { .. } => {
                    self.open.push(self.next);
                    self.next = self.next.saturating_add(1);
                }
                // The root never closes.
                Event::LoopClosed { .. } => {
                    if self.open.len() > 1 {
                        self.open.pop();
                    }
                }
                Event::Captured { segment, .. }
                | Event::Unmatched { segment }
                | Event::Empty { segment } => {
                    if let Some(slot @ None) = self.owner.get_mut(segment) {
                        *slot = self.open.last().copied();
                    }
                }
            }
        }
    }

    fn finish(self) -> Vec<usize> {
        self.owner
            .into_iter()
            .map(|node| node.unwrap_or(0))
            .collect()
    }
}

fn segments(document: &Document<'_>, owners: &[usize]) -> Vec<SegmentView> {
    document
        .spans()
        .zip(owners.iter().copied())
        .enumerate()
        .map(|(index, (span, node))| SegmentView {
            id: String::from_utf8_lossy(document.segment_id(index).unwrap_or_default())
                .into_owned(),
            start: span.raw.start,
            end: span.raw.end,
            node,
        })
        .collect()
}

fn nodes(spec: &Spec, tree: &LoopTree) -> Vec<NodeView> {
    let mut seen = vec![0usize; spec.loops().len()];
    tree.nodes()
        .iter()
        .map(|node| {
            let (loop_name, ordinal) = match node.loop_id {
                Some(id) => {
                    let ordinal = seen.get_mut(id.index()).map_or(0, |count| {
                        *count = count.saturating_add(1);
                        *count
                    });
                    (Some(spec.loop_name(id).to_string()), ordinal)
                }
                None => (None, 0),
            };
            NodeView {
                loop_name,
                ordinal,
                implicit: node.implicit,
                parent: node.parent.map(NodeId::index),
                children: node.children.iter().map(|child| child.index()).collect(),
            }
        })
        .collect()
}

fn tables_view(tables: &Tables) -> Vec<TableView> {
    tables
        .iter()
        .map(|table| TableView {
            name: table.name().to_string(),
            rows: table.len(),
            columns: table
                .columns()
                .iter()
                .map(|(name, data)| ColumnView {
                    name: name.clone(),
                    values: (0..data.len())
                        .map(|row| match data.get(row) {
                            Some(Cell::Null) | None => None,
                            Some(_) => data.render(row),
                        })
                        .collect(),
                })
                .collect(),
        })
        .collect()
}

fn diagnostic(diagnostic: &Diagnostic) -> DiagnosticView {
    DiagnosticView {
        text: diagnostic.to_string(),
        level: diagnostic.level.to_string(),
        segment: diagnostic.segment,
        element: diagnostic.element,
        component: diagnostic.component,
        path: diagnostic
            .path
            .iter()
            .map(|loop_ref| (loop_ref.name.clone(), loop_ref.ordinal))
            .collect(),
    }
}
