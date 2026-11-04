//! `stream`: the tables of a file in batches, one per closed loop instance.
//!
//! The stream owns a copy of the input and walks it with the tokenizer, so
//! no document index is built; after each instance of the chosen loop closes
//! it moves the rows appended so far out of the projector. What it holds at
//! any time is the input, the open loops and the rows of one batch.

use std::sync::{Arc, Mutex, TryLockError};

use oxedi_core::{
    Builtin, Delimiters, Diagnostic, Event, LoopId, Mask, Processor, Spec, Tables, Tokenizer,
};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyList;
use pyo3_stub_gen::derive::{gen_stub_pyclass, gen_stub_pyfunction, gen_stub_pymethods};
use self_cell::self_cell;

use crate::diagnostic;
use crate::document::{ParseError, PyDelimiters};
use crate::mask::PyMask;
use crate::parse::copy_input;
use crate::spec::{self, PySpec};
use crate::tables::PyTables;

/// What the walk borrows from: the spec, the input and its delimiters.
struct Source {
    spec: Arc<Spec>,
    bytes: Vec<u8>,
    delimiters: Delimiters,
    mask: Option<Mask>,
}

/// The tokenizer over the input and the processor over the spec.
struct Pass<'a> {
    segments: Tokenizer<'a>,
    processor: Processor<'a>,
}

self_cell!(
    struct Walk {
        owner: Source,
        #[covariant]
        dependent: Pass,
    }
);

/// One batch: the tables and diagnostics produced since the previous one,
/// and the spec that projected them.
type Produced = (Tables, Vec<Diagnostic>, Arc<Spec>);

struct State {
    walk: Walk,
    by: LoopId,
    done: bool,
}

impl State {
    /// Feeds segments until an instance of `by` closes, or finishes the
    /// stream at its end. `None` once the stream is over and nothing is left.
    fn step(&mut self) -> Option<Produced> {
        if self.done {
            return None;
        }
        let by = self.by;
        let (produced, done) = self.walk.with_dependent_mut(|source, pass| {
            let spec = Arc::clone(&source.spec);
            let mut diagnostics = Vec::new();
            for segment in pass.segments.by_ref() {
                let output = pass.processor.feed(&segment);
                diagnostics.extend_from_slice(output.diagnostics());
                if output.events().contains(&Event::LoopClosed { id: by }) {
                    return ((pass.processor.take_tables(), diagnostics, spec), false);
                }
            }
            diagnostics.extend_from_slice(pass.processor.finish().diagnostics());
            ((pass.processor.take_tables(), diagnostics, spec), true)
        });
        self.done = done;
        let empty = produced.0.iter().all(|table| table.is_empty()) && produced.1.is_empty();
        if done && empty { None } else { Some(produced) }
    }
}

/// Iterates the batches of a file. Each step runs with the GIL released.
///
/// A stream must be advanced from one thread at a time: a `next()` that finds
/// another thread inside a step raises a `RuntimeError` naming the stream;
/// unlike a generator, it does not raise `ValueError`.
#[gen_stub_pyclass(module = "oxedi._core")]
#[pyclass(name = "Stream", module = "oxedi", frozen)]
pub struct PyStream {
    state: Mutex<State>,
}

/// The tables and diagnostics of one closed loop instance (or of the end
/// of the stream).
#[gen_stub_pyclass(module = "oxedi._core")]
#[pyclass(name = "Batch", module = "oxedi", frozen)]
pub struct PyBatch {
    tables: Py<PyTables>,
    diagnostics: Py<PyList>,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyBatch {
    /// The rows appended since the previous batch, by table.
    #[getter]
    fn tables(&self, py: Python<'_>) -> Py<PyTables> {
        self.tables.clone_ref(py)
    }

    /// The diagnostics emitted since the previous batch, in stream order.
    #[getter]
    #[gen_stub(override_return_type(type_repr = "builtins.list[Diagnostic]", imports = ("builtins",)))]
    fn diagnostics(&self, py: Python<'_>) -> Py<PyList> {
        self.diagnostics.clone_ref(py)
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "Batch(tables={}, diagnostics={})",
            self.tables.bind(py).repr()?,
            self.diagnostics.bind(py).len()
        ))
    }
}

#[gen_stub_pymethods]
#[pymethods]
impl PyStream {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    // Returning `None` ends the iteration, so a call never yields `None`.
    #[gen_stub(override_return_type(type_repr = "Batch"))]
    fn __next__(&self, py: Python<'_>) -> PyResult<Option<PyBatch>> {
        let stepped = py.detach(|| match self.state.try_lock() {
            Ok(mut state) => Ok(state.step()),
            Err(TryLockError::WouldBlock) => Err(
                "Stream.__next__: this stream is already being advanced by another thread; \
                 a stream is advanced from one thread at a time",
            ),
            Err(TryLockError::Poisoned(_)) => Err(
                "Stream.__next__: a previous step of this stream panicked, so it cannot be advanced",
            ),
        });
        let Some((tables, diagnostics, spec)) = stepped.map_err(PyRuntimeError::new_err)? else {
            return Ok(None);
        };
        let diagnostics = diagnostic::to_list(py, diagnostics)?;
        Ok(Some(PyBatch {
            tables: Py::new(py, PyTables::projected(tables, spec))?,
            diagnostics: diagnostics.unbind(),
        }))
    }
}

/// The loop of `spec` named `by`, or an error naming the loops it has.
fn loop_of(spec: &Spec, by: &str) -> PyResult<LoopId> {
    spec.loop_id(by).ok_or_else(|| {
        let names: Vec<&str> = spec.loops().iter().map(|l| l.name.as_str()).collect();
        PyValueError::new_err(format!(
            "stream by {by:?}: the spec has no such loop; its loops are {}",
            names.join(", ")
        ))
    })
}

/// Walks a file and yields a batch each time an instance of loop `by`
/// closes, and one more at the end when anything is left. Without `spec`,
/// the built-in spec of the version the file declares is used, else the
/// default one. With `mask`, the patient data of every batch is hidden.
#[gen_stub_pyfunction]
#[pyfunction]
#[pyo3(signature = (data, spec = None, by = "transaction", delimiters = None, mask = None))]
pub fn stream(
    py: Python<'_>,
    #[gen_stub(override_type(type_repr = "typing_extensions.Buffer", imports = ("typing_extensions",)))]
    data: &Bound<'_, PyAny>,
    spec: Option<&Bound<'_, PySpec>>,
    // Described as written so the stub carries the default value.
    #[gen_stub(override_type(type_repr = "builtins.str", imports = ("builtins",)))] by: &str,
    delimiters: Option<&Bound<'_, PyDelimiters>>,
    mask: Option<&Bound<'_, PyMask>>,
) -> PyResult<PyStream> {
    let bytes = copy_input("stream", data)?;
    let given = spec::given(spec);
    // The loop is checked before the input is read, against the given spec
    // or the default built-in; every built-in has the same loops.
    loop_of(given.as_deref().unwrap_or(&Builtin::fallback().spec), by)?;
    let delimiters = match delimiters {
        Some(delimiters) => delimiters.get().inner,
        None => *Tokenizer::new(&bytes)
            .map_err(|err| ParseError::new_err(err.to_string()))?
            .delimiters(),
    };
    let spec = spec::given_or_selected(given, Tokenizer::with_delimiters(&bytes, delimiters));
    let by = loop_of(&spec, by)?;
    let source = Source {
        spec,
        bytes,
        delimiters,
        mask: mask.map(|mask| mask.get().inner.clone()),
    };
    let walk = py.detach(|| {
        Walk::new(source, |source| Pass {
            segments: Tokenizer::with_delimiters(&source.bytes, source.delimiters),
            processor: Processor::new(&source.spec, &source.delimiters)
                .with_mask(source.mask.as_ref()),
        })
    });
    Ok(PyStream {
        state: Mutex::new(State {
            walk,
            by,
            done: false,
        }),
    })
}
