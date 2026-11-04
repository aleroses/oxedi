//! `Spec`: the loop structure, element definitions and tables, as data.

use std::sync::Arc;

use oxedi_core::{Builtin, Segment, Spec, Version};
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyCapsule, PyDict, PyString};

use pyo3_stub_gen::derive::{gen_stub_pyclass, gen_stub_pymethods};
use pyo3_stub_gen::{PyStubType, TypeInfo};

use crate::arrow;
use crate::tables::extra;

native_exception!(
    SpecError,
    PyValueError,
    "A spec that cannot be loaded or patched; the message names the rule, the loop and key, and the datum."
);

/// One table's schema, offered to `pyarrow.schema` through the Arrow
/// PyCapsule interface; it is not part of the module's public names.
#[pyclass(frozen)]
struct SchemaExport {
    name: String,
    schema: arrow_schema::Schema,
}

#[pymethods]
impl SchemaExport {
    fn __arrow_c_schema__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyCapsule>> {
        let schema = arrow::export_schema(&self.name, &self.schema)?;
        PyCapsule::new_with_value(py, schema, c"arrow_schema")
    }
}

/// The spec given to a call, which it uses as is.
pub fn given(spec: Option<&Bound<'_, PySpec>>) -> Option<Arc<Spec>> {
    spec.map(|spec| spec.get().inner.clone())
}

/// The spec a call uses: the one given, or else the built-in of the version
/// the segments declare, and the default built-in when they declare none of
/// theirs. Reading stops once the version is settled.
pub fn given_or_selected<'s>(
    given: Option<Arc<Spec>>,
    segments: impl IntoIterator<Item = Segment<'s>>,
) -> Arc<Spec> {
    if let Some(spec) = given {
        return spec;
    }
    Arc::clone(&Builtin::select(segments).spec)
}

/// An X12 release a built-in spec covers.
#[gen_stub_pyclass(module = "oxedi._core")]
#[pyclass(name = "Version", module = "oxedi", eq, frozen, hash, from_py_object)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PyVersion(Version);

impl From<Version> for PyVersion {
    fn from(version: Version) -> PyVersion {
        PyVersion(version)
    }
}

#[gen_stub_pymethods]
#[pymethods]
impl PyVersion {
    /// Release 5010.
    #[classattr]
    #[pyo3(name = "V5010")]
    fn v5010() -> PyVersion {
        Version::V5010.into()
    }

    /// Release 4010.
    #[classattr]
    #[pyo3(name = "V4010")]
    fn v4010() -> PyVersion {
        Version::V4010.into()
    }

    /// The release number as text.
    #[getter]
    fn text(&self) -> &'static str {
        self.0.text()
    }

    fn __str__(&self) -> &'static str {
        self.text()
    }

    fn __repr__(&self) -> String {
        format!("Version.V{}", self.0.text())
    }

    /// Every version a built-in spec covers, the default first.
    #[staticmethod]
    fn all() -> Vec<PyVersion> {
        Version::ALL.into_iter().map(PyVersion::from).collect()
    }
}

/// A version argument: a `Version`, its text, or anything else (kept as its
/// repr and type name, so the call that received it can refuse it).
pub enum VersionArg {
    Version(Version),
    Text(String),
    Other { shown: String, kind: String },
}

impl VersionArg {
    /// The version `Spec.builtin` uses when none is given.
    const DEFAULT: VersionArg = VersionArg::Version(Builtin::DEFAULT_VERSION);

    /// The built-in of `transaction` the argument names, for the `call` that
    /// received it (e.g. `Spec.builtin`): `TypeError` for an argument that is
    /// neither a `Version` nor text, `ValueError` for a version no built-in
    /// covers.
    fn resolve(&self, call: &str, transaction: &str) -> PyResult<&'static Builtin> {
        let found = match self {
            VersionArg::Version(version) => Builtin::find(transaction, *version),
            VersionArg::Text(text) => Builtin::find_text(transaction, text),
            VersionArg::Other { shown, kind } => {
                return Err(PyTypeError::new_err(format!(
                    "{call}(version={shown}): version must be an oxedi.Version or its text, not {kind}"
                )));
            }
        };
        found.map_err(|error| {
            PyValueError::new_err(format!("{call}(version={:?}): {error}", error.version))
        })
    }
}

impl<'a, 'py> FromPyObject<'a, 'py> for VersionArg {
    type Error = PyErr;

    fn extract(object: Borrowed<'a, 'py, PyAny>) -> PyResult<VersionArg> {
        if let Ok(version) = object.extract::<PyVersion>() {
            return Ok(VersionArg::Version(version.0));
        }
        if let Ok(text) = object.extract::<String>() {
            return Ok(VersionArg::Text(text));
        }
        let shown = object.repr().map_or_else(
            |_| "an object whose repr failed".to_owned(),
            |text| text.to_string(),
        );
        let kind = object
            .get_type()
            .qualname()
            .map_or_else(|_| "an unknown type".to_owned(), |name| name.to_string());
        Ok(VersionArg::Other { shown, kind })
    }
}

impl<'py> IntoPyObject<'py> for VersionArg {
    type Target = PyAny;
    type Output = Bound<'py, PyAny>;
    type Error = PyErr;

    fn into_pyobject(self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        match self {
            VersionArg::Version(version) => {
                Ok(PyVersion::from(version).into_pyobject(py)?.into_any())
            }
            VersionArg::Text(text) => Ok(PyString::new(py, &text).into_any()),
            // This conversion only renders the default, a `Version`; this arm is never reached.
            VersionArg::Other { shown, .. } => Ok(PyString::new(py, &shown).into_any()),
        }
    }
}

impl PyStubType for VersionArg {
    fn type_output() -> TypeInfo {
        PyVersion::type_output()
    }

    fn type_input() -> TypeInfo {
        PyVersion::type_input()
    }
}

/// The loop structure, element definitions and tables of a file format.
#[gen_stub_pyclass(module = "oxedi._core")]
#[pyclass(name = "Spec", module = "oxedi", frozen)]
pub struct PySpec {
    pub inner: Arc<Spec>,
}

#[gen_stub_pymethods]
#[pymethods]
impl PySpec {
    /// The version of the default built-in spec.
    #[classattr]
    #[pyo3(name = "DEFAULT_VERSION")]
    fn default_version() -> PyVersion {
        Builtin::DEFAULT_VERSION.into()
    }

    /// The built-in spec of `version`; it defaults to `Spec.DEFAULT_VERSION`.
    /// Raises `ValueError` for a version no built-in covers.
    #[staticmethod]
    #[pyo3(signature = (version = VersionArg::DEFAULT))]
    fn builtin(version: VersionArg) -> PyResult<PySpec> {
        let found = version.resolve("Spec.builtin", Builtin::fallback().transaction)?;
        Ok(PySpec {
            inner: Arc::clone(&found.spec),
        })
    }

    /// The spec described by `json`; raises `SpecError` with the core message
    /// when it is not valid.
    #[staticmethod]
    fn from_json(json: &str) -> PyResult<PySpec> {
        let spec = Spec::from_json(json).map_err(|err| SpecError::new_err(err.to_string()))?;
        Ok(PySpec {
            inner: Arc::new(spec),
        })
    }

    /// A new spec with `patch` (a dict or JSON text) merged over this one;
    /// raises `SpecError` when the result is not valid.
    fn patch(
        &self,
        py: Python<'_>,
        // A JSON merge patch: the dict's values are any JSON value.
        #[gen_stub(override_type(type_repr = "builtins.str | builtins.dict[builtins.str, typing.Any]", imports = ("builtins", "typing")))]
        patch: &Bound<'_, PyAny>,
    ) -> PyResult<PySpec> {
        let text = if let Ok(text) = patch.cast::<PyString>() {
            text.to_cow()?.into_owned()
        } else if patch.is_instance_of::<PyDict>() {
            py.import("json")?
                .call_method1("dumps", (patch,))
                .and_then(|text| text.extract::<String>())
                .map_err(|err| {
                    PyTypeError::new_err(format!(
                        "Spec.patch: the dict cannot be written as JSON: {}",
                        err.value(py)
                    ))
                })?
        } else {
            return Err(PyTypeError::new_err(format!(
                "Spec.patch takes a dict or a JSON string, not {}",
                patch.get_type().name()?
            )));
        };
        let spec = self
            .inner
            .merge_patch(&text)
            .map_err(|err| SpecError::new_err(err.to_string()))?;
        Ok(PySpec {
            inner: Arc::new(spec),
        })
    }

    /// The spec as JSON text.
    fn to_json(&self) -> String {
        self.inner.to_json()
    }

    /// The schema of every table of the spec, by name, in table order: one
    /// `pyarrow.Schema` per table, with the column names and Arrow types the
    /// tables of a parse with this spec have. A reference column (such as
    /// `claim` in `services`) holds the `row` of a row in another table, and
    /// its field metadata names that table under the key `oxedi.parent`.
    /// Needs pyarrow.
    #[gen_stub(override_return_type(type_repr = "builtins.dict[builtins.str, pyarrow.Schema]", imports = ("builtins", "pyarrow")))]
    fn schema<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let make = extra(py, "Spec", "schema", "pyarrow", "pandas")?.getattr("schema")?;
        let schemas = PyDict::new(py);
        for (name, schema) in arrow::spec_schemas(&self.inner)? {
            let export = Bound::new(
                py,
                SchemaExport {
                    name: name.clone(),
                    schema,
                },
            )?;
            schemas.set_item(name, make.call1((export,))?)?;
        }
        Ok(schemas)
    }

    /// The loop names, in spec order.
    fn loops(&self) -> Vec<String> {
        self.inner
            .loops()
            .iter()
            .map(|def| def.name.clone())
            .collect()
    }

    fn __repr__(&self) -> String {
        format!(
            "Spec(name='{}', loops={}, tables={})",
            self.inner.name(),
            self.inner.loops().len(),
            self.inner.tables().len()
        )
    }
}
