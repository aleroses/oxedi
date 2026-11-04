//! The envelope's date and time from Python's `datetime.date` and
//! `datetime.time`, as the core counts them: days since 1970-01-01 and
//! seconds since midnight.

use oxedi_core::value::{days_from_civil, seconds_of_day};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// The integer attributes `names` of a `datetime` object, in order.
fn parts<const N: usize>(value: &Bound<'_, PyAny>, names: [&str; N]) -> PyResult<[i32; N]> {
    let mut out = [0; N];
    for (slot, name) in out.iter_mut().zip(names) {
        *slot = value.getattr(name)?.extract()?;
    }
    Ok(out)
}

/// Whether `value` is a `datetime.datetime`.
fn is_datetime(value: &Bound<'_, PyAny>) -> PyResult<bool> {
    let class = value.py().import("datetime")?.getattr("datetime")?;
    value.is_instance(&class)
}

/// Days since 1970-01-01 of a `datetime.date`; a `datetime.datetime` is
/// refused, since the envelope's time is its own field.
pub fn date_days(date: &Bound<'_, PyAny>) -> PyResult<i32> {
    if is_datetime(date)? {
        return Err(PyValueError::new_err(format!(
            "date {date} is a datetime; give a datetime.date (the envelope's time is its own field)"
        )));
    }
    let [year, month, day] = parts(date, ["year", "month", "day"])?;
    days_from_civil(year, month, day)
        .ok_or_else(|| PyValueError::new_err(format!("date {date} is out of range")))
}

/// Seconds since midnight of a `datetime.time` without a time zone and
/// without a fraction of a second.
pub fn time_seconds(time: &Bound<'_, PyAny>) -> PyResult<i32> {
    if !time.getattr("tzinfo")?.is_none() {
        return Err(PyValueError::new_err(format!(
            "time {time} has a time zone; give a datetime.time without one, as the envelope writes local time"
        )));
    }
    let [hour, minute, second, microsecond] =
        parts(time, ["hour", "minute", "second", "microsecond"])?;
    if microsecond != 0 {
        return Err(PyValueError::new_err(format!(
            "time {time} has a fraction of a second; the envelope holds whole seconds"
        )));
    }
    seconds_of_day(hour, minute, second)
        .ok_or_else(|| PyValueError::new_err(format!("time {time} is out of range")))
}
