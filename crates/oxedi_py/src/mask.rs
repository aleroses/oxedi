//! `Mask`: the key and date mode that hide patient data in tables and
//! diagnostics.

use oxedi_core::{DateMode, Mask};
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyString};
use pyo3_stub_gen::derive::{gen_methods_from_python, gen_stub_pyclass, gen_stub_pymethods};

/// Hides the patient data of a parse: the columns the spec flags as
/// patient data are replaced by keyed tokens or shifted dates, and the
/// diagnostics that quote them are masked the same way. The same key always
/// gives the same token, so rows still join on it; without the key the
/// originals cannot be recovered.
#[gen_stub_pyclass(module = "oxedi._core")]
#[pyclass(name = "Mask", module = "oxedi", frozen)]
pub struct PyMask {
    pub inner: Mask,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyMask {
    /// A mask from `key` (at least 16 bytes; a `str` is encoded as UTF-8)
    /// and `dates`: `"year"` (the default) moves each date to January 1 of
    /// its year, `"shift"` moves every date of a row by the same number of
    /// days, and `"keep"` leaves dates as they are. Raises `ValueError` for
    /// a short key or an unknown mode.
    #[new]
    #[pyo3(
        signature = (key, dates = "year"),
        text_signature = "(key, dates='year')"
    )]
    #[gen_stub(skip)]
    fn new(key: &Bound<'_, PyAny>, dates: &str) -> PyResult<Self> {
        let bytes: Vec<u8> = if let Ok(text) = key.cast::<PyString>() {
            text.to_cow()?.as_bytes().to_vec()
        } else if let Ok(bytes) = key.cast::<PyBytes>() {
            bytes.as_bytes().to_vec()
        } else {
            return Err(PyTypeError::new_err(format!(
                "Mask: the key must be bytes or str, not {}",
                key.get_type().name()?
            )));
        };
        let dates = DateMode::parse(dates).map_err(|err| PyValueError::new_err(err.to_string()))?;
        let inner =
            Mask::new(&bytes, dates).map_err(|err| PyValueError::new_err(err.to_string()))?;
        Ok(PyMask { inner })
    }

    fn __repr__(&self) -> String {
        format!("Mask(key=<hidden>, dates='{}')", self.inner.dates().word())
    }
}

pyo3_stub_gen::inventory::submit! {
    gen_methods_from_python! {
        r#"
        class PyMask:
            def __new__(cls, key: builtins.bytes | builtins.str, dates: builtins.str = "year") -> Mask: ...
        "#
    }
}
