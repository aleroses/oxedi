//! `Envelope`, `WriteFinding` and `_write`: tables back into a file.
//!
//! `_write` converts its tables to the core's columns (the tables of a parse
//! are used as they are) and calls the core's writer with the GIL released.
//! Every refusal raises `oxedi.WriteError`, defined in the Python package
//! with the findings as an attribute; `oxedi.write` wraps `_write`.

use std::sync::Arc;

use oxedi_core::write::{Envelope, Finding, ValueOrigin, WriteError, write_with_findings};
use oxedi_core::{Builtin, Spec, Tables};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyList, PyMapping, PyTuple};
use pyo3_stub_gen::derive::{gen_stub_pyclass, gen_stub_pyfunction, gen_stub_pymethods};

use crate::diagnostic::PyDiagnostic;
use crate::document::PyDelimiters;
use crate::import;
use crate::spec::{self, PySpec};
use crate::stamp;
use crate::tables::PyTables;

/// Who sends and receives an interchange, when, its first control number
/// and its delimiters. The writer derives the rest of the envelope: counts,
/// later control numbers, codes and fixed widths.
#[gen_stub_pyclass(module = "oxedi._core")]
#[pyclass(name = "Envelope", module = "oxedi", frozen)]
pub struct PyEnvelope {
    inner: Envelope,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyEnvelope {
    /// `date` (a `datetime.date`) and `time` (a `datetime.time` without a
    /// time zone, whole seconds) stamp the interchange and the group: the
    /// interchange header holds the date without its century and the time
    /// without its seconds, the group header keeps both. `delimiters`
    /// defaults to `*`, `:`, `~` and the repetition separator `^`;
    /// `line_break` adds a line break after each segment.
    #[new]
    #[pyo3(signature = (
        *,
        sender_id,
        receiver_id,
        date,
        time,
        sender_qualifier = Envelope::DEFAULT_QUALIFIER,
        receiver_qualifier = Envelope::DEFAULT_QUALIFIER,
        usage_indicator = Envelope::DEFAULT_USAGE_INDICATOR,
        control_number = Envelope::DEFAULT_CONTROL_NUMBER,
        application_sender = None,
        application_receiver = None,
        delimiters = None,
        line_break = false
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        sender_id: String,
        receiver_id: String,
        #[gen_stub(override_type(type_repr = "datetime.date", imports = ("datetime",)))]
        date: &Bound<'_, PyAny>,
        #[gen_stub(override_type(type_repr = "datetime.time", imports = ("datetime",)))]
        time: &Bound<'_, PyAny>,
        sender_qualifier: &str,
        receiver_qualifier: &str,
        usage_indicator: &str,
        control_number: u64,
        application_sender: Option<String>,
        application_receiver: Option<String>,
        delimiters: Option<&Bound<'_, PyDelimiters>>,
        #[gen_stub(override_type(type_repr = "builtins.bool", imports = ("builtins",)))]
        line_break: bool,
    ) -> PyResult<Self> {
        let base = Envelope::new(
            sender_qualifier,
            sender_id,
            receiver_qualifier,
            receiver_id,
            stamp::date_days(date)?,
            stamp::time_seconds(time)?,
        );
        let delimiters = delimiters.map_or(base.delimiters, |given| given.get().inner);
        let inner = Envelope {
            usage_indicator: usage_indicator.to_string(),
            control_number,
            application_sender,
            application_receiver,
            delimiters,
            line_break,
            ..base
        };
        Ok(Self { inner })
    }

    /// The qualifier of a sender or receiver id when none is given, the
    /// default of `sender_qualifier` and `receiver_qualifier`.
    #[classattr]
    #[pyo3(name = "DEFAULT_QUALIFIER")]
    fn default_qualifier() -> &'static str {
        Envelope::DEFAULT_QUALIFIER
    }

    /// The usage indicator when none is given, the default of
    /// `usage_indicator`: production data.
    #[classattr]
    #[pyo3(name = "DEFAULT_USAGE_INDICATOR")]
    fn default_usage_indicator() -> &'static str {
        Envelope::DEFAULT_USAGE_INDICATOR
    }

    /// The first control number when none is given, the default of
    /// `control_number`.
    #[classattr]
    #[pyo3(name = "DEFAULT_CONTROL_NUMBER")]
    fn default_control_number() -> u64 {
        Envelope::DEFAULT_CONTROL_NUMBER
    }

    /// The sender id.
    #[getter]
    fn sender_id(&self) -> &str {
        &self.inner.sender_id
    }

    /// The sender id's qualifier.
    #[getter]
    fn sender_qualifier(&self) -> &str {
        &self.inner.sender_qualifier
    }

    /// The receiver id.
    #[getter]
    fn receiver_id(&self) -> &str {
        &self.inner.receiver_id
    }

    /// The receiver id's qualifier.
    #[getter]
    fn receiver_qualifier(&self) -> &str {
        &self.inner.receiver_qualifier
    }

    /// The usage indicator, e.g. `P` (production) or `T` (test).
    #[getter]
    fn usage_indicator(&self) -> &str {
        &self.inner.usage_indicator
    }

    /// The first control number.
    #[getter]
    fn control_number(&self) -> u64 {
        self.inner.control_number
    }

    /// The date.
    #[getter]
    #[gen_stub(override_return_type(type_repr = "datetime.date", imports = ("datetime",)))]
    fn date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let datetime = py.import("datetime")?;
        let epoch = datetime.getattr("date")?.call1((1970, 1, 1))?;
        let offset = datetime
            .getattr("timedelta")?
            .call1((i64::from(self.inner.date),))?;
        epoch.add(offset)
    }

    /// The time.
    #[getter]
    #[gen_stub(override_return_type(type_repr = "datetime.time", imports = ("datetime",)))]
    fn time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let seconds = self.inner.time;
        py.import("datetime")?.getattr("time")?.call1((
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60,
        ))
    }

    /// The functional group's sender code, when it is not the sender id.
    #[getter]
    fn application_sender(&self) -> Option<&str> {
        self.inner.application_sender.as_deref()
    }

    /// The functional group's receiver code, when it is not the receiver id.
    #[getter]
    fn application_receiver(&self) -> Option<&str> {
        self.inner.application_receiver.as_deref()
    }

    /// Whether a line break follows each segment.
    #[getter]
    fn line_break(&self) -> bool {
        self.inner.line_break
    }

    /// The delimiters written.
    #[getter]
    fn delimiters(&self) -> PyDelimiters {
        PyDelimiters {
            inner: self.inner.delimiters,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "Envelope(sender_id={:?}, receiver_id={:?}, control_number={})",
            self.inner.sender_id, self.inner.receiver_id, self.inner.control_number
        )
    }
}

/// One reason tables do not make a valid file. A value, never raised.
#[gen_stub_pyclass(module = "oxedi._core")]
#[pyclass(name = "WriteFinding", module = "oxedi", frozen)]
pub struct PyWriteFinding {
    inner: Finding,
}

impl PyWriteFinding {
    /// Where the value at fault comes from.
    fn origin(&self) -> Option<&ValueOrigin> {
        match &self.inner {
            Finding::DelimiterInValue { origin, .. }
            | Finding::UnwrittenValue { origin, .. }
            | Finding::NotWritable { origin, .. } => Some(origin),
            Finding::ReadBack { origin, .. } => origin.as_ref(),
            _ => None,
        }
    }
}

#[gen_stub_pymethods]
#[pymethods]
impl PyWriteFinding {
    /// The kind of finding, e.g. `MissingParent` or `ReadBack`.
    #[getter]
    fn kind(&self) -> &'static str {
        match &self.inner {
            Finding::DelimiterInValue { .. } => "DelimiterInValue",
            Finding::UnwrittenValue { .. } => "UnwrittenValue",
            Finding::MissingParent { .. } => "MissingParent",
            Finding::MismatchedReference { .. } => "MismatchedReference",
            Finding::DuplicateRowNumber { .. } => "DuplicateRowNumber",
            Finding::OutOfOrder { .. } => "OutOfOrder",
            Finding::NotWritable { .. } => "NotWritable",
            Finding::ReadBack { .. } => "ReadBack",
        }
    }

    /// The table at fault, when a table is.
    #[getter]
    fn table(&self) -> Option<&str> {
        match &self.inner {
            Finding::MissingParent { table, .. }
            | Finding::MismatchedReference { table, .. }
            | Finding::DuplicateRowNumber { table, .. }
            | Finding::OutOfOrder { table, .. } => Some(table),
            _ => match self.origin()? {
                ValueOrigin::Cell { table, .. } | ValueOrigin::Row { table, .. } => Some(table),
                ValueOrigin::Envelope { .. } => None,
            },
        }
    }

    /// The row at fault (its position in the table), when a row is.
    #[getter]
    fn row(&self) -> Option<usize> {
        match &self.inner {
            Finding::MissingParent { row, .. }
            | Finding::MismatchedReference { row, .. }
            | Finding::DuplicateRowNumber { row, .. }
            | Finding::OutOfOrder { row, .. } => Some(*row),
            _ => match self.origin()? {
                ValueOrigin::Cell { row, .. } | ValueOrigin::Row { row, .. } => Some(*row),
                ValueOrigin::Envelope { .. } => None,
            },
        }
    }

    /// The column at fault, when a column is.
    #[getter]
    fn column(&self) -> Option<&str> {
        match &self.inner {
            Finding::MissingParent { column, .. }
            | Finding::MismatchedReference { column, .. }
            | Finding::OutOfOrder { column, .. } => Some(column),
            _ => match self.origin()? {
                ValueOrigin::Cell { column, .. } => Some(column),
                _ => None,
            },
        }
    }

    /// The envelope field at fault, when one is.
    #[getter]
    fn field(&self) -> Option<&str> {
        match self.origin()? {
            ValueOrigin::Envelope { field } => Some(field),
            _ => None,
        }
    }

    /// The diagnostic of reading the written file back, for a `ReadBack`.
    #[getter]
    fn diagnostic(&self) -> Option<PyDiagnostic> {
        match &self.inner {
            Finding::ReadBack { diagnostic, .. } => Some(PyDiagnostic::from(diagnostic.clone())),
            _ => None,
        }
    }

    /// The finding as one sentence: the rule, where and the value.
    #[getter]
    fn message(&self) -> String {
        self.inner.to_string()
    }

    fn __str__(&self) -> String {
        self.inner.to_string()
    }

    fn __repr__(&self) -> String {
        format!("WriteFinding({:?})", self.inner.to_string())
    }
}

/// Raises `oxedi.WriteError` with `message` and `findings`.
fn refuse(py: Python<'_>, message: String, findings: Vec<Finding>) -> PyErr {
    let raised = || -> PyResult<PyErr> {
        let list = findings_list(py, findings)?;
        let class = py.import("oxedi._write")?.getattr("WriteError")?;
        Ok(PyErr::from_value(class.call1((message, list))?))
    };
    raised().unwrap_or_else(|err| err)
}

fn findings_list(py: Python<'_>, findings: Vec<Finding>) -> PyResult<Bound<'_, PyList>> {
    let list = PyList::empty(py);
    for inner in findings {
        list.append(Py::new(py, PyWriteFinding { inner })?)?;
    }
    Ok(list)
}

/// Writes tables as one interchange; internal, see `oxedi.write`. Returns
/// the bytes and the findings, which are empty unless `allow_findings`.
#[gen_stub_pyfunction]
#[pyfunction]
#[pyo3(name = "_write", signature = (tables, envelope, spec = None, allow_findings = false))]
#[gen_stub(override_return_type(type_repr = "tuple[bytes, builtins.list[WriteFinding]]", imports = ("builtins",)))]
pub fn write_tables<'py>(
    py: Python<'py>,
    #[gen_stub(override_type(type_repr = "Tables | collections.abc.Mapping[builtins.str, typing.Any]", imports = ("builtins", "collections.abc", "typing")))]
    tables: &Bound<'py, PyAny>,
    envelope: &Bound<'py, PyEnvelope>,
    spec: Option<&Bound<'py, PySpec>>,
    #[gen_stub(override_type(type_repr = "builtins.bool", imports = ("builtins",)))]
    allow_findings: bool,
) -> PyResult<Bound<'py, PyTuple>> {
    let (shared, spec) = resolve(py, tables, spec::given(spec))?;
    let envelope = envelope.get().inner.clone();
    let written = py.detach(|| write_with_findings(&spec, &shared, &envelope));
    finish(py, written, allow_findings)
}

/// The tables to write and the spec to write them with: those of a parse
/// (the given spec replacing its own), or a mapping read against the given
/// spec, else the fallback built-in.
fn resolve(
    py: Python<'_>,
    tables: &Bound<'_, PyAny>,
    given: Option<Arc<Spec>>,
) -> PyResult<(Arc<Tables>, Arc<Spec>)> {
    if let Ok(parsed) = tables.cast::<PyTables>() {
        let parsed = parsed.get();
        let spec = given.unwrap_or_else(|| Arc::clone(parsed.spec()));
        return Ok((Arc::clone(parsed.tables()), spec));
    }
    let mapping = tables.cast::<PyMapping>().map_err(|_| {
        let message = "write: tables must be the tables of a parse or a mapping of table names to Arrow, Polars or pandas tables";
        refuse(py, message.to_string(), Vec::new())
    })?;
    let spec = given.unwrap_or_else(|| Arc::clone(&Builtin::fallback().spec));
    let imported = import::tables(&spec, mapping)?;
    let tables = imported.map_err(|message| refuse(py, message, Vec::new()))?;
    Ok((Arc::new(tables), spec))
}

/// The written bytes with their findings as a tuple; a failed write, or
/// findings when they are not allowed, are refused with the findings.
fn finish<'py>(
    py: Python<'py>,
    written: Result<(Vec<u8>, Vec<Finding>), WriteError>,
    allow_findings: bool,
) -> PyResult<Bound<'py, PyTuple>> {
    let (bytes, findings) = written.map_err(|error| {
        let findings = match &error {
            WriteError::Findings(findings) => findings.clone(),
            _ => Vec::new(),
        };
        refuse(py, error.to_string(), findings)
    })?;
    if !allow_findings && !findings.is_empty() {
        let message = WriteError::Findings(findings.clone()).to_string();
        return Err(refuse(py, message, findings));
    }
    let list = findings_list(py, findings)?;
    PyTuple::new(py, [PyBytes::new(py, &bytes).into_any(), list.into_any()])
}

pyo3_stub_gen::export_verbatim!("oxedi._core", "_write");
