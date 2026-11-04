//! Python binding of the EDI 835 parser core.
//!
//! Every class delegates to the core. The only logic of its own is the bridge
//! from the core's columns to Arrow record batches (module `arrow`) and back
//! for writing (module `import`), the conversion of diagnostics and write
//! findings to Python attributes (modules `diagnostic` and `write`), and the
//! loops open at each segment, read from the core's loop engine for adapters
//! to external validators (module `parse`).
//! Parsing, streaming and exporting run with the GIL released. It also carries
//! the text of the core's `edi_835_parser.json` patch, so the Python layer that
//! reproduces edi-835-parser applies the same file the core's goldens test.
//! Every class, function and exception is also described for the stub
//! generator; the `stub_gen` binary writes the descriptions as
//! `python/oxedi/_core.pyi`. stubtest is what guards completeness: a
//! `#[pymethods]` block or `#[pyfunction]` without its `gen_stub` attribute
//! leaves the generated stub unchanged, so regenerating it shows no drift,
//! while stubtest reports the item as missing from the stub.

use pyo3::prelude::*;

/// Creates an exception of the `oxedi` package and describes it for the
/// stub of `oxedi._core`, the module that registers it.
macro_rules! native_exception {
    ($name:ident, $base:ty, $doc:literal) => {
        pyo3::create_exception!(oxedi, $name, $base, $doc);

        pyo3_stub_gen::inventory::submit! {
            pyo3_stub_gen::type_info::PyClassInfo {
                pyclass_name: stringify!($name),
                struct_id: std::any::TypeId::of::<$name>,
                getters: &[],
                setters: &[],
                module: Some("oxedi._core"),
                doc: $doc,
                bases: &[|| <$base as pyo3_stub_gen::PyStubType>::type_output()],
                has_eq: false,
                has_ord: false,
                has_hash: false,
                has_str: false,
                subclass: true,
            }
        }
    };
}

mod arrow;
mod diagnostic;
mod document;
mod import;
mod mask;
mod native;
mod parse;
mod spec;
mod stamp;
mod stream;
mod tables;
mod write;

/// The name of the module attribute holding the text of `edi_835_parser.json`.
const EDI_835_PARSER_PATCH: &str = "EDI_835_PARSER_PATCH";

pyo3_stub_gen::module_variable!("oxedi._core", EDI_835_PARSER_PATCH, String);

/// Registers one member of the module.
type Register = fn(&Bound<'_, PyModule>) -> PyResult<()>;

/// The `oxedi._core` extension module.
#[pymodule]
#[pyo3(name = "_core")]
fn core_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    members().into_iter().try_for_each(|add| add(m))?;
    m.add(
        EDI_835_PARSER_PATCH,
        include_str!("../../oxedi_core/specs/edi_835_parser.json"),
    )
}

/// Every exception, class and function of the module, one registration each.
fn members() -> [Register; 23] {
    [
        |m| m.add("ParseError", m.py().get_type::<document::ParseError>()),
        |m| m.add("SpecError", m.py().get_type::<spec::SpecError>()),
        |m| m.add_class::<diagnostic::PyDiagnostic>(),
        |m| m.add_class::<document::PyDelimiters>(),
        |m| m.add_class::<document::PyDocument>(),
        |m| m.add_class::<document::PySegment>(),
        |m| m.add_class::<mask::PyMask>(),
        |m| m.add_class::<parse::PyParseResult>(),
        |m| m.add_class::<spec::PySpec>(),
        |m| m.add_class::<spec::PyVersion>(),
        |m| m.add_class::<stream::PyBatch>(),
        |m| m.add_class::<stream::PyStream>(),
        |m| m.add_class::<tables::PyTable>(),
        |m| m.add_class::<tables::PyTables>(),
        |m| m.add_class::<write::PyEnvelope>(),
        |m| m.add_class::<write::PyWriteFinding>(),
        |m| wrap_pyfunction!(diagnostic::external_diagnostic, m).and_then(|f| m.add_function(f)),
        |m| wrap_pyfunction!(diagnostic::mask_findings, m).and_then(|f| m.add_function(f)),
        |m| wrap_pyfunction!(parse::parse, m).and_then(|f| m.add_function(f)),
        |m| wrap_pyfunction!(parse::loop_paths, m).and_then(|f| m.add_function(f)),
        |m| wrap_pyfunction!(parse::parse_document, m).and_then(|f| m.add_function(f)),
        |m| wrap_pyfunction!(stream::stream, m).and_then(|f| m.add_function(f)),
        |m| wrap_pyfunction!(write::write_tables, m).and_then(|f| m.add_function(f)),
    ]
}

// The description of the module that the `stub_gen` binary writes as
// `python/oxedi/_core.pyi`.
pyo3_stub_gen::define_stub_info_gatherer!(stub_info);
