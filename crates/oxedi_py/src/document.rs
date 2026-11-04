//! `Document`, `Segment` and `Delimiters`: the file held losslessly.

use oxedi_core::{Delimiters, Document, Element};
use pyo3::exceptions::{PyIndexError, PyOverflowError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyInt, PyList};
use pyo3_stub_gen::derive::{gen_methods_from_python, gen_stub_pyclass, gen_stub_pymethods};
use pyo3_stub_gen::type_info::{
    MemberInfo, MethodInfo, MethodType, ParameterDefault, ParameterInfo, ParameterKind,
    PyMethodsInfo,
};
use pyo3_stub_gen::{PyStubType, TypeInfo};

native_exception!(
    ParseError,
    PyValueError,
    "Input that cannot be indexed: its delimiters cannot be read or it exceeds the size limit; the message says what was found and where."
);

/// The five delimiters of an interchange.
#[gen_stub_pyclass(module = "oxedi._core")]
#[pyclass(name = "Delimiters", module = "oxedi", frozen, eq, skip_from_py_object)]
#[derive(Clone, PartialEq)]
pub struct PyDelimiters {
    pub inner: Delimiters,
}

/// A delimiter parameter of `Delimiters.__new__`: `bytes`, defaulting to
/// `default` written as a Python literal.
const fn delimiter(name: &'static str, default: fn() -> String) -> ParameterInfo {
    ParameterInfo {
        name,
        kind: ParameterKind::PositionalOrKeyword,
        type_info: <Py<PyBytes> as PyStubType>::type_input,
        default: ParameterDefault::Expr {
            value: default,
            source_module: None,
        },
    }
}

/// An optional delimiter parameter of `Delimiters.__new__`: `bytes` or
/// `None`, defaulting to `None`.
const fn optional_delimiter(name: &'static str) -> ParameterInfo {
    ParameterInfo {
        name,
        kind: ParameterKind::PositionalOrKeyword,
        type_info: <Option<Py<PyBytes>> as PyStubType>::type_input,
        default: ParameterDefault::Expr {
            value: || "None".to_string(),
            source_module: None,
        },
    }
}

// `Delimiters.__new__` is described by hand: its byte-string defaults have
// no Python form the derive macros can write. It mirrors `signature =` on
// `PyDelimiters::new`. `__hash__` is `None`: the class compares by value and
// is not hashable.
pyo3_stub_gen::inventory::submit! {
    PyMethodsInfo {
        struct_id: std::any::TypeId::of::<PyDelimiters>,
        attrs: &[MemberInfo {
            name: "__hash__",
            // Overriding `object.__hash__` with `None` is how typeshed spells an
            // unhashable class; the ignore keeps type checkers from flagging it.
            r#type: || {
                TypeInfo::with_module(
                    "typing.ClassVar[None]  # type: ignore[assignment]",
                    "typing".into(),
                )
            },
            doc: "",
            default: None,
            deprecated: None,
        }],
        getters: &[],
        setters: &[],
        methods: &[MethodInfo {
            name: "__new__",
            parameters: &[
                delimiter("element", || "b\"*\"".to_string()),
                delimiter("component", || "b\":\"".to_string()),
                delimiter("segment", || "b\"~\"".to_string()),
                optional_delimiter("repetition"),
                optional_delimiter("release"),
            ],
            r#return: || TypeInfo::unqualified("Delimiters"),
            doc: "Each delimiter is one byte. `element`, `component` and `segment` default to\n`*`, `:` and `~`; `repetition` and `release` default to `None` (not in use).\nRaises `ValueError` for a value that is not exactly one byte.",
            r#type: MethodType::New,
            is_async: false,
            deprecated: None,
            type_ignored: None,
            is_overload: false,
        }],
        file: file!(),
        line: line!(),
        column: column!(),
    }
}

/// The bytes as Python writes them, e.g. `b'*'` or `b'\n'`.
fn bytes_repr(py: Python<'_>, bytes: &[u8]) -> PyResult<String> {
    Ok(PyBytes::new(py, bytes).repr()?.to_string())
}

fn one_byte(py: Python<'_>, name: &str, value: &[u8]) -> PyResult<u8> {
    match value {
        [byte] => Ok(*byte),
        _ => Err(PyValueError::new_err(format!(
            "delimiter {name} must be exactly one byte, got {} bytes: {}",
            value.len(),
            bytes_repr(py, value)?
        ))),
    }
}

fn optional_repr(py: Python<'_>, byte: Option<u8>) -> PyResult<String> {
    byte.map_or_else(|| Ok("None".to_string()), |b| bytes_repr(py, &[b]))
}

fn single<'py>(py: Python<'py>, byte: u8) -> Bound<'py, PyBytes> {
    PyBytes::new(py, &[byte])
}

#[gen_stub_pymethods]
#[pymethods]
impl PyDelimiters {
    #[new]
    #[gen_stub(skip)]
    // `text_signature` mirrors `signature =`, as the stub description above does.
    #[pyo3(
        signature = (element = b"*".as_slice(), component = b":".as_slice(), segment = b"~".as_slice(), repetition = None, release = None),
        text_signature = "(element=b'*', component=b':', segment=b'~', repetition=None, release=None)"
    )]
    fn new(
        py: Python<'_>,
        element: &[u8],
        component: &[u8],
        segment: &[u8],
        repetition: Option<&[u8]>,
        release: Option<&[u8]>,
    ) -> PyResult<Self> {
        let mut inner = Delimiters::new(
            one_byte(py, "element", element)?,
            one_byte(py, "component", component)?,
            one_byte(py, "segment", segment)?,
        );
        inner.repetition = repetition
            .map(|value| one_byte(py, "repetition", value))
            .transpose()?;
        inner.release = release
            .map(|value| one_byte(py, "release", value))
            .transpose()?;
        Ok(Self { inner })
    }

    /// The element separator, one byte.
    #[getter]
    fn element<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        single(py, self.inner.element)
    }

    /// The component separator, one byte.
    #[getter]
    fn component<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        single(py, self.inner.component)
    }

    /// The segment terminator, one byte.
    #[getter]
    fn segment<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        single(py, self.inner.segment)
    }

    /// The repetition separator, or `None` when the file has none.
    #[getter]
    fn repetition<'py>(&self, py: Python<'py>) -> Option<Bound<'py, PyBytes>> {
        self.inner.repetition.map(|byte| single(py, byte))
    }

    /// The release character, or `None` when it is not in use.
    #[getter]
    fn release<'py>(&self, py: Python<'py>) -> Option<Bound<'py, PyBytes>> {
        self.inner.release.map(|byte| single(py, byte))
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "Delimiters(element={}, component={}, segment={}, repetition={}, release={})",
            bytes_repr(py, &[self.inner.element])?,
            bytes_repr(py, &[self.inner.component])?,
            bytes_repr(py, &[self.inner.segment])?,
            optional_repr(py, self.inner.repetition)?,
            optional_repr(py, self.inner.release)?
        ))
    }
}

/// The file, indexed into segments and held byte for byte.
#[gen_stub_pyclass(module = "oxedi._core")]
#[pyclass(name = "Document", module = "oxedi", frozen)]
pub struct PyDocument {
    pub inner: Document<'static>,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyDocument {
    fn __len__(&self) -> usize {
        self.inner.len()
    }

    #[gen_stub(skip)]
    fn __getitem__(slf: &Bound<'_, Self>, index: isize) -> PyResult<PySegment> {
        let len = slf.get().inner.len();
        let resolved = if index < 0 {
            len.checked_sub(index.unsigned_abs())
        } else {
            Some(index.unsigned_abs()).filter(|position| *position < len)
        };
        match resolved {
            Some(index) => Ok(PySegment {
                document: slf.clone().unbind(),
                index,
            }),
            None => Err(PyIndexError::new_err(format!(
                "segment index {index} is out of range: the document has {len} segments"
            ))),
        }
    }

    /// The file, byte for byte.
    fn write<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, self.inner.as_bytes())
    }

    /// The delimiters the file was indexed with.
    #[getter]
    fn delimiters(&self) -> PyDelimiters {
        PyDelimiters {
            inner: *self.inner.delimiters(),
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "Document(segments={}, bytes={})",
            self.inner.len(),
            self.inner.as_bytes().len()
        )
    }
}

// Slot methods take their argument by position only.
pyo3_stub_gen::inventory::submit! {
    gen_methods_from_python! {
        r#"
        class PyDocument:
            def __getitem__(self, index: builtins.int, /) -> Segment: ...
        "#
    }
}

/// One segment of a document, read from the document on access.
#[gen_stub_pyclass(module = "oxedi._core")]
#[pyclass(name = "Segment", module = "oxedi", frozen)]
pub struct PySegment {
    document: Py<PyDocument>,
    index: usize,
}

impl PySegment {
    fn with<T>(
        &self,
        py: Python<'_>,
        read: impl FnOnce(&oxedi_core::Segment<'_>) -> T,
    ) -> PyResult<T> {
        let document = self.document.bind(py).get();
        document
            .inner
            .segment(self.index)
            .map(|segment| read(&segment))
            .ok_or_else(|| out_of_range(self.index, document.inner.len()))
    }
}

/// A simple element as `bytes`, a composite one as a list of `bytes`.
fn element_object<'py>(py: Python<'py>, element: &Element<'_>) -> PyResult<Bound<'py, PyAny>> {
    match element {
        Element::Simple(value) => Ok(PyBytes::new(py, value).into_any()),
        Element::Composite(values) => {
            let parts = PyList::empty(py);
            for value in values {
                parts.append(PyBytes::new(py, value))?;
            }
            Ok(parts.into_any())
        }
    }
}

/// A 1-based index argument as `usize`. Below 1 is refused with `refusal`
/// naming the value; an integer beyond `i64` is refused when negative and
/// otherwise lies past every place, so it reads as the largest index.
fn one_based(
    value: &Bound<'_, PyInt>,
    refusal: impl Fn(&Bound<'_, PyInt>) -> String,
) -> PyResult<usize> {
    match value.extract::<i64>() {
        Ok(number) => usize::try_from(number)
            .ok()
            .filter(|at| *at >= 1)
            .ok_or_else(|| PyValueError::new_err(refusal(value))),
        Err(error) if error.is_instance_of::<PyOverflowError>(value.py()) => {
            if value.lt(0)? {
                Err(PyValueError::new_err(refusal(value)))
            } else {
                Ok(usize::MAX)
            }
        }
        Err(error) => Err(error),
    }
}

/// The 1-based `position` and `component` of `method` as indexes, refusing
/// one below 1 with the value it got.
fn places(
    method: &str,
    position: &Bound<'_, PyInt>,
    component: Option<&Bound<'_, PyInt>>,
) -> PyResult<(usize, Option<usize>)> {
    let position = one_based(position, |got| {
        format!("{method}: position must be 1 or more (XX01 is 1), got {got}")
    })?;
    let component = component
        .map(|value| {
            one_based(value, |got| {
                format!("{method}: component must be 1 or more, got {got}")
            })
        })
        .transpose()?;
    Ok((position, component))
}

fn out_of_range(index: usize, len: usize) -> PyErr {
    PyIndexError::new_err(format!(
        "segment index {index} is out of range: the document has {len} segments"
    ))
}

#[gen_stub_pymethods]
#[pymethods]
impl PySegment {
    /// The 0-based position of the segment in the document.
    #[getter]
    fn index(&self) -> usize {
        self.index
    }

    /// The segment identifier; empty for an empty frame.
    #[getter]
    fn id<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let document = self.document.bind(py).get();
        document
            .inner
            .segment_id(self.index)
            .map(|id| PyBytes::new(py, id))
            .ok_or_else(|| out_of_range(self.index, document.inner.len()))
    }

    /// The elements in X12 order (`elements[0]` is `XX01`): `bytes` for a
    /// simple element, a list of `bytes` for a composite one.
    #[getter]
    #[gen_stub(override_return_type(type_repr = "builtins.list[builtins.bytes | builtins.list[builtins.bytes]]", imports = ("builtins",)))]
    fn elements<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        self.with(py, |segment| -> PyResult<Bound<'py, PyList>> {
            let list = PyList::empty(py);
            for element in &segment.elements {
                list.append(element_object(py, element)?)?;
            }
            Ok(list)
        })?
    }

    /// The element at the 1-based `position` (`XX01` is 1), or one of its
    /// components: as `elements` holds it without `component` (`bytes` for
    /// a simple element, a list of `bytes` for a composite one), and the
    /// bytes of that 1-based component with it (component 1 of a simple
    /// element is the element). `None` when the segment has no such place.
    #[pyo3(signature = (position, component=None))]
    #[gen_stub(override_return_type(type_repr = "builtins.bytes | builtins.list[builtins.bytes] | None", imports = ("builtins",)))]
    fn element<'py>(
        &self,
        py: Python<'py>,
        position: &Bound<'_, PyInt>,
        component: Option<&Bound<'_, PyInt>>,
    ) -> PyResult<Option<Bound<'py, PyAny>>> {
        let (position, component) = places("Segment.element", position, component)?;
        self.with(py, |segment| -> PyResult<Option<Bound<'py, PyAny>>> {
            let Some(component) = component else {
                return segment
                    .element(position)
                    .map(|e| element_object(py, e))
                    .transpose();
            };
            Ok(segment
                .component(position, component)
                .map(|bytes| PyBytes::new(py, bytes).into_any()))
        })?
    }

    /// Whether the segment has the element at the 1-based `position`, or
    /// that 1-based `component` of it, even when it is empty: `element(...)`
    /// is not `None`.
    #[pyo3(signature = (position, component=None))]
    fn has(
        &self,
        py: Python<'_>,
        position: &Bound<'_, PyInt>,
        component: Option<&Bound<'_, PyInt>>,
    ) -> PyResult<bool> {
        let (position, component) = places("Segment.has", position, component)?;
        self.with(py, |segment| match component {
            None => segment.element(position).is_some(),
            Some(component) => segment.component(position, component).is_some(),
        })
    }

    /// The exact bytes of the segment: leading trivia, body and terminator.
    #[getter]
    fn raw<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let document = self.document.bind(py).get();
        document
            .inner
            .raw(self.index)
            .map(|raw| PyBytes::new(py, raw))
            .ok_or_else(|| out_of_range(self.index, document.inner.len()))
    }

    /// The segment's byte range in the input, `(start, end)` with `end`
    /// exclusive: leading trivia, body and terminator, as `raw` holds them.
    #[getter]
    fn span(&self, py: Python<'_>) -> PyResult<(usize, usize)> {
        let document = self.document.bind(py).get();
        document
            .inner
            .span(self.index)
            .map(|span| (span.raw.start, span.raw.end))
            .ok_or_else(|| out_of_range(self.index, document.inner.len()))
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        self.with(py, |segment| -> PyResult<String> {
            Ok(format!(
                "Segment(index={}, id={})",
                segment.index,
                bytes_repr(py, segment.id)?
            ))
        })?
    }
}

/// Indexes owned bytes, reading the delimiters from the ISA or using the
/// ones given.
pub fn index(bytes: Vec<u8>, delimiters: Option<Delimiters>) -> PyResult<Document<'static>> {
    match delimiters {
        Some(delimiters) => Document::with_delimiters(bytes, delimiters)
            .map_err(|err| ParseError::new_err(err.to_string())),
        None => Document::parse(bytes).map_err(|err| ParseError::new_err(err.to_string())),
    }
}
