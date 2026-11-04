//! `Diagnostic`: one finding about a file's data, as Python attributes.

use oxedi_core::{Diagnostic, LoopRef, Projector, Rule, SnipLevel};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyInt, PyList};
use pyo3_stub_gen::derive::{gen_stub_pyclass, gen_stub_pyfunction, gen_stub_pymethods};
use pyo3_stub_gen::type_info::{ParameterDefault, ParameterInfo, ParameterKind, PyFunctionInfo};
use pyo3_stub_gen::{PyStubType, TypeInfo};

use crate::document::PyDocument;
use crate::mask::PyMask;
use crate::spec::PySpec;

/// One finding about the data of a file. A value, never raised.
#[gen_stub_pyclass(module = "oxedi._core")]
#[pyclass(name = "Diagnostic", module = "oxedi", frozen)]
pub struct PyDiagnostic {
    inner: Diagnostic,
}

impl From<Diagnostic> for PyDiagnostic {
    fn from(inner: Diagnostic) -> Self {
        Self { inner }
    }
}

fn position(value: Option<usize>) -> String {
    value.map_or_else(|| "None".to_string(), |value| value.to_string())
}

#[gen_stub_pymethods]
#[pymethods]
impl PyDiagnostic {
    /// The SNIP level of the rule: 1, 2 or 3.
    #[getter]
    fn level(&self) -> u8 {
        self.inner.level.number()
    }

    /// Who reported the diagnostic: `"oxedi"` for the parser's own rules,
    /// the validator's name for an external one.
    #[getter]
    fn origin(&self) -> &str {
        self.inner.origin()
    }

    /// The external validator's own code for the finding; `None` for the
    /// parser's own rules and for external findings without one.
    #[getter]
    fn code(&self) -> Option<&str> {
        self.inner.code()
    }

    /// The name of the rule that failed, e.g. `RequiredElementMissing`.
    #[getter]
    fn kind(&self) -> &'static str {
        self.inner.rule.kind()
    }

    /// The rule that failed, as a sentence with its values.
    #[getter]
    fn rule(&self) -> String {
        self.inner.rule.to_string()
    }

    /// The index of the segment at fault; `None` at the end of the stream.
    #[getter]
    fn segment(&self) -> Option<usize> {
        self.inner.segment
    }

    /// The 1-based element position, when the diagnostic has one.
    #[getter]
    fn element(&self) -> Option<usize> {
        self.inner.element
    }

    /// The 1-based component position, when the diagnostic has one.
    #[getter]
    fn component(&self) -> Option<usize> {
        self.inner.component
    }

    /// The open loops, outermost first, joined by `/`; empty at the root.
    #[getter]
    fn path(&self) -> String {
        self.inner.path_text()
    }

    /// The offending value as it appears in the file.
    #[getter]
    fn datum<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.inner.datum)
    }

    fn __str__(&self) -> String {
        self.inner.to_string()
    }

    fn __repr__(&self) -> String {
        format!(
            "Diagnostic(level={}, origin='{}', kind='{}', segment={}, element={}, component={})",
            self.level(),
            self.origin(),
            self.kind(),
            position(self.inner.segment),
            position(self.inner.element),
            position(self.inner.component)
        )
    }
}

/// A position argument as an index. Any Python integer outside `usize` is a
/// `ValueError` naming the argument and the value; other types keep Python's
/// own `TypeError`.
fn position_arg(name: &str, value: Option<&Bound<'_, PyAny>>) -> PyResult<Option<usize>> {
    let Some(value) = value.filter(|value| !value.is_none()) else {
        return Ok(None);
    };
    match value.extract::<usize>() {
        Ok(index) => Ok(Some(index)),
        Err(_) if value.is_instance_of::<PyInt>() => Err(PyValueError::new_err(format!(
            "{name} must be a non-negative integer, got {}",
            value.str()?
        ))),
        Err(err) => Err(err),
    }
}

/// Builds a `Diagnostic` for a finding reported by an external validator.
///
/// Internal: adapters to external validators call it; it is not part of the
/// public API. `path` holds the open loops as `(name, ordinal)` pairs,
/// outermost first, as `_loop_paths` returns them; `level` must be 1, 2 or 3.
// `text_signature` mirrors `signature =`, as the stub description below does.
#[pyfunction]
#[pyo3(
    name = "_external_diagnostic",
    signature = (origin, message, level, code=None, segment=None, element=None, component=None, datum=b"".to_vec(), path=None),
    text_signature = "(origin, message, level, code=None, segment=None, element=None, component=None, datum=b'', path=None)"
)]
#[allow(clippy::too_many_arguments)]
pub fn external_diagnostic(
    origin: String,
    message: String,
    level: &Bound<'_, PyAny>,
    code: Option<String>,
    segment: Option<&Bound<'_, PyAny>>,
    element: Option<&Bound<'_, PyAny>>,
    component: Option<&Bound<'_, PyAny>>,
    datum: Vec<u8>,
    path: Option<Vec<(String, usize)>>,
) -> PyResult<PyDiagnostic> {
    let segment = position_arg("segment", segment)?;
    let element = position_arg("element", element)?;
    let component = position_arg("component", component)?;
    let level = match level.extract::<i64>() {
        Ok(level) => level,
        Err(_) if level.is_instance_of::<PyInt>() => {
            return Err(PyValueError::new_err(format!(
                "level must be 1, 2 or 3, got {}",
                level.str()?
            )));
        }
        Err(err) => return Err(err),
    };
    let level = match level {
        1 => SnipLevel::L1,
        2 => SnipLevel::L2,
        3 => SnipLevel::L3,
        _ => {
            return Err(PyValueError::new_err(format!(
                "level must be 1, 2 or 3, got {level}"
            )));
        }
    };
    let rule = Rule::External {
        origin,
        code,
        message,
        level,
    };
    let path = path
        .unwrap_or_default()
        .into_iter()
        .map(|(name, ordinal)| LoopRef { name, ordinal })
        .collect();
    Ok(PyDiagnostic::from(Diagnostic::new(
        rule, segment, element, component, path, datum,
    )))
}

/// A parameter of `_external_diagnostic` with no default.
const fn required(name: &'static str, type_info: fn() -> TypeInfo) -> ParameterInfo {
    ParameterInfo {
        name,
        kind: ParameterKind::PositionalOrKeyword,
        type_info,
        default: ParameterDefault::None,
    }
}

/// A parameter of `_external_diagnostic` defaulting to `default`, written as
/// a Python literal.
const fn defaulted(
    name: &'static str,
    type_info: fn() -> TypeInfo,
    default: fn() -> String,
) -> ParameterInfo {
    ParameterInfo {
        name,
        kind: ParameterKind::PositionalOrKeyword,
        type_info,
        default: ParameterDefault::Expr {
            value: default,
            source_module: None,
        },
    }
}

fn none() -> String {
    "None".to_string()
}

// `_external_diagnostic` is described by hand: its positions are read from
// any Python object to name the argument in the error, and `datum` defaults
// to an empty byte string, which the derive macros cannot write in Python.
// It mirrors `signature =` on `external_diagnostic`.
pyo3_stub_gen::inventory::submit! {
    PyFunctionInfo {
        name: "_external_diagnostic",
        parameters: &[
            required("origin", <String as PyStubType>::type_input),
            required("message", <String as PyStubType>::type_input),
            required("level", <i64 as PyStubType>::type_input),
            defaulted("code", <Option<String> as PyStubType>::type_input, none),
            defaulted("segment", <Option<usize> as PyStubType>::type_input, none),
            defaulted("element", <Option<usize> as PyStubType>::type_input, none),
            defaulted("component", <Option<usize> as PyStubType>::type_input, none),
            defaulted("datum", <Py<PyBytes> as PyStubType>::type_input, || "b\"\"".to_string()),
            defaulted("path", <Option<Vec<(String, usize)>> as PyStubType>::type_input, none),
        ],
        r#return: <PyDiagnostic as PyStubType>::type_output,
        doc: "Builds a `Diagnostic` for a finding reported by an external validator; internal.",
        module: Some("oxedi._core"),
        is_async: false,
        deprecated: None,
        type_ignored: None,
        is_overload: false,
        file: file!(),
        line: line!(),
        column: column!(),
        index: 0,
    }
}

pyo3_stub_gen::export_verbatim!("oxedi._core", "_external_diagnostic");

/// Copies of `diagnostics`, findings an external validator raised about
/// `document`, with the datum of each one at a place a column `spec` flags
/// as patient data masked by `mask`, as `parse` masks its own diagnostics,
/// and, in the message, the original datum and each flagged value of the
/// finding's segment replaced by its masked form. The inputs are left as
/// they are. A finding whose position in `own_text` is true is one the
/// adapter wrote itself: its datum is masked but its message is left as
/// written.
///
/// Internal: adapters to external validators call it; it is not part of the
/// public API.
#[gen_stub_pyfunction]
#[pyfunction]
#[pyo3(name = "_mask_findings", signature = (diagnostics, document, mask, spec, own_text=None))]
pub fn mask_findings(
    diagnostics: Vec<PyRef<'_, PyDiagnostic>>,
    document: &Bound<'_, PyDocument>,
    mask: &Bound<'_, PyMask>,
    spec: &Bound<'_, PySpec>,
    own_text: Option<Vec<bool>>,
) -> Vec<PyDiagnostic> {
    let document = &document.get().inner;
    let spec = &spec.get().inner;
    let mut found: Vec<Diagnostic> = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.inner.clone())
        .collect();
    // The adapter wrote these messages itself; they hold no value of the file
    // to replace, so they are set aside while the rest is masked and put back.
    let own_text = own_text.unwrap_or_default();
    let mut kept: Vec<Option<String>> = Vec::with_capacity(found.len());
    for (position, diagnostic) in found.iter_mut().enumerate() {
        let own = own_text.get(position).copied().unwrap_or(false);
        kept.push(match &mut diagnostic.rule {
            Rule::External { message, .. } if own => Some(std::mem::take(message)),
            _ => None,
        });
    }
    Projector::new(spec, document.delimiters())
        .with_mask(&mask.get().inner)
        .mask_found(&mut found, document);
    for (diagnostic, original) in found.iter_mut().zip(kept) {
        if let (Rule::External { message, .. }, Some(original)) = (&mut diagnostic.rule, original) {
            *message = original;
        }
    }
    found.into_iter().map(PyDiagnostic::from).collect()
}

pyo3_stub_gen::export_verbatim!("oxedi._core", "_mask_findings");

/// The diagnostics as a Python list of `Diagnostic`.
pub fn to_list(py: Python<'_>, diagnostics: Vec<Diagnostic>) -> PyResult<Bound<'_, PyList>> {
    let list = PyList::empty(py);
    for diagnostic in diagnostics {
        list.append(Py::new(py, PyDiagnostic::from(diagnostic))?)?;
    }
    Ok(list)
}
