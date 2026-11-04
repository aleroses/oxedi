//! The bridge from Arrow tables to the core's columns, for writing.
//!
//! A table comes in through the Arrow PyCapsule stream interface (pyarrow,
//! Polars and the tables of a parse export it); an object without it, such
//! as a pandas frame, is read through `pyarrow.table`. Each column is
//! converted to the type the spec gives it: text and bytes of any Arrow
//! layout (dictionary-encoded too) to bytes, integers (and whole floats) to
//! integers, decimals rescaled exactly (floats are refused for them), dates
//! from dates or midnight timestamps, and times from their Arrow units. A value that
//! would change on the way is refused with the table, column and row.

use arrow_array::cast::AsArray;
use arrow_array::ffi_stream::{ArrowArrayStreamReader, FFI_ArrowArrayStream};
use arrow_array::types::{
    Date32Type, Date64Type, Decimal128Type, Float32Type, Float64Type, Int8Type, Int16Type,
    Int32Type, Int64Type, Time32MillisecondType, Time32SecondType, Time64MicrosecondType,
    Time64NanosecondType, TimestampMicrosecondType, TimestampMillisecondType,
    TimestampNanosecondType, TimestampSecondType, UInt8Type, UInt16Type, UInt32Type, UInt64Type,
};
use arrow_array::{Array, ArrowPrimitiveType, RecordBatch, RecordBatchReader};
use arrow_schema::{DataType, TimeUnit};
use oxedi_core::project::table_columns;
use oxedi_core::value::{
    TimeUnit as Unit, date_from_midnight, date_from_millis, integer_from_f64, integer_from_u128,
    rescale, seconds_from,
};
use oxedi_core::{Cell, ColumnType, Spec, Table, Tables};
use pyo3::prelude::*;
use pyo3::types::{PyCapsule, PyCapsuleMethods, PyMapping};

use crate::tables::extra;

/// What went wrong reading one table, as the message says it.
pub type Refused = String;

/// The tables of a mapping of table names to Arrow tables, each converted
/// to the spec's schema.
pub fn tables(spec: &Spec, mapping: &Bound<'_, PyMapping>) -> PyResult<Result<Tables, Refused>> {
    let mut out = Vec::new();
    for item in mapping.items()?.iter() {
        let (name, value): (String, Bound<'_, PyAny>) = item.extract()?;
        let Some(def) = spec.table(&name) else {
            let names: Vec<String> = spec
                .tables()
                .iter()
                .map(|def| format!("{:?}", def.name))
                .collect();
            return Ok(Err(format!(
                "table {name:?} is not a table of the spec, whose tables are {}",
                names.join(", ")
            )));
        };
        let schema = table_columns(spec, def);
        let reader = stream(&value)?;
        match table(&name, &schema, reader) {
            Ok(table) => out.push(table),
            Err(refused) => return Ok(Err(refused)),
        }
    }
    Ok(Ok(Tables::new(out)))
}

/// The Arrow stream of a table: its own capsule, or pyarrow's reading of it.
fn stream(value: &Bound<'_, PyAny>) -> PyResult<ArrowArrayStreamReader> {
    let exporter = if value.hasattr("__arrow_c_stream__")? {
        value.clone()
    } else {
        extra(value.py(), "oxedi", "write", "pyarrow", "pandas")?
            .getattr("table")?
            .call1((value,))?
    };
    let capsule = exporter.call_method0("__arrow_c_stream__")?;
    let capsule = capsule.cast::<PyCapsule>()?;
    let pointer = capsule.pointer_checked(Some(c"arrow_array_stream"))?;
    // SAFETY: a capsule named "arrow_array_stream" holds an
    // `ArrowArrayStream` (the Arrow PyCapsule interface). `from_raw` moves
    // the stream out and leaves a released one in its place, so the
    // capsule's destructor releases nothing twice.
    let stream = unsafe { FFI_ArrowArrayStream::from_raw(pointer.as_ptr().cast()) };
    ArrowArrayStreamReader::try_new(stream)
        .map_err(|err| pyo3::exceptions::PyValueError::new_err(err.to_string()))
}

/// One table, converted batch by batch.
fn table(
    name: &str,
    schema: &[(String, ColumnType)],
    reader: ArrowArrayStreamReader,
) -> Result<Table, Refused> {
    let fields = reader.schema();
    let mut columns = Vec::with_capacity(fields.fields().len());
    for field in fields.fields() {
        let Some((_, kind)) = schema.iter().find(|(column, _)| column == field.name()) else {
            let names: Vec<String> = schema
                .iter()
                .map(|(column, _)| format!("{column:?}"))
                .collect();
            return Err(format!(
                "table {name:?} has no column {:?} in the spec; its columns are {}",
                field.name(),
                names.join(", ")
            ));
        };
        columns.push((field.name().clone(), *kind));
    }
    let mut table = Table::new(name, columns.clone());
    let mut offset = 0;
    for batch in reader {
        let batch = batch.map_err(|err| format!("table {name:?} could not be read: {err}"))?;
        rows(name, &columns, &batch, offset, &mut table)?;
        offset += batch.num_rows();
    }
    Ok(table)
}

/// Appends the rows of one batch.
fn rows(
    name: &str,
    columns: &[(String, ColumnType)],
    batch: &RecordBatch,
    offset: usize,
    table: &mut Table,
) -> Result<(), Refused> {
    let arrays = batch.columns();
    let mut cells = Vec::with_capacity(columns.len());
    for row in 0..batch.num_rows() {
        cells.clear();
        for ((column, kind), array) in columns.iter().zip(arrays) {
            let cell = cell(array.as_ref(), row, *kind).map_err(|reason| {
                format!(
                    "table {name:?} column {column:?} row {}: {reason}",
                    offset + row
                )
            })?;
            cells.push(cell);
        }
        table.push_row(&cells).map_err(|err| err.to_string())?;
    }
    Ok(())
}

/// The value of one row as a cell of the column's type.
fn cell(array: &dyn Array, row: usize, kind: ColumnType) -> Result<Cell<'_>, String> {
    if array.is_null(row) || *array.data_type() == DataType::Null {
        return Ok(Cell::Null);
    }
    let unsupported = || {
        format!(
            "an Arrow {} column cannot hold the spec's {kind} values",
            array.data_type()
        )
    };
    if let Some(dictionary) = array.as_any_dictionary_opt() {
        let key = integer(dictionary.keys(), row)
            .and_then(Result::ok)
            .ok_or_else(unsupported)?;
        let values = dictionary.values();
        let index = usize::try_from(key)
            .ok()
            .filter(|&index| index < values.len())
            .ok_or_else(|| {
                format!(
                    "the dictionary key {key} is outside the column's {} dictionary values",
                    values.len()
                )
            })?;
        return cell(values.as_ref(), index, kind);
    }
    match kind {
        ColumnType::Binary => bytes(array, row).ok_or_else(unsupported).map(Cell::Binary),
        ColumnType::Decimal128 { .. }
            if matches!(array.data_type(), DataType::Float32 | DataType::Float64) =>
        {
            Err(format!(
                "an Arrow {} column is refused for the spec's {kind} values: a float may not \
                 hold the amount exactly; give decimal values (decimal.Decimal in Python)",
                array.data_type()
            ))
        }
        ColumnType::Int64 { .. } => integer(array, row)
            .ok_or_else(unsupported)?
            .map(Cell::Int64),
        ColumnType::Decimal128 { scale, .. } => decimal(array, row, scale)
            .ok_or_else(unsupported)?
            .map(Cell::Decimal128),
        ColumnType::Date32 => date(array, row).ok_or_else(unsupported)?.map(Cell::Date32),
        ColumnType::Time32 => time(array, row).ok_or_else(unsupported)?.map(Cell::Time32),
    }
}

fn bytes(array: &dyn Array, row: usize) -> Option<&[u8]> {
    Some(match array.data_type() {
        DataType::Binary => array.as_binary_opt::<i32>()?.value(row),
        DataType::LargeBinary => array.as_binary_opt::<i64>()?.value(row),
        DataType::BinaryView => array.as_binary_view_opt()?.value(row),
        DataType::Utf8 => array.as_string_opt::<i32>()?.value(row).as_bytes(),
        DataType::LargeUtf8 => array.as_string_opt::<i64>()?.value(row).as_bytes(),
        DataType::Utf8View => array.as_string_view_opt()?.value(row).as_bytes(),
        _ => return None,
    })
}

/// The value at `row` of an array of primitive type `T`; `None` when the
/// array is of another type.
fn primitive<T: ArrowPrimitiveType>(array: &dyn Array, row: usize) -> Option<T::Native> {
    Some(array.as_primitive_opt::<T>()?.value(row))
}

/// A signed integer of any width; `None` for another type.
fn signed(array: &dyn Array, row: usize) -> Option<i64> {
    match array.data_type() {
        DataType::Int8 => primitive::<Int8Type>(array, row).map(i64::from),
        DataType::Int16 => primitive::<Int16Type>(array, row).map(i64::from),
        DataType::Int32 => primitive::<Int32Type>(array, row).map(i64::from),
        DataType::Int64 => primitive::<Int64Type>(array, row),
        _ => None,
    }
}

/// An unsigned integer of any width; `None` for another type.
fn unsigned(array: &dyn Array, row: usize) -> Option<u64> {
    match array.data_type() {
        DataType::UInt8 => primitive::<UInt8Type>(array, row).map(u64::from),
        DataType::UInt16 => primitive::<UInt16Type>(array, row).map(u64::from),
        DataType::UInt32 => primitive::<UInt32Type>(array, row).map(u64::from),
        DataType::UInt64 => primitive::<UInt64Type>(array, row),
        _ => None,
    }
}

/// A float of either width; `None` for another type.
fn float(array: &dyn Array, row: usize) -> Option<f64> {
    match array.data_type() {
        DataType::Float32 => primitive::<Float32Type>(array, row).map(f64::from),
        DataType::Float64 => primitive::<Float64Type>(array, row),
        _ => None,
    }
}

/// An integer of any width, or a float with no fraction; `None` for another
/// type, the reason for a value that does not fit.
fn integer(array: &dyn Array, row: usize) -> Option<Result<i64, String>> {
    signed(array, row)
        .map(Ok)
        .or_else(|| unsigned(array, row).map(|value| integer_from_u128(u128::from(value))))
        .or_else(|| float(array, row).map(integer_from_f64))
        .map(|converted| converted.map_err(|error| error.to_string()))
}

/// A decimal rescaled to `scale`, or an integer scaled up; `None` for
/// another type, the reason for a value that does not rescale exactly.
fn decimal(array: &dyn Array, row: usize, scale: u8) -> Option<Result<i128, String>> {
    let rescaled =
        |value: i128, from: i32| rescale(value, from, scale).map_err(|error| error.to_string());
    if let DataType::Decimal128(_, from) = array.data_type() {
        let value = primitive::<Decimal128Type>(array, row)?;
        return Some(rescaled(value, i32::from(*from)));
    }
    Some(integer(array, row)?.and_then(|value| rescaled(i128::from(value), 0)))
}

/// The value at `row` of an array of type `T`, with the `unit` it is
/// counted in; `None` for another type.
fn ticks<T>(array: &dyn Array, row: usize, unit: Unit) -> Option<(i64, Unit)>
where
    T: ArrowPrimitiveType,
    T::Native: Into<i64>,
{
    Some((primitive::<T>(array, row)?.into(), unit))
}

/// A timestamp's value and the core unit of its Arrow unit.
fn timestamp(array: &dyn Array, row: usize, unit: TimeUnit) -> Option<(i64, Unit)> {
    match unit {
        TimeUnit::Second => ticks::<TimestampSecondType>(array, row, Unit::Second),
        TimeUnit::Millisecond => ticks::<TimestampMillisecondType>(array, row, Unit::Millisecond),
        TimeUnit::Microsecond => ticks::<TimestampMicrosecondType>(array, row, Unit::Microsecond),
        TimeUnit::Nanosecond => ticks::<TimestampNanosecondType>(array, row, Unit::Nanosecond),
    }
}

/// Days since 1970-01-01 from a date, or from a timestamp without a time
/// zone at exactly midnight; `None` for another type.
fn date(array: &dyn Array, row: usize) -> Option<Result<i32, String>> {
    let converted = match array.data_type() {
        DataType::Timestamp(unit, None) => {
            let (value, unit) = timestamp(array, row, *unit)?;
            date_from_midnight(value, unit)
        }
        DataType::Date32 => Ok(primitive::<Date32Type>(array, row)?),
        DataType::Date64 => date_from_millis(primitive::<Date64Type>(array, row)?),
        _ => return None,
    };
    Some(converted.map_err(|error| error.to_string()))
}

/// A time's value and the core unit of its Arrow unit.
fn time_ticks(array: &dyn Array, row: usize) -> Option<(i64, Unit)> {
    match array.data_type() {
        DataType::Time32(TimeUnit::Second) => ticks::<Time32SecondType>(array, row, Unit::Second),
        DataType::Time32(TimeUnit::Millisecond) => {
            ticks::<Time32MillisecondType>(array, row, Unit::Millisecond)
        }
        DataType::Time64(TimeUnit::Microsecond) => {
            ticks::<Time64MicrosecondType>(array, row, Unit::Microsecond)
        }
        DataType::Time64(TimeUnit::Nanosecond) => {
            ticks::<Time64NanosecondType>(array, row, Unit::Nanosecond)
        }
        _ => None,
    }
}

/// Seconds since midnight from a time; `None` for another type.
fn time(array: &dyn Array, row: usize) -> Option<Result<i32, String>> {
    let (value, unit) = time_ticks(array, row)?;
    Some(seconds_from(value, unit).map_err(|error| error.to_string()))
}
