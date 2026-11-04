//! Writing rows of a core column into a DuckDB output vector.

use std::ffi::c_char;

use libduckdb_sys as ffi;
use oxedi_core::value::TimeUnit;
use oxedi_core::{Cell, Column, ColumnData};

use crate::error::ReadError;
use crate::types::SqlType;

/// Microseconds in a second: DuckDB's `TIME` counts microseconds.
const MICROS_PER_SECOND: i64 = TimeUnit::Microsecond.ticks_per_second();

/// Where a cell came from, for error messages.
pub struct Place<'a> {
    /// The path of the file.
    pub file: &'a str,
    /// The table read.
    pub table: &'a str,
    /// The column of the cell.
    pub column: &'a str,
    /// The table's `row` column, which names a row as users see it.
    pub rows: Option<&'a ColumnData>,
}

impl Place<'_> {
    /// The value of the `row` column at `index`, when there is one.
    fn row(&self, index: usize) -> Option<i64> {
        match self.rows?.get(index)? {
            Cell::Int64(row) => Some(row),
            _ => None,
        }
    }
}

/// One output vector of the chunk being filled.
pub struct Vector {
    raw: ffi::duckdb_vector,
    /// Rows the vector can hold.
    capacity: usize,
}

impl Vector {
    /// The vector of output column `index`.
    ///
    /// # Safety
    ///
    /// `chunk` must be the output chunk DuckDB passed to the running scan
    /// callback, `index` one of its columns, and `capacity` at most its
    /// vector size.
    pub unsafe fn of(chunk: ffi::duckdb_data_chunk, index: usize, capacity: usize) -> Vector {
        // SAFETY: `chunk` is live and `index` is one of its columns (caller
        // contract).
        let raw = unsafe { ffi::duckdb_data_chunk_get_vector(chunk, index as ffi::idx_t) };
        Vector { raw, capacity }
    }

    /// The vector's first `len` values as `T`, the physical type of its
    /// logical type.
    ///
    /// # Safety
    ///
    /// `T` must be the physical type DuckDB stores for the vector's type.
    unsafe fn values<T>(&mut self, len: usize) -> &mut [T] {
        let len = len.min(self.capacity);
        // SAFETY: the vector's data holds `capacity` values of its physical
        // type (`T`, caller contract) and nothing else aliases it while the
        // chunk is being filled.
        unsafe {
            std::slice::from_raw_parts_mut(ffi::duckdb_vector_get_data(self.raw).cast::<T>(), len)
        }
    }

    /// Marks row `index` NULL.
    pub fn set_null(&mut self, index: usize) {
        // SAFETY: the vector is live; ensuring a writable validity mask
        // before reading it is what the C API requires.
        unsafe {
            ffi::duckdb_vector_ensure_validity_writable(self.raw);
            let validity = ffi::duckdb_vector_get_validity(self.raw);
            ffi::duckdb_validity_set_row_invalid(validity, index as ffi::idx_t);
        }
    }

    /// Stores `bytes` as the VARCHAR or BLOB of row `index`. DuckDB does
    /// not check them: a VARCHAR must have been checked for UTF-8 first.
    fn set_bytes(&mut self, index: usize, bytes: &[u8]) {
        // SAFETY: the vector is a VARCHAR or BLOB vector, and VARCHAR bytes
        // were checked to be UTF-8 by the caller; DuckDB copies the
        // `bytes.len()` bytes at `bytes` into its own string heap.
        unsafe {
            ffi::duckdb_unsafe_vector_assign_string_element_len(
                self.raw,
                index as ffi::idx_t,
                bytes.as_ptr().cast::<c_char>(),
                bytes.len() as ffi::idx_t,
            );
        }
    }

    /// Stores `text` as the VARCHAR of row `index`.
    pub fn set_text(&mut self, index: usize, text: &str) {
        if index < self.capacity {
            self.set_bytes(index, text.as_bytes());
        }
    }

    /// Stores `value` as the INTEGER of row `index`.
    ///
    /// # Safety
    ///
    /// The vector must be an INTEGER vector.
    pub unsafe fn set_integer(&mut self, index: usize, value: i32) {
        // SAFETY: INTEGER vectors store `i32` (caller contract).
        if let Some(slot) = unsafe { self.values::<i32>(self.capacity) }.get_mut(index) {
            *slot = value;
        }
    }

    /// Stores `text` in every one of the first `len` rows.
    pub fn fill_text(&mut self, len: usize, text: &str) {
        for index in 0..len.min(self.capacity) {
            self.set_bytes(index, text.as_bytes());
        }
    }

    /// Copies rows `start..start + len` of `data` into the vector, which has
    /// type `sql`.
    pub fn fill(
        &mut self,
        data: &ColumnData,
        sql: SqlType,
        start: usize,
        len: usize,
        place: &Place<'_>,
    ) -> Result<(), ReadError> {
        let len = len.min(self.capacity);
        let end = start.saturating_add(len);
        let short = || ReadError::Internal {
            message: format!(
                "{:?}, table {:?}, column {:?} has fewer than {end} rows",
                place.file, place.table, place.column
            ),
        };
        match (data.column(), sql) {
            (
                Column::Binary {
                    offsets,
                    data: bytes,
                },
                SqlType::Varchar | SqlType::Blob,
            ) => {
                for row in start..end {
                    if data.validity().get(row) == Some(false) {
                        continue;
                    }
                    let cell = cell_bytes(offsets, bytes, row).ok_or_else(short)?;
                    // This check is what reports invalid text: DuckDB's
                    // checked assign would store NULL for it without an
                    // error, and the unchecked one used after it stores the
                    // bytes as they are.
                    if sql == SqlType::Varchar && std::str::from_utf8(cell).is_err() {
                        return Err(ReadError::InvalidUtf8 {
                            file: place.file.to_owned(),
                            table: place.table.to_owned(),
                            column: place.column.to_owned(),
                            row: place.row(row),
                            index: row,
                            bytes: cell.to_vec(),
                        });
                    }
                    self.set_bytes(row - start, cell);
                }
            }
            (Column::Int64 { values, .. }, SqlType::Bigint) => {
                let source = values.get(start..end).ok_or_else(short)?;
                // SAFETY: BIGINT vectors store `i64`.
                unsafe { self.values::<i64>(len) }.copy_from_slice(source);
            }
            (Column::Decimal128 { values, .. }, SqlType::Decimal { .. }) => {
                let source = values.get(start..end).ok_or_else(short)?;
                // SAFETY: DECIMAL vectors of width 19 to 38 store a 128-bit
                // integer as `duckdb_hugeint`; `SqlType::of` only yields
                // those widths.
                let target = unsafe { self.values::<ffi::duckdb_hugeint>(len) };
                for (slot, value) in target.iter_mut().zip(source) {
                    *slot = hugeint(*value);
                }
            }
            (Column::Date32(values), SqlType::Date) => {
                let source = values.get(start..end).ok_or_else(short)?;
                // SAFETY: DATE vectors store `duckdb_date`, an `i32` of days
                // since 1970-01-01, the same count the core holds.
                unsafe { self.values::<i32>(len) }.copy_from_slice(source);
            }
            (Column::Time32(values), SqlType::Time) => {
                let source = values.get(start..end).ok_or_else(short)?;
                // SAFETY: TIME vectors store `duckdb_time`, an `i64` of
                // microseconds since midnight.
                let target = unsafe { self.values::<i64>(len) };
                for (slot, seconds) in target.iter_mut().zip(source) {
                    *slot = i64::from(*seconds) * MICROS_PER_SECOND;
                }
            }
            (column, sql) => {
                return Err(ReadError::Internal {
                    message: format!(
                        "{:?}, table {:?}, column {:?}: a {} column cannot fill a {sql:?} vector",
                        place.file,
                        place.table,
                        place.column,
                        kind_name(column)
                    ),
                });
            }
        }
        if data.null_count() > 0 {
            for row in start..end {
                if data.validity().get(row) == Some(false) {
                    self.set_null(row - start);
                }
            }
        }
        Ok(())
    }
}

/// The bytes of row `row` of a binary column.
pub(super) fn cell_bytes<'a>(offsets: &[i32], bytes: &'a [u8], row: usize) -> Option<&'a [u8]> {
    let from = usize::try_from(*offsets.get(row)?).ok()?;
    let to = usize::try_from(*offsets.get(row.checked_add(1)?)?).ok()?;
    bytes.get(from..to)
}

/// A 128-bit integer split as DuckDB's `hugeint` stores it.
pub(super) fn hugeint(value: i128) -> ffi::duckdb_hugeint {
    ffi::duckdb_hugeint {
        lower: value as u64,
        upper: (value >> 64) as i64,
    }
}

/// The name of a column's physical type, for internal errors.
fn kind_name(column: &Column) -> &'static str {
    match column {
        Column::Binary { .. } => "binary",
        Column::Int64 { .. } => "int64",
        Column::Decimal128 { .. } => "decimal128",
        Column::Date32(_) => "date32",
        Column::Time32(_) => "time32",
    }
}
