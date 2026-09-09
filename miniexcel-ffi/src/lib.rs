use std::cell::RefCell;
use std::ffi::{CStr, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::str::FromStr;

use miniexcel::{CellReference, CellValue, DynamicRow, HeaderMode, MiniExcel, ReadOptions};

const ABI_VERSION: u32 = 1;
const RESULT_END: i32 = 0;
const RESULT_BATCH: i32 = 1;
const ERROR_INVALID_ARGUMENT: i32 = -1;
const ERROR_QUERY: i32 = -2;
const ERROR_PANIC: i32 = -3;

thread_local! {
    static LAST_ERROR: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

pub struct QueryHandle {
    rows: Box<dyn Iterator<Item = miniexcel::Result<DynamicRow>> + Send>,
    frame: Vec<u8>,
}

#[unsafe(no_mangle)]
pub extern "C" fn miniexcel_abi_version() -> u32 {
    ABI_VERSION
}

/// Opens a path-based XLSX query and returns an opaque native handle.
///
/// # Safety
///
/// String pointers must be null-terminated UTF-8. `path`, `start_cell`, and `out_handle` must be
/// non-null and valid for the duration of the call. `sheet_name` may be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_query_open(
    path: *const c_char,
    use_header_row: u8,
    sheet_name: *const c_char,
    start_cell: *const c_char,
    out_handle: *mut *mut QueryHandle,
) -> i32 {
    ffi_result(|| {
        if path.is_null() || start_cell.is_null() || out_handle.is_null() {
            set_last_error("path, start_cell, and out_handle are required");
            return Err(ERROR_INVALID_ARGUMENT);
        }

        let path = unsafe { read_utf8(path) }?;
        let start_cell = unsafe { read_utf8(start_cell) }?;
        let start_cell = CellReference::from_str(start_cell).map_err(|error| {
            set_last_error(error.to_string());
            ERROR_INVALID_ARGUMENT
        })?;

        let mut options = ReadOptions::new()
            .with_header_mode(if use_header_row == 0 {
                HeaderMode::None
            } else {
                HeaderMode::FirstRow
            })
            .with_start_cell(start_cell);

        if !sheet_name.is_null() {
            let sheet_name = unsafe { read_utf8(sheet_name) }?;
            if !sheet_name.is_empty() {
                options = options.with_sheet_name(sheet_name);
            }
        }

        let rows = MiniExcel::query_with_options(path, &options).map_err(|error| {
            set_last_error(error.to_string());
            ERROR_QUERY
        })?;
        let handle = Box::new(QueryHandle { rows, frame: Vec::new() });
        unsafe { ptr::write(out_handle, Box::into_raw(handle)) };
        Ok(RESULT_BATCH)
    })
}

/// Writes the next bounded batch into memory owned by the query handle.
///
/// # Safety
///
/// `handle` must have been returned by `miniexcel_query_open` and not yet closed. Output pointers
/// must be non-null and writable. Returned data is valid until the next call using the handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_query_next_batch(
    handle: *mut QueryHandle,
    max_rows: u32,
    out_data: *mut *const u8,
    out_length: *mut usize,
) -> i32 {
    ffi_result(|| {
        if handle.is_null() || max_rows == 0 || out_data.is_null() || out_length.is_null() {
            set_last_error("handle, max_rows, out_data, and out_length are required");
            return Err(ERROR_INVALID_ARGUMENT);
        }

        let handle = unsafe { &mut *handle };
        handle.frame.clear();
        write_u32(&mut handle.frame, 0);

        let mut row_count = 0_u32;
        while row_count < max_rows {
            let Some(row) = handle.rows.next() else {
                break;
            };
            let row = row.map_err(|error| {
                set_last_error(error.to_string());
                ERROR_QUERY
            })?;
            write_row(&mut handle.frame, &row)?;
            row_count += 1;
        }

        if row_count == 0 {
            unsafe {
                ptr::write(out_data, ptr::null());
                ptr::write(out_length, 0);
            }
            return Ok(RESULT_END);
        }

        handle.frame[0..4].copy_from_slice(&row_count.to_le_bytes());
        unsafe {
            ptr::write(out_data, handle.frame.as_ptr());
            ptr::write(out_length, handle.frame.len());
        }
        Ok(RESULT_BATCH)
    })
}

/// Closes a query handle and releases its worker and temporary resources.
///
/// # Safety
///
/// `handle` must be null or a handle returned by `miniexcel_query_open` that has not been closed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_query_close(handle: *mut QueryHandle) {
    if !handle.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| drop(unsafe { Box::from_raw(handle) })));
    }
}

/// Returns the last error recorded on the current native thread.
///
/// # Safety
///
/// `out_length` may be null; otherwise it must be writable. The returned data remains valid until
/// the next MiniExcel FFI error on this thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_last_error(out_length: *mut usize) -> *const u8 {
    LAST_ERROR.with(|error| {
        let error = error.borrow();
        if !out_length.is_null() {
            unsafe { ptr::write(out_length, error.len()) };
        }
        error.as_ptr()
    })
}

fn ffi_result(operation: impl FnOnce() -> Result<i32, i32>) -> i32 {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(Ok(result)) => result,
        Ok(Err(code)) => code,
        Err(_) => {
            set_last_error("Rust panic crossed the MiniExcel FFI boundary");
            ERROR_PANIC
        }
    }
}

unsafe fn read_utf8<'a>(value: *const c_char) -> Result<&'a str, i32> {
    unsafe { CStr::from_ptr(value) }.to_str().map_err(|error| {
        set_last_error(error.to_string());
        ERROR_INVALID_ARGUMENT
    })
}

fn set_last_error(message: impl AsRef<str>) {
    LAST_ERROR.with(|error| {
        let mut error = error.borrow_mut();
        error.clear();
        error.extend_from_slice(message.as_ref().as_bytes());
    });
}

fn write_row(frame: &mut Vec<u8>, row: &DynamicRow) -> Result<(), i32> {
    write_length(frame, row.len())?;
    for (name, value) in row {
        write_string(frame, name)?;
        match value {
            CellValue::Empty => frame.push(0),
            CellValue::Bool(value) => {
                frame.push(1);
                frame.push(u8::from(*value));
            }
            CellValue::Int(value) => {
                frame.push(2);
                frame.extend_from_slice(&value.to_le_bytes());
            }
            CellValue::Float(value) => {
                frame.push(3);
                frame.extend_from_slice(&value.to_le_bytes());
            }
            CellValue::String(value) => {
                frame.push(4);
                write_string(frame, value)?;
            }
            CellValue::Date(value) => {
                frame.push(5);
                write_string(frame, value.format("%Y-%m-%d").to_string())?;
            }
            CellValue::Time(value) => {
                frame.push(6);
                write_string(frame, value.format("%H:%M:%S%.f").to_string())?;
            }
            CellValue::DateTime(value) => {
                frame.push(7);
                write_string(frame, value.format("%Y-%m-%dT%H:%M:%S%.f").to_string())?;
            }
            CellValue::Duration(value) => {
                frame.push(8);
                frame.extend_from_slice(&value.num_milliseconds().to_le_bytes());
            }
            CellValue::Error(value) => {
                frame.push(9);
                write_string(frame, value)?;
            }
        }
    }
    Ok(())
}

fn write_string(frame: &mut Vec<u8>, value: impl AsRef<str>) -> Result<(), i32> {
    let bytes = value.as_ref().as_bytes();
    write_length(frame, bytes.len())?;
    frame.extend_from_slice(bytes);
    Ok(())
}

fn write_length(frame: &mut Vec<u8>, length: usize) -> Result<(), i32> {
    let length = u32::try_from(length).map_err(|_| {
        set_last_error("FFI frame value exceeds the 4 GiB format limit");
        ERROR_QUERY
    })?;
    write_u32(frame, length);
    Ok(())
}

fn write_u32(frame: &mut Vec<u8>, value: u32) {
    frame.extend_from_slice(&value.to_le_bytes());
}
