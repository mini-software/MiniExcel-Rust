use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::ffi::{CStr, c_char};
use std::fs::File;
use std::io::{BufReader, ErrorKind, Read, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::ptr;
use std::str::FromStr;

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
use miniexcel::{
    CellMap, CellReference, CellValue, CommentPerson, CommentTimestamp, CsvConfiguration,
    CsvEncoding, CsvReadOptions, CsvWriteOptions, DynamicRow, ExistingSheetPolicy, HeaderMode,
    HeaderStyle, HorizontalAlignment, InsertOptions, MergeSameCellsOptions, MiniExcel, ReadOptions,
    RgbColor, SheetType, SheetVisibility, TableStyle, TargetRelationshipPolicy, TemplateOptions,
    VerticalAlignment, WriteOptions,
};
use quick_xml::Reader as XmlReader;
use quick_xml::events::{BytesStart, Event};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const ABI_VERSION: u32 = 1;
const RESULT_END: i32 = 0;
const RESULT_BATCH: i32 = 1;
const ERROR_INVALID_ARGUMENT: i32 = -1;
const ERROR_QUERY: i32 = -2;
const ERROR_PANIC: i32 = -3;
const ERROR_WRITE: i32 = -4;

thread_local! {
    static LAST_ERROR: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

pub struct QueryHandle {
    rows: Box<dyn Iterator<Item = miniexcel::Result<DynamicRow>> + Send>,
    frame: Vec<u8>,
}

struct PhysicalRowIterator {
    inner: Box<dyn Iterator<Item = miniexcel::Result<DynamicRow>> + Send>,
    pattern: std::vec::IntoIter<PhysicalRowAction>,
    columns: Vec<String>,
    start_column: usize,
    normalize_merged_cells: bool,
    merged_ranges: Vec<MergedRangeInfo>,
    merge_anchor_values: Vec<Option<CellValue>>,
}

enum PhysicalRowAction {
    Data { row: usize, columns: Vec<usize> },
    Empty,
    Skip,
}

#[derive(Clone, Copy)]
struct MergedRangeInfo {
    start_row: usize,
    start_column: usize,
    end_row: usize,
    end_column: usize,
}

impl Iterator for PhysicalRowIterator {
    type Item = miniexcel::Result<DynamicRow>;

    fn next(&mut self) -> Option<Self::Item> {
        for action in self.pattern.by_ref() {
            match action {
                PhysicalRowAction::Empty => {
                    let row = self
                        .columns
                        .iter()
                        .cloned()
                        .map(|column| (column, CellValue::Empty))
                        .collect();
                    return Some(Ok(row));
                }
                PhysicalRowAction::Data { row: row_index, columns: physical_columns } => {
                    let mut row = self.inner.next()?;
                    if self.normalize_merged_cells {
                        if let Ok(row) = row.as_mut() {
                            for (offset, (_, value)) in row.iter_mut().enumerate() {
                                let column = self.start_column + offset;
                                if !physical_columns.contains(&column) {
                                    *value = CellValue::Empty;
                                }
                            }
                            for (index, range) in self.merged_ranges.iter().enumerate() {
                                if row_index == range.start_row {
                                    let offset =
                                        range.start_column.saturating_sub(self.start_column);
                                    self.merge_anchor_values[index] =
                                        row.get_index(offset).map(|(_, value)| value.clone());
                                }
                                if row_index < range.start_row || row_index > range.end_row {
                                    continue;
                                }
                                let Some(anchor) = self.merge_anchor_values[index].as_ref() else {
                                    continue;
                                };
                                for column in range.start_column..=range.end_column {
                                    if column == range.start_column && row_index == range.start_row
                                    {
                                        continue;
                                    }
                                    if physical_columns.contains(&column) {
                                        let offset = column.saturating_sub(self.start_column);
                                        if let Some((_, value)) = row.get_index_mut(offset) {
                                            if value.is_empty() {
                                                *value = anchor.clone();
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    return Some(row);
                }
                PhysicalRowAction::Skip => {
                    if let Err(error) = self.inner.next()? {
                        return Some(Err(error));
                    }
                }
            }
        }
        self.inner.next()
    }
}

pub struct BufferHandle {
    frame: Vec<u8>,
}

pub struct CancellationHandle {
    token: miniexcel::CancellationToken,
}

struct QueryOpenOptions {
    path: *const c_char,
    use_header_row: u8,
    sheet_name: *const c_char,
    start_cell: *const c_char,
    end_cell: *const c_char,
    ignore_empty_rows: u8,
    fill_merged_cells: u8,
    trim_headers: u8,
    enable_shared_string_cache: u8,
    shared_string_cache_size: u64,
    shared_string_cache_path: *const c_char,
}

struct CsvWriteArguments {
    path: *const c_char,
    data: *const u8,
    data_length: usize,
    delimiter: u8,
    encoding: u8,
    write_bom: u8,
    print_header: u8,
    overwrite_file: u8,
}

struct SpoolRows {
    reader: BufReader<File>,
    finished: bool,
}

impl SpoolRows {
    fn open(path: impl AsRef<Path>) -> std::io::Result<Self> {
        Ok(Self { reader: BufReader::new(File::open(path)?), finished: false })
    }
}

impl Iterator for SpoolRows {
    type Item = miniexcel::Result<DynamicRow>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        let mut length = [0_u8; 4];
        if let Err(error) = self.reader.read_exact(&mut length) {
            self.finished = true;
            return if error.kind() == ErrorKind::UnexpectedEof {
                None
            } else {
                Some(Err(error.into()))
            };
        }
        let length = u32::from_le_bytes(length) as usize;
        let mut frame = vec![0_u8; length];
        if let Err(error) = self.reader.read_exact(&mut frame) {
            self.finished = true;
            return Some(Err(error.into()));
        }
        match decode_rows(&frame) {
            Ok(mut rows) if rows.len() == 1 => Some(Ok(rows.remove(0))),
            Ok(_) => {
                self.finished = true;
                Some(Err(std::io::Error::new(
                    ErrorKind::InvalidData,
                    "spool frame must contain exactly one row",
                )
                .into()))
            }
            Err(_) => {
                self.finished = true;
                Some(Err(std::io::Error::new(ErrorKind::InvalidData, "spool row frame is invalid")
                    .into()))
            }
        }
    }
}

type DeclaredDimension = (Option<String>, Option<String>);

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
    ffi_result(|| unsafe {
        open_query(
            QueryOpenOptions {
                path,
                use_header_row,
                sheet_name,
                start_cell,
                end_cell: ptr::null(),
                ignore_empty_rows: 0,
                fill_merged_cells: 0,
                trim_headers: 1,
                enable_shared_string_cache: 1,
                shared_string_cache_size: 5 * 1024 * 1024,
                shared_string_cache_path: ptr::null(),
            },
            out_handle,
        )
    })
}

/// Opens a bounded path-based XLSX query and returns an opaque native handle.
///
/// # Safety
///
/// String pointers must be null-terminated UTF-8. `path`, `start_cell`, and `out_handle` must be
/// non-null and valid for the duration of the call. `sheet_name` and `end_cell` may be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_query_range_open(
    path: *const c_char,
    use_header_row: u8,
    sheet_name: *const c_char,
    start_cell: *const c_char,
    end_cell: *const c_char,
    out_handle: *mut *mut QueryHandle,
) -> i32 {
    ffi_result(|| unsafe {
        open_query(
            QueryOpenOptions {
                path,
                use_header_row,
                sheet_name,
                start_cell,
                end_cell,
                ignore_empty_rows: 0,
                fill_merged_cells: 0,
                trim_headers: 1,
                enable_shared_string_cache: 1,
                shared_string_cache_size: 5 * 1024 * 1024,
                shared_string_cache_path: ptr::null(),
            },
            out_handle,
        )
    })
}

/// Opens a configured path-based XLSX query and returns an opaque native handle.
///
/// # Safety
///
/// Required string and output pointers must be non-null and valid for the duration of the call.
/// `sheet_name`, `end_cell`, and `shared_string_cache_path` may be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_query_options_open(
    path: *const c_char,
    use_header_row: u8,
    sheet_name: *const c_char,
    start_cell: *const c_char,
    end_cell: *const c_char,
    ignore_empty_rows: u8,
    fill_merged_cells: u8,
    trim_headers: u8,
    enable_shared_string_cache: u8,
    shared_string_cache_size: u64,
    shared_string_cache_path: *const c_char,
    out_handle: *mut *mut QueryHandle,
) -> i32 {
    ffi_result(|| unsafe {
        open_query(
            QueryOpenOptions {
                path,
                use_header_row,
                sheet_name,
                start_cell,
                end_cell,
                ignore_empty_rows,
                fill_merged_cells,
                trim_headers,
                enable_shared_string_cache,
                shared_string_cache_size,
                shared_string_cache_path,
            },
            out_handle,
        )
    })
}

/// Opens a path-based query over a named OpenXML table.
///
/// # Safety
///
/// `path`, `table_name`, and `out_handle` must be non-null and valid for the duration of the call.
/// `sheet_name` may be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_query_table_open(
    path: *const c_char,
    sheet_name: *const c_char,
    table_name: *const c_char,
    out_handle: *mut *mut QueryHandle,
) -> i32 {
    ffi_result(|| {
        if path.is_null() || table_name.is_null() || out_handle.is_null() {
            set_last_error("path, table_name, and out_handle are required");
            return Err(ERROR_INVALID_ARGUMENT);
        }

        unsafe { ptr::write(out_handle, ptr::null_mut()) };
        let path = unsafe { read_utf8(path) }?;
        let table_name = unsafe { read_utf8(table_name) }?;
        if table_name.is_empty() {
            set_last_error("table_name cannot be empty");
            return Err(ERROR_INVALID_ARGUMENT);
        }
        let sheet_name = if sheet_name.is_null() {
            None
        } else {
            let value = unsafe { read_utf8(sheet_name) }?;
            (!value.is_empty()).then_some(value)
        };

        let rows = MiniExcel::query_table(path, table_name, sheet_name).map_err(|error| {
            set_last_error(error.to_string());
            ERROR_QUERY
        })?;
        let handle = Box::new(QueryHandle { rows, frame: Vec::new() });
        unsafe { ptr::write(out_handle, Box::into_raw(handle)) };
        Ok(RESULT_BATCH)
    })
}

/// Opens a path-based CSV query using explicit read options.
///
/// # Safety
///
/// `path` and `out_handle` must be non-null and valid for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_query_csv_open(
    path: *const c_char,
    use_header_row: u8,
    delimiter: u8,
    encoding: u8,
    read_empty_as_null: u8,
    trim_headers: u8,
    out_handle: *mut *mut QueryHandle,
) -> i32 {
    ffi_result(|| {
        if path.is_null() || out_handle.is_null() {
            set_last_error("path and out_handle are required");
            return Err(ERROR_INVALID_ARGUMENT);
        }
        if delimiter == 0 {
            set_last_error("delimiter must be a single-byte character");
            return Err(ERROR_INVALID_ARGUMENT);
        }

        unsafe { ptr::write(out_handle, ptr::null_mut()) };
        let path = unsafe { read_utf8(path) }?;
        let options = csv_read_options(
            use_header_row,
            delimiter,
            encoding,
            read_empty_as_null,
            trim_headers,
        )?;
        let rows = MiniExcel::query_csv_with_options(path, &options).map_err(|error| {
            set_last_error(error.to_string());
            ERROR_QUERY
        })?;
        let handle = Box::new(QueryHandle { rows, frame: Vec::new() });
        unsafe { ptr::write(out_handle, Box::into_raw(handle)) };
        Ok(RESULT_BATCH)
    })
}

/// Returns selected CSV column names through an owned metadata buffer.
///
/// # Safety
///
/// `path` and all output pointers must be non-null and valid for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_get_csv_columns(
    path: *const c_char,
    use_header_row: u8,
    delimiter: u8,
    encoding: u8,
    read_empty_as_null: u8,
    trim_headers: u8,
    out_handle: *mut *mut BufferHandle,
    out_data: *mut *const u8,
    out_length: *mut usize,
) -> i32 {
    ffi_result(|| {
        if path.is_null() || out_handle.is_null() || out_data.is_null() || out_length.is_null() {
            set_last_error("path, out_handle, out_data, and out_length are required");
            return Err(ERROR_INVALID_ARGUMENT);
        }

        unsafe {
            ptr::write(out_handle, ptr::null_mut());
            ptr::write(out_data, ptr::null());
            ptr::write(out_length, 0);
        }

        let path = unsafe { read_utf8(path) }?;
        let options = csv_read_options(
            use_header_row,
            delimiter,
            encoding,
            read_empty_as_null,
            trim_headers,
        )?;
        let columns = MiniExcel::get_csv_columns(path, &options).map_err(|error| {
            set_last_error(error.to_string());
            ERROR_QUERY
        })?;
        let handle = Box::new(BufferHandle { frame: write_strings(columns)? });
        unsafe {
            ptr::write(out_data, handle.frame.as_ptr());
            ptr::write(out_length, handle.frame.len());
            ptr::write(out_handle, Box::into_raw(handle));
        }
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

/// Returns worksheet names in workbook order using memory owned by an opaque buffer handle.
///
/// # Safety
///
/// `path`, `out_handle`, `out_data`, and `out_length` must be non-null and valid for the duration
/// of the call. Returned data remains valid until `miniexcel_buffer_close` closes the handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_get_sheet_names(
    path: *const c_char,
    out_handle: *mut *mut BufferHandle,
    out_data: *mut *const u8,
    out_length: *mut usize,
) -> i32 {
    ffi_result(|| {
        if path.is_null() || out_handle.is_null() || out_data.is_null() || out_length.is_null() {
            set_last_error("path, out_handle, out_data, and out_length are required");
            return Err(ERROR_INVALID_ARGUMENT);
        }

        unsafe {
            ptr::write(out_handle, ptr::null_mut());
            ptr::write(out_data, ptr::null());
            ptr::write(out_length, 0);
        }

        let path = unsafe { read_utf8(path) }?;
        let names = MiniExcel::get_sheet_names(path).map_err(|error| {
            set_last_error(error.to_string());
            ERROR_QUERY
        })?;
        let mut frame = Vec::new();
        write_length(&mut frame, names.len())?;
        for name in names {
            write_string(&mut frame, name)?;
        }

        let handle = Box::new(BufferHandle { frame });
        unsafe {
            ptr::write(out_data, handle.frame.as_ptr());
            ptr::write(out_length, handle.frame.len());
            ptr::write(out_handle, Box::into_raw(handle));
        }
        Ok(RESULT_BATCH)
    })
}

/// Returns selected column names using memory owned by an opaque buffer handle.
///
/// # Safety
///
/// `path`, `start_cell`, and all output pointers must be non-null and valid for the duration of
/// the call. `sheet_name` may be null. Returned data remains valid until the handle is closed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_get_columns(
    path: *const c_char,
    use_header_row: u8,
    sheet_name: *const c_char,
    start_cell: *const c_char,
    out_handle: *mut *mut BufferHandle,
    out_data: *mut *const u8,
    out_length: *mut usize,
) -> i32 {
    ffi_result(|| {
        if path.is_null()
            || start_cell.is_null()
            || out_handle.is_null()
            || out_data.is_null()
            || out_length.is_null()
        {
            set_last_error("path, start_cell, out_handle, out_data, and out_length are required");
            return Err(ERROR_INVALID_ARGUMENT);
        }

        unsafe {
            ptr::write(out_handle, ptr::null_mut());
            ptr::write(out_data, ptr::null());
            ptr::write(out_length, 0);
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

        let columns = MiniExcel::get_columns(path, &options).map_err(|error| {
            set_last_error(error.to_string());
            ERROR_QUERY
        })?;
        let handle = Box::new(BufferHandle { frame: write_strings(columns)? });
        unsafe {
            ptr::write(out_data, handle.frame.as_ptr());
            ptr::write(out_length, handle.frame.len());
            ptr::write(out_handle, Box::into_raw(handle));
        }
        Ok(RESULT_BATCH)
    })
}

/// Returns worksheet dimensions as optional A1 start/end address pairs.
///
/// # Safety
///
/// `path` and all output pointers must be non-null and valid for the duration of the call.
/// Returned data remains valid until the buffer handle is closed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_get_sheet_dimensions(
    path: *const c_char,
    out_handle: *mut *mut BufferHandle,
    out_data: *mut *const u8,
    out_length: *mut usize,
) -> i32 {
    ffi_result(|| {
        if path.is_null() || out_handle.is_null() || out_data.is_null() || out_length.is_null() {
            set_last_error("path, out_handle, out_data, and out_length are required");
            return Err(ERROR_INVALID_ARGUMENT);
        }

        unsafe {
            ptr::write(out_handle, ptr::null_mut());
            ptr::write(out_data, ptr::null());
            ptr::write(out_length, 0);
        }

        let path = unsafe { read_utf8(path) }?;
        let dimensions = declared_sheet_dimensions(path)?;
        let mut frame = Vec::new();
        write_length(&mut frame, dimensions.len())?;
        for (start_cell, end_cell) in dimensions {
            write_string(&mut frame, start_cell.unwrap_or_default())?;
            write_string(&mut frame, end_cell.unwrap_or_default())?;
        }

        let handle = Box::new(BufferHandle { frame });
        unsafe {
            ptr::write(out_data, handle.frame.as_ptr());
            ptr::write(out_length, handle.frame.len());
            ptr::write(out_handle, Box::into_raw(handle));
        }
        Ok(RESULT_BATCH)
    })
}

/// Returns worksheet metadata in workbook order.
///
/// # Safety
///
/// `path` and all output pointers must be non-null and valid for the duration of the call.
/// Returned data remains valid until the buffer handle is closed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_get_sheet_info(
    path: *const c_char,
    out_handle: *mut *mut BufferHandle,
    out_data: *mut *const u8,
    out_length: *mut usize,
) -> i32 {
    ffi_result(|| {
        if path.is_null() || out_handle.is_null() || out_data.is_null() || out_length.is_null() {
            set_last_error("path, out_handle, out_data, and out_length are required");
            return Err(ERROR_INVALID_ARGUMENT);
        }

        unsafe {
            ptr::write(out_handle, ptr::null_mut());
            ptr::write(out_data, ptr::null());
            ptr::write(out_length, 0);
        }

        let path = unsafe { read_utf8(path) }?;
        let sheets = MiniExcel::get_sheet_info(path).map_err(|error| {
            set_last_error(error.to_string());
            ERROR_QUERY
        })?;
        let mut frame = Vec::new();
        write_length(&mut frame, sheets.len())?;
        for sheet in sheets {
            write_u32(&mut frame, sheet.id());
            write_length(&mut frame, sheet.index())?;
            write_string(&mut frame, sheet.name())?;
            frame.push(match sheet.sheet_type() {
                SheetType::Worksheet => 0,
                SheetType::DialogSheet => 1,
                SheetType::MacroSheet => 2,
                SheetType::ChartSheet => 3,
                SheetType::Vba => 4,
            });
            frame.push(match sheet.visibility() {
                SheetVisibility::Visible => 0,
                SheetVisibility::Hidden => 1,
                SheetVisibility::VeryHidden => 2,
            });
            frame.push(u8::from(sheet.is_active()));
        }

        let handle = Box::new(BufferHandle { frame });
        unsafe {
            ptr::write(out_data, handle.frame.as_ptr());
            ptr::write(out_length, handle.frame.len());
            ptr::write(out_handle, Box::into_raw(handle));
        }
        Ok(RESULT_BATCH)
    })
}

/// Returns threaded comments, replies, and legacy notes for a worksheet.
///
/// # Safety
///
/// `path` and all output pointers must be non-null and valid for the duration of the call.
/// `sheet_name` may be null. Returned data remains valid until the buffer handle is closed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_get_comments(
    path: *const c_char,
    sheet_name: *const c_char,
    out_handle: *mut *mut BufferHandle,
    out_data: *mut *const u8,
    out_length: *mut usize,
) -> i32 {
    ffi_result(|| {
        if path.is_null() || out_handle.is_null() || out_data.is_null() || out_length.is_null() {
            set_last_error("path, out_handle, out_data, and out_length are required");
            return Err(ERROR_INVALID_ARGUMENT);
        }

        unsafe {
            ptr::write(out_handle, ptr::null_mut());
            ptr::write(out_data, ptr::null());
            ptr::write(out_length, 0);
        }

        let path = unsafe { read_utf8(path) }?;
        let sheet_name = if sheet_name.is_null() {
            None
        } else {
            let value = unsafe { read_utf8(sheet_name) }?;
            (!value.is_empty()).then_some(value)
        };
        let comments = MiniExcel::get_comments(path, sheet_name).map_err(|error| {
            set_last_error(error.to_string());
            ERROR_QUERY
        })?;
        let mut frame = Vec::new();
        write_string(&mut frame, comments.sheet_name())?;
        write_length(&mut frame, comments.threaded_comments().len())?;
        for comment in comments.threaded_comments() {
            write_string(&mut frame, comment.id().to_string())?;
            write_string(&mut frame, comment.cell().to_string())?;
            write_person(&mut frame, comment.person())?;
            write_timestamp(&mut frame, comment.created_at())?;
            frame.push(u8::from(comment.resolved()));
            write_string(&mut frame, comment.text())?;
            write_length(&mut frame, comment.replies().len())?;
            for reply in comment.replies() {
                write_string(&mut frame, reply.id().to_string())?;
                write_string(&mut frame, reply.parent_id().to_string())?;
                write_person(&mut frame, reply.person())?;
                write_timestamp(&mut frame, reply.created_at())?;
                write_string(&mut frame, reply.text())?;
            }
        }
        write_length(&mut frame, comments.notes().len())?;
        for note in comments.notes() {
            write_optional_string(&mut frame, note.id().map(|id| id.to_string()).as_deref())?;
            write_string(&mut frame, note.cell().to_string())?;
            write_optional_string(&mut frame, note.author())?;
            write_string(&mut frame, note.text())?;
        }

        let handle = Box::new(BufferHandle { frame });
        unsafe {
            ptr::write(out_data, handle.frame.as_ptr());
            ptr::write(out_length, handle.frame.len());
            ptr::write(out_handle, Box::into_raw(handle));
        }
        Ok(RESULT_BATCH)
    })
}

/// Reads explicitly mapped worksheet cells into one dynamic row.
///
/// # Safety
///
/// `path`, `mapping_data`, and all output pointers must be valid for supplied lengths.
/// `sheet_name` may be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_read_mapped(
    path: *const c_char,
    sheet_name: *const c_char,
    mapping_data: *const u8,
    mapping_length: usize,
    out_handle: *mut *mut BufferHandle,
    out_data: *mut *const u8,
    out_length: *mut usize,
) -> i32 {
    ffi_result(|| {
        if path.is_null()
            || mapping_data.is_null()
            || out_handle.is_null()
            || out_data.is_null()
            || out_length.is_null()
        {
            set_last_error("path, mapping_data, out_handle, out_data, and out_length are required");
            return Err(ERROR_INVALID_ARGUMENT);
        }
        unsafe {
            ptr::write(out_handle, ptr::null_mut());
            ptr::write(out_data, ptr::null());
            ptr::write(out_length, 0);
        }
        let path = unsafe { read_utf8(path) }?;
        let mut reader =
            FrameInput::new(unsafe { std::slice::from_raw_parts(mapping_data, mapping_length) });
        let count = reader.read_length()?;
        let mut mapping = CellMap::new();
        let mut fields = Vec::with_capacity(count);
        if !sheet_name.is_null() {
            let sheet_name = unsafe { read_utf8(sheet_name) }?;
            if !sheet_name.is_empty() {
                mapping = mapping.with_sheet_name(sheet_name);
            }
        }
        for _ in 0..count {
            let field = reader.read_string()?;
            let cell = CellReference::from_str(&reader.read_string()?).map_err(|error| {
                set_last_error(error.to_string());
                ERROR_INVALID_ARGUMENT
            })?;
            mapping = mapping.with_cell(&field, cell);
            fields.push(field);
        }
        reader.ensure_complete()?;
        let mut values =
            MiniExcel::read_mapped_as::<serde_json::Map<String, serde_json::Value>>(path, &mapping)
                .map_err(|error| {
                    set_last_error(error.to_string());
                    ERROR_QUERY
                })?;
        let mut row = DynamicRow::with_capacity(fields.len());
        for field in fields {
            let value = values.remove(&field).unwrap_or(serde_json::Value::Null);
            row.insert(field, json_value_to_cell(value)?);
        }
        let mut frame = Vec::new();
        write_u32(&mut frame, 1);
        write_row(&mut frame, &row)?;
        let handle = Box::new(BufferHandle { frame });
        unsafe {
            ptr::write(out_data, handle.frame.as_ptr());
            ptr::write(out_length, handle.frame.len());
            ptr::write(out_handle, Box::into_raw(handle));
        }
        Ok(RESULT_BATCH)
    })
}

fn json_value_to_cell(value: serde_json::Value) -> Result<CellValue, i32> {
    match value {
        serde_json::Value::Null => Ok(CellValue::Empty),
        serde_json::Value::Bool(value) => Ok(CellValue::Bool(value)),
        serde_json::Value::Number(value) => {
            if let Some(integer) = value.as_i64() {
                Ok(CellValue::Int(integer))
            } else if let Some(unsigned) = value.as_u64() {
                i64::try_from(unsigned).map(CellValue::Int).map_err(|_| {
                    set_last_error("mapped unsigned integer exceeds Int64");
                    ERROR_QUERY
                })
            } else {
                value.as_f64().map(CellValue::Float).ok_or_else(|| {
                    set_last_error("mapped JSON number is not representable");
                    ERROR_QUERY
                })
            }
        }
        serde_json::Value::String(value) => Ok(CellValue::String(value)),
        _ => {
            set_last_error("mapped cell produced a non-scalar JSON value");
            Err(ERROR_QUERY)
        }
    }
}

/// Creates a single-sheet XLSX workbook from encoded dynamic rows.
///
/// # Safety
///
/// `path`, `data`, and `out_row_count` must be non-null and valid for the supplied lengths.
/// `sheet_name` may be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_save_as(
    path: *const c_char,
    data: *const u8,
    data_length: usize,
    print_header: u8,
    sheet_name: *const c_char,
    overwrite_file: u8,
    out_row_count: *mut u32,
) -> i32 {
    ffi_result(|| {
        if path.is_null() || data.is_null() || out_row_count.is_null() {
            set_last_error("path, data, and out_row_count are required");
            return Err(ERROR_INVALID_ARGUMENT);
        }

        unsafe { ptr::write(out_row_count, 0) };
        let path = unsafe { read_utf8(path) }?;
        let bytes = unsafe { std::slice::from_raw_parts(data, data_length) };
        let rows = decode_rows(bytes)?;
        let mut options = WriteOptions::new()
            .with_print_header(print_header != 0)
            .with_overwrite_file(overwrite_file != 0);
        if !sheet_name.is_null() {
            let sheet_name = unsafe { read_utf8(sheet_name) }?;
            if !sheet_name.is_empty() {
                options = options.with_sheet_name(sheet_name);
            }
        }
        MiniExcel::save_as_with_options(path, &rows, &options).map_err(|error| {
            set_last_error(error.to_string());
            ERROR_WRITE
        })?;
        let row_count = u32::try_from(rows.len()).map_err(|_| {
            set_last_error("row count exceeds the ABI limit");
            ERROR_WRITE
        })?;
        unsafe { ptr::write(out_row_count, row_count) };
        Ok(RESULT_BATCH)
    })
}

/// Creates a multi-sheet XLSX workbook from an ordered encoded sheet collection.
///
/// # Safety
///
/// `path`, `data`, and all output pointers must be valid for the supplied lengths.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_save_as_sheets(
    path: *const c_char,
    data: *const u8,
    data_length: usize,
    print_header: u8,
    overwrite_file: u8,
    out_handle: *mut *mut BufferHandle,
    out_data: *mut *const u8,
    out_length: *mut usize,
) -> i32 {
    ffi_result(|| {
        if path.is_null()
            || data.is_null()
            || out_handle.is_null()
            || out_data.is_null()
            || out_length.is_null()
        {
            set_last_error("path, data, out_handle, out_data, and out_length are required");
            return Err(ERROR_INVALID_ARGUMENT);
        }
        unsafe {
            ptr::write(out_handle, ptr::null_mut());
            ptr::write(out_data, ptr::null());
            ptr::write(out_length, 0);
        }
        let path = unsafe { read_utf8(path) }?;
        let sheets = decode_sheets(unsafe { std::slice::from_raw_parts(data, data_length) })?;
        let options = WriteOptions::new()
            .with_print_header(print_header != 0)
            .with_overwrite_file(overwrite_file != 0);
        let counts = MiniExcel::save_as_sheets(
            path,
            sheets.iter().map(|(name, rows)| (name, rows.as_slice())),
            &options,
        )
        .map_err(|error| {
            set_last_error(error.to_string());
            ERROR_WRITE
        })?;
        let mut frame = Vec::new();
        write_length(&mut frame, counts.len())?;
        for count in counts {
            write_length(&mut frame, count)?;
        }
        let handle = Box::new(BufferHandle { frame });
        unsafe {
            ptr::write(out_data, handle.frame.as_ptr());
            ptr::write(out_length, handle.frame.len());
            ptr::write(out_handle, Box::into_raw(handle));
        }
        Ok(RESULT_BATCH)
    })
}

/// Creates an XLSX workbook from dynamic rows and a JSON write-options payload.
///
/// # Safety
///
/// `path`, `data`, `options_json`, and `out_row_count` must be valid for supplied lengths.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_save_as_configured(
    path: *const c_char,
    data: *const u8,
    data_length: usize,
    options_json: *const u8,
    options_length: usize,
    out_row_count: *mut u32,
) -> i32 {
    ffi_result(|| {
        if path.is_null() || data.is_null() || options_json.is_null() || out_row_count.is_null() {
            set_last_error("path, data, options_json, and out_row_count are required");
            return Err(ERROR_INVALID_ARGUMENT);
        }
        unsafe { ptr::write(out_row_count, 0) };
        let path = unsafe { read_utf8(path) }?;
        let mut rows = decode_rows(unsafe { std::slice::from_raw_parts(data, data_length) })?;
        let payload: serde_json::Value = serde_json::from_slice(unsafe {
            std::slice::from_raw_parts(options_json, options_length)
        })
        .map_err(|error| {
            set_last_error(format!("invalid write-options JSON: {error}"));
            ERROR_INVALID_ARGUMENT
        })?;
        let options = configured_write_options(&payload)?;
        let schema = configured_schema(&payload)?;
        let formula_columns = configured_formula_columns(&payload)?;
        if formula_columns.is_empty() {
            write_configured_workbook(path, &rows, schema.as_deref(), &options)?;
        } else {
            for row in &mut rows {
                for column in &formula_columns {
                    if let Some(value) = row.get_mut(column) {
                        let CellValue::String(formula) = value else {
                            set_last_error(format!(
                                "formula column '{column}' requires string values"
                            ));
                            return Err(ERROR_INVALID_ARGUMENT);
                        };
                        let formula = formula.strip_prefix('=').unwrap_or(formula);
                        *value = CellValue::String(format!("$={formula}"));
                    }
                }
            }
            let destination = Path::new(path);
            let parent = destination.parent().unwrap_or_else(|| Path::new("."));
            let staging = tempfile::Builder::new()
                .prefix(".miniexcel-formula-")
                .suffix(".xlsx")
                .tempfile_in(parent)
                .map_err(write_error)?
                .into_temp_path();
            std::fs::remove_file(&staging).map_err(write_error)?;
            let staging_options = options.clone().with_overwrite_file(false);
            let staging_path: &Path = staging.as_ref();
            write_configured_workbook(staging_path, &rows, schema.as_deref(), &staging_options)?;
            let template_options = TemplateOptions::new()
                .with_overwrite_file(json_bool(&payload, "overwriteFile", false)?)
                .with_ignore_missing_variables(true);
            MiniExcel::save_as_template(path, &staging, &serde_json::json!({}), &template_options)
                .map_err(|error| {
                    set_last_error(error.to_string());
                    ERROR_WRITE
                })?;
        }
        write_row_count(rows.len(), out_row_count)
    })
}

/// Creates a cancellable XLSX export by consuming framed rows from a spool file once.
///
/// # Safety
///
/// All pointers must be non-null, valid, and remain alive for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_save_as_spooled_async(
    path: *const c_char,
    spool_path: *const c_char,
    options_json: *const u8,
    options_length: usize,
    cancellation: *mut CancellationHandle,
    out_row_count: *mut u32,
) -> i32 {
    ffi_result(|| {
        if path.is_null()
            || spool_path.is_null()
            || options_json.is_null()
            || cancellation.is_null()
            || out_row_count.is_null()
        {
            set_last_error(
                "path, spool_path, options_json, cancellation, and out_row_count are required",
            );
            return Err(ERROR_INVALID_ARGUMENT);
        }
        unsafe { ptr::write(out_row_count, 0) };
        let path = unsafe { read_utf8(path) }?;
        let spool_path = unsafe { read_utf8(spool_path) }?;
        let payload: serde_json::Value = serde_json::from_slice(unsafe {
            std::slice::from_raw_parts(options_json, options_length)
        })
        .map_err(|error| {
            set_last_error(format!("invalid write-options JSON: {error}"));
            ERROR_INVALID_ARGUMENT
        })?;
        let schema = configured_schema(&payload)?.ok_or_else(|| {
            set_last_error("async spool export requires an explicit schema");
            ERROR_INVALID_ARGUMENT
        })?;
        let options = configured_write_options(&payload)?;
        let rows = SpoolRows::open(spool_path).map_err(write_error)?;
        let rows = futures_util::stream::iter(rows);
        let token = unsafe { &*cancellation }.token.clone();
        let count =
            futures_executor::block_on(MiniExcel::save_as_with_schema_async_with_cancellation(
                path, &schema, rows, &options, token,
            ))
            .map_err(|error| {
                set_last_error(error.to_string());
                ERROR_WRITE
            })?;
        write_row_count(count, out_row_count)
    })
}

/// Creates a cancellable CSV export by consuming framed rows from a spool file once.
///
/// # Safety
///
/// All pointers must be non-null, valid, and remain alive for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_save_csv_spooled_async(
    path: *const c_char,
    spool_path: *const c_char,
    options_json: *const u8,
    options_length: usize,
    cancellation: *mut CancellationHandle,
    out_row_count: *mut u32,
) -> i32 {
    ffi_result(|| {
        if path.is_null()
            || spool_path.is_null()
            || options_json.is_null()
            || cancellation.is_null()
            || out_row_count.is_null()
        {
            set_last_error(
                "path, spool_path, options_json, cancellation, and out_row_count are required",
            );
            return Err(ERROR_INVALID_ARGUMENT);
        }
        unsafe { ptr::write(out_row_count, 0) };
        let path = unsafe { read_utf8(path) }?;
        let spool_path = unsafe { read_utf8(spool_path) }?;
        let payload: serde_json::Value = serde_json::from_slice(unsafe {
            std::slice::from_raw_parts(options_json, options_length)
        })
        .map_err(|error| {
            set_last_error(format!("invalid CSV write-options JSON: {error}"));
            ERROR_INVALID_ARGUMENT
        })?;
        let schema = configured_schema(&payload)?.ok_or_else(|| {
            set_last_error("async CSV spool export requires an explicit schema");
            ERROR_INVALID_ARGUMENT
        })?;
        let configuration = CsvConfiguration::new()
            .with_delimiter(
                json_u64(&payload, "delimiter", b',' as u64)?
                    .try_into()
                    .map_err(|_| invalid_write_options("delimiter exceeds one byte"))?,
            )
            .with_encoding(parse_csv_encoding(
                json_u64(&payload, "encoding", 0)?
                    .try_into()
                    .map_err(|_| invalid_write_options("encoding exceeds one byte"))?,
            )?)
            .with_write_bom(json_bool(&payload, "writeBom", true)?);
        let options = CsvWriteOptions::new()
            .with_configuration(configuration)
            .with_print_header(json_bool(&payload, "printHeader", true)?)
            .with_overwrite_file(true);
        let overwrite = json_bool(&payload, "overwriteFile", false)?;
        let destination = Path::new(path);
        if destination.exists() && !overwrite {
            set_last_error(format!("destination '{}' already exists", destination.display()));
            return Err(ERROR_WRITE);
        }
        let parent = destination.parent().unwrap_or_else(|| Path::new("."));
        let staging = tempfile::Builder::new()
            .prefix(".miniexcel-csv-")
            .suffix(".csv")
            .tempfile_in(parent)
            .map_err(write_error)?
            .into_temp_path();
        let staging_path: &Path = staging.as_ref();
        let token = unsafe { &*cancellation }.token.clone();
        let rows = SpoolRows::open(spool_path).map_err(write_error)?;
        let mut count = 0_usize;
        for row in rows {
            if token.is_cancelled() {
                set_last_error("operation cancelled");
                return Err(ERROR_WRITE);
            }
            let row = row.map_err(|error| {
                set_last_error(error.to_string());
                ERROR_WRITE
            })?;
            if count == 0 {
                MiniExcel::save_csv_with_schema(staging_path, &schema, &[row], &options)
            } else {
                MiniExcel::append_csv_with_schema(staging_path, &schema, &[row], &options)
            }
            .map_err(|error| {
                set_last_error(error.to_string());
                ERROR_WRITE
            })?;
            count += 1;
        }
        if count == 0 {
            MiniExcel::save_csv_with_schema(staging_path, &schema, &[], &options).map_err(
                |error| {
                    set_last_error(error.to_string());
                    ERROR_WRITE
                },
            )?;
        }
        if token.is_cancelled() {
            set_last_error("operation cancelled");
            return Err(ERROR_WRITE);
        }
        publish_staged_file(staging_path, destination)?;
        write_row_count(count, out_row_count)
    })
}

#[unsafe(no_mangle)]
/// Creates a native cooperative cancellation handle.
///
/// # Safety
///
/// `out_handle` must be non-null and writable.
pub unsafe extern "C" fn miniexcel_cancellation_create(
    out_handle: *mut *mut CancellationHandle,
) -> i32 {
    ffi_result(|| {
        if out_handle.is_null() {
            set_last_error("out_handle is required");
            return Err(ERROR_INVALID_ARGUMENT);
        }
        let handle = Box::new(CancellationHandle { token: miniexcel::CancellationToken::new() });
        unsafe { ptr::write(out_handle, Box::into_raw(handle)) };
        Ok(RESULT_BATCH)
    })
}

#[unsafe(no_mangle)]
/// Signals cooperative cancellation.
///
/// # Safety
///
/// `handle` must be null or a live cancellation handle returned by this library.
pub unsafe extern "C" fn miniexcel_cancellation_cancel(handle: *mut CancellationHandle) {
    if !handle.is_null() {
        unsafe { &*handle }.token.cancel();
    }
}

#[unsafe(no_mangle)]
/// Releases a native cancellation handle.
///
/// # Safety
///
/// `handle` must be null or a live cancellation handle that has not already been closed.
pub unsafe extern "C" fn miniexcel_cancellation_close(handle: *mut CancellationHandle) {
    if !handle.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| drop(unsafe { Box::from_raw(handle) })));
    }
}

/// Creates a CSV file from encoded dynamic rows.
///
/// # Safety
///
/// `path`, `data`, and `out_row_count` must be non-null and valid for the supplied lengths.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_save_csv(
    path: *const c_char,
    data: *const u8,
    data_length: usize,
    delimiter: u8,
    encoding: u8,
    write_bom: u8,
    print_header: u8,
    overwrite_file: u8,
    out_row_count: *mut u32,
) -> i32 {
    ffi_result(|| unsafe {
        write_csv(
            CsvWriteArguments {
                path,
                data,
                data_length,
                delimiter,
                encoding,
                write_bom,
                print_header,
                overwrite_file,
            },
            false,
            out_row_count,
        )
    })
}

/// Appends encoded dynamic rows to a CSV file.
///
/// # Safety
///
/// `path`, `data`, and `out_row_count` must be non-null and valid for the supplied lengths.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_append_csv(
    path: *const c_char,
    data: *const u8,
    data_length: usize,
    delimiter: u8,
    encoding: u8,
    write_bom: u8,
    print_header: u8,
    out_row_count: *mut u32,
) -> i32 {
    ffi_result(|| unsafe {
        write_csv(
            CsvWriteArguments {
                path,
                data,
                data_length,
                delimiter,
                encoding,
                write_bom,
                print_header,
                overwrite_file: 0,
            },
            true,
            out_row_count,
        )
    })
}

/// Inserts or replaces a worksheet in an XLSX workbook.
///
/// # Safety
///
/// `path`, `data`, `sheet_name`, and `out_row_count` must be valid for the supplied lengths.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_insert_sheet(
    path: *const c_char,
    data: *const u8,
    data_length: usize,
    sheet_name: *const c_char,
    print_header: u8,
    replace_existing: u8,
    remove_supported_relationships: u8,
    out_row_count: *mut u32,
) -> i32 {
    ffi_result(|| {
        if path.is_null() || data.is_null() || sheet_name.is_null() || out_row_count.is_null() {
            set_last_error("path, data, sheet_name, and out_row_count are required");
            return Err(ERROR_INVALID_ARGUMENT);
        }
        unsafe { ptr::write(out_row_count, 0) };
        let path = unsafe { read_utf8(path) }?;
        let sheet_name = unsafe { read_utf8(sheet_name) }?;
        let rows = decode_rows(unsafe { std::slice::from_raw_parts(data, data_length) })?;
        let options = insert_options(
            sheet_name,
            print_header,
            replace_existing,
            remove_supported_relationships,
            false,
        );
        let count = MiniExcel::insert(path, &rows, &options).map_err(|error| {
            set_last_error(error.to_string());
            ERROR_WRITE
        })?;
        write_row_count(count, out_row_count)
    })
}

/// Copies an XLSX workbook and adds or replaces one worksheet in the destination.
///
/// # Safety
///
/// Both paths, `data`, `sheet_name`, and `out_row_count` must be valid for supplied lengths.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_copy_and_add_sheet(
    source_path: *const c_char,
    destination_path: *const c_char,
    data: *const u8,
    data_length: usize,
    sheet_name: *const c_char,
    print_header: u8,
    replace_existing: u8,
    remove_supported_relationships: u8,
    overwrite_destination: u8,
    out_row_count: *mut u32,
) -> i32 {
    ffi_result(|| {
        if source_path.is_null()
            || destination_path.is_null()
            || data.is_null()
            || sheet_name.is_null()
            || out_row_count.is_null()
        {
            set_last_error(
                "source_path, destination_path, data, sheet_name, and out_row_count are required",
            );
            return Err(ERROR_INVALID_ARGUMENT);
        }
        unsafe { ptr::write(out_row_count, 0) };
        let source_path = unsafe { read_utf8(source_path) }?;
        let destination_path = unsafe { read_utf8(destination_path) }?;
        let sheet_name = unsafe { read_utf8(sheet_name) }?;
        let rows = decode_rows(unsafe { std::slice::from_raw_parts(data, data_length) })?;
        let options = insert_options(
            sheet_name,
            print_header,
            replace_existing,
            remove_supported_relationships,
            overwrite_destination != 0,
        );
        let count = MiniExcel::copy_and_add_sheet(source_path, destination_path, &rows, &options)
            .map_err(|error| {
            set_last_error(error.to_string());
            ERROR_WRITE
        })?;
        write_row_count(count, out_row_count)
    })
}

/// Fills an XLSX template from a UTF-8 JSON value and atomically writes the destination.
///
/// # Safety
///
/// All string pointers must be non-null, valid, null-terminated UTF-8 for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_fill_template(
    destination_path: *const c_char,
    template_path: *const c_char,
    json_data: *const u8,
    json_length: usize,
    overwrite_file: u8,
    ignore_missing_variables: u8,
) -> i32 {
    ffi_result(|| {
        if destination_path.is_null() || template_path.is_null() || json_data.is_null() {
            set_last_error("destination_path, template_path, and json_data are required");
            return Err(ERROR_INVALID_ARGUMENT);
        }

        let destination_path = unsafe { read_utf8(destination_path) }?;
        let template_path = unsafe { read_utf8(template_path) }?;
        let json = unsafe { std::slice::from_raw_parts(json_data, json_length) };
        let value: serde_json::Value = serde_json::from_slice(json).map_err(|error| {
            set_last_error(format!("invalid template JSON: {error}"));
            ERROR_INVALID_ARGUMENT
        })?;
        let options = TemplateOptions::new()
            .with_overwrite_file(overwrite_file != 0)
            .with_ignore_missing_variables(ignore_missing_variables != 0);
        MiniExcel::save_as_template(destination_path, template_path, &value, &options).map_err(
            |error| {
                set_last_error(error.to_string());
                ERROR_WRITE
            },
        )?;
        Ok(RESULT_BATCH)
    })
}

/// Overlays an expanded fluent-mapping cell plan onto an XLSX template.
///
/// # Safety
///
/// All string pointers must be non-null, valid, null-terminated UTF-8 for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_fill_mapped_template(
    destination_path: *const c_char,
    template_path: *const c_char,
    json_data: *const u8,
    json_length: usize,
    overwrite_file: u8,
) -> i32 {
    ffi_result(|| {
        if destination_path.is_null() || template_path.is_null() || json_data.is_null() {
            set_last_error("destination_path, template_path, and json_data are required");
            return Err(ERROR_INVALID_ARGUMENT);
        }
        let destination_path = unsafe { read_utf8(destination_path) }?;
        let template_path = unsafe { read_utf8(template_path) }?;
        let payload: serde_json::Value =
            serde_json::from_slice(unsafe { std::slice::from_raw_parts(json_data, json_length) })
                .map_err(|error| {
                set_last_error(format!("invalid mapped template JSON: {error}"));
                ERROR_INVALID_ARGUMENT
            })?;
        fill_mapped_template(destination_path, template_path, &payload, overwrite_file != 0)?;
        Ok(RESULT_BATCH)
    })
}

/// Merges tagged same-value cells into a separate XLSX destination.
///
/// # Safety
///
/// Both path pointers must be non-null, valid, null-terminated UTF-8 for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_merge_same_cells(
    destination_path: *const c_char,
    source_path: *const c_char,
    overwrite_file: u8,
) -> i32 {
    ffi_result(|| {
        if destination_path.is_null() || source_path.is_null() {
            set_last_error("destination_path and source_path are required");
            return Err(ERROR_INVALID_ARGUMENT);
        }
        let destination_path = unsafe { read_utf8(destination_path) }?;
        let source_path = unsafe { read_utf8(source_path) }?;
        let options = MergeSameCellsOptions::new().with_overwrite_file(overwrite_file != 0);
        MiniExcel::merge_same_cells(source_path, destination_path, &options).map_err(|error| {
            set_last_error(error.to_string());
            ERROR_WRITE
        })?;
        Ok(RESULT_BATCH)
    })
}

/// Adds one PNG picture to an existing XLSX workbook.
///
/// # Safety
///
/// String and image pointers must be valid for the supplied lengths. `sheet_name` may be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_add_picture(
    path: *const c_char,
    sheet_name: *const c_char,
    cell_address: *const c_char,
    image_data: *const u8,
    image_length: usize,
    width_px: u32,
    height_px: u32,
    anchor_type: u8,
    location_x: i32,
    location_y: i32,
) -> i32 {
    ffi_result(|| {
        if path.is_null() || cell_address.is_null() || image_data.is_null() {
            set_last_error("path, cell_address, and image_data are required");
            return Err(ERROR_INVALID_ARGUMENT);
        }
        if image_length == 0 || width_px == 0 || height_px == 0 {
            set_last_error("image data, width, and height must be non-zero");
            return Err(ERROR_INVALID_ARGUMENT);
        }
        let path = unsafe { read_utf8(path) }?;
        let sheet_name = if sheet_name.is_null() {
            None
        } else {
            let value = unsafe { read_utf8(sheet_name) }?;
            (!value.is_empty()).then_some(value)
        };
        let cell_address = unsafe { read_utf8(cell_address) }?;
        let image = unsafe { std::slice::from_raw_parts(image_data, image_length) };
        add_png_picture(
            path,
            sheet_name,
            cell_address,
            image,
            width_px,
            height_px,
            anchor_type,
            location_x,
            location_y,
        )?;
        Ok(RESULT_BATCH)
    })
}

/// Atomically renames a worksheet in an existing XLSX workbook.
///
/// # Safety
///
/// All string pointers must be non-null, valid, null-terminated UTF-8 for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_rename_sheet(
    path: *const c_char,
    sheet_name: *const c_char,
    new_sheet_name: *const c_char,
) -> i32 {
    ffi_result(|| {
        if path.is_null() || sheet_name.is_null() || new_sheet_name.is_null() {
            set_last_error("path, sheet_name, and new_sheet_name are required");
            return Err(ERROR_INVALID_ARGUMENT);
        }
        let path = unsafe { read_utf8(path) }?;
        let sheet_name = unsafe { read_utf8(sheet_name) }?;
        let new_sheet_name = unsafe { read_utf8(new_sheet_name) }?;
        MiniExcel::rename_sheet(path, sheet_name, new_sheet_name).map_err(|error| {
            set_last_error(error.to_string());
            ERROR_WRITE
        })?;
        Ok(RESULT_BATCH)
    })
}

/// Atomically moves a worksheet to a zero-based index.
///
/// # Safety
///
/// Both string pointers must be non-null, valid, null-terminated UTF-8 for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_reorder_sheet(
    path: *const c_char,
    sheet_name: *const c_char,
    new_sheet_index: i32,
) -> i32 {
    ffi_result(|| {
        if path.is_null() || sheet_name.is_null() {
            set_last_error("path and sheet_name are required");
            return Err(ERROR_INVALID_ARGUMENT);
        }
        let path = unsafe { read_utf8(path) }?;
        let sheet_name = unsafe { read_utf8(sheet_name) }?;
        MiniExcel::reorder_sheet(path, sheet_name, new_sheet_index).map_err(|error| {
            set_last_error(error.to_string());
            ERROR_WRITE
        })?;
        Ok(RESULT_BATCH)
    })
}

/// Atomically changes a worksheet visibility state.
///
/// # Safety
///
/// Both string pointers must be non-null, valid, null-terminated UTF-8 for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_set_sheet_visibility(
    path: *const c_char,
    sheet_name: *const c_char,
    visibility: u8,
) -> i32 {
    ffi_result(|| {
        if path.is_null() || sheet_name.is_null() {
            set_last_error("path and sheet_name are required");
            return Err(ERROR_INVALID_ARGUMENT);
        }
        let visibility = match visibility {
            0 => SheetVisibility::Visible,
            1 => SheetVisibility::Hidden,
            2 => SheetVisibility::VeryHidden,
            _ => {
                set_last_error("visibility is not supported");
                return Err(ERROR_INVALID_ARGUMENT);
            }
        };
        let path = unsafe { read_utf8(path) }?;
        let sheet_name = unsafe { read_utf8(sheet_name) }?;
        MiniExcel::set_sheet_visibility(path, sheet_name, visibility).map_err(|error| {
            set_last_error(error.to_string());
            ERROR_WRITE
        })?;
        Ok(RESULT_BATCH)
    })
}

/// Releases a buffer returned by a metadata operation.
///
/// # Safety
///
/// `handle` must be null or a handle returned by this library that has not already been closed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn miniexcel_buffer_close(handle: *mut BufferHandle) {
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

unsafe fn open_query(
    arguments: QueryOpenOptions,
    out_handle: *mut *mut QueryHandle,
) -> Result<i32, i32> {
    let QueryOpenOptions {
        path,
        use_header_row,
        sheet_name,
        start_cell,
        end_cell,
        ignore_empty_rows,
        fill_merged_cells,
        trim_headers,
        enable_shared_string_cache,
        shared_string_cache_size,
        shared_string_cache_path,
    } = arguments;
    if path.is_null() || start_cell.is_null() || out_handle.is_null() {
        set_last_error("path, start_cell, and out_handle are required");
        return Err(ERROR_INVALID_ARGUMENT);
    }

    unsafe { ptr::write(out_handle, ptr::null_mut()) };
    let path = unsafe { read_utf8(path) }?;
    let start_cell_text = unsafe { read_utf8(start_cell) }?;
    let start_row = cell_row_index(start_cell_text)?;
    let start_column = cell_column_index(start_cell_text)?;
    let start_cell = CellReference::from_str(start_cell_text).map_err(|error| {
        set_last_error(error.to_string());
        ERROR_INVALID_ARGUMENT
    })?;

    let mut options = ReadOptions::new()
        .with_header_mode(if use_header_row == 0 { HeaderMode::None } else { HeaderMode::FirstRow })
        .with_start_cell(start_cell)
        .with_ignore_empty_rows(ignore_empty_rows != 0)
        .with_fill_merged_cells(fill_merged_cells != 0)
        .with_trim_headers(trim_headers != 0)
        .with_shared_string_disk_cache(enable_shared_string_cache != 0)
        .with_shared_string_cache_size(shared_string_cache_size);

    let mut end_row = None;
    if !end_cell.is_null() {
        let end_cell_text = unsafe { read_utf8(end_cell) }?;
        end_row = Some(cell_row_index(end_cell_text)?);
        let end_cell = CellReference::from_str(end_cell_text).map_err(|error| {
            set_last_error(error.to_string());
            ERROR_INVALID_ARGUMENT
        })?;
        options = options.with_end_cell(end_cell);
    }

    let mut selected_sheet_name = None;
    if !sheet_name.is_null() {
        let sheet_name = unsafe { read_utf8(sheet_name) }?;
        if !sheet_name.is_empty() {
            selected_sheet_name = Some(sheet_name.to_owned());
            options = options.with_sheet_name(sheet_name);
        }
    }

    if !shared_string_cache_path.is_null() {
        let cache_path = unsafe { read_utf8(shared_string_cache_path) }?;
        if !cache_path.is_empty() {
            options = options.with_shared_string_cache_path(cache_path);
        }
    }

    let rows = MiniExcel::query_with_options(path, &options).map_err(|error| {
        set_last_error(error.to_string());
        ERROR_QUERY
    })?;
    let rows: Box<dyn Iterator<Item = miniexcel::Result<DynamicRow>> + Send> = if ignore_empty_rows
        != 0
    {
        let columns = MiniExcel::get_columns(path, &options).map_err(|error| {
            set_last_error(error.to_string());
            ERROR_QUERY
        })?;
        let (mut pattern, merged_ranges) = worksheet_physical_row_pattern(
            path,
            selected_sheet_name.as_deref(),
            start_row,
            end_row,
        )?;
        if use_header_row != 0 {
            if let Some(header_index) =
                pattern.iter().position(|action| matches!(action, PhysicalRowAction::Data { .. }))
            {
                pattern.remove(header_index);
            }
        }
        Box::new(PhysicalRowIterator {
            inner: rows,
            pattern: pattern.into_iter(),
            columns,
            start_column,
            normalize_merged_cells: fill_merged_cells != 0,
            merge_anchor_values: vec![None; merged_ranges.len()],
            merged_ranges,
        })
    } else {
        rows
    };
    let handle = Box::new(QueryHandle { rows, frame: Vec::new() });
    unsafe { ptr::write(out_handle, Box::into_raw(handle)) };
    Ok(RESULT_BATCH)
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
                let value = if value.date()
                    == NaiveDate::from_ymd_opt(1899, 12, 31).expect("valid Excel epoch date")
                {
                    *value - Duration::days(1)
                } else {
                    *value
                };
                write_string(frame, value.format("%Y-%m-%dT%H:%M:%S%.f").to_string())?;
            }
            CellValue::Duration(value) => {
                frame.push(3);
                let excel_days = value.num_milliseconds() as f64 / 86_400_000_f64;
                frame.extend_from_slice(&excel_days.to_le_bytes());
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

fn write_strings(values: Vec<String>) -> Result<Vec<u8>, i32> {
    let mut frame = Vec::new();
    write_length(&mut frame, values.len())?;
    for value in values {
        write_string(&mut frame, value)?;
    }
    Ok(frame)
}

fn write_optional_string(frame: &mut Vec<u8>, value: Option<&str>) -> Result<(), i32> {
    frame.push(u8::from(value.is_some()));
    if let Some(value) = value {
        write_string(frame, value)?;
    }
    Ok(())
}

fn write_person(frame: &mut Vec<u8>, person: Option<&CommentPerson>) -> Result<(), i32> {
    frame.push(u8::from(person.is_some()));
    if let Some(person) = person {
        write_string(frame, person.id().to_string())?;
        write_string(frame, person.display_name())?;
        write_optional_string(frame, person.provider_id())?;
    }
    Ok(())
}

fn write_timestamp(frame: &mut Vec<u8>, timestamp: Option<&CommentTimestamp>) -> Result<(), i32> {
    let value = timestamp.map(|value| match value {
        CommentTimestamp::Local(value) => value.format("%Y-%m-%dT%H:%M:%S%.f").to_string(),
        CommentTimestamp::Offset(value) => value.to_rfc3339(),
    });
    write_optional_string(frame, value.as_deref())
}

fn csv_read_options(
    use_header_row: u8,
    delimiter: u8,
    encoding: u8,
    read_empty_as_null: u8,
    trim_headers: u8,
) -> Result<CsvReadOptions, i32> {
    if delimiter == 0 {
        set_last_error("delimiter must be a single-byte character");
        return Err(ERROR_INVALID_ARGUMENT);
    }
    let encoding = parse_csv_encoding(encoding)?;
    let configuration = CsvConfiguration::new()
        .with_delimiter(delimiter)
        .with_encoding(encoding)
        .with_read_empty_as_null(read_empty_as_null != 0);
    Ok(CsvReadOptions::new()
        .with_configuration(configuration)
        .with_header_mode(if use_header_row == 0 { HeaderMode::None } else { HeaderMode::FirstRow })
        .with_trim_headers(trim_headers != 0))
}

fn parse_csv_encoding(encoding: u8) -> Result<CsvEncoding, i32> {
    match encoding {
        0 => Ok(CsvEncoding::Utf8),
        1 => Ok(CsvEncoding::Utf16Le),
        2 => Ok(CsvEncoding::Utf16Be),
        3 => Ok(CsvEncoding::Gbk),
        4 => Ok(CsvEncoding::Windows1252),
        _ => {
            set_last_error("encoding is not supported");
            Err(ERROR_INVALID_ARGUMENT)
        }
    }
}

unsafe fn write_csv(
    arguments: CsvWriteArguments,
    append: bool,
    out_row_count: *mut u32,
) -> Result<i32, i32> {
    let CsvWriteArguments {
        path,
        data,
        data_length,
        delimiter,
        encoding,
        write_bom,
        print_header,
        overwrite_file,
    } = arguments;
    if path.is_null() || data.is_null() || out_row_count.is_null() {
        set_last_error("path, data, and out_row_count are required");
        return Err(ERROR_INVALID_ARGUMENT);
    }
    if delimiter == 0 {
        set_last_error("delimiter must be a single-byte character");
        return Err(ERROR_INVALID_ARGUMENT);
    }

    unsafe { ptr::write(out_row_count, 0) };
    let path = unsafe { read_utf8(path) }?;
    let rows = decode_rows(unsafe { std::slice::from_raw_parts(data, data_length) })?;
    let configuration = CsvConfiguration::new()
        .with_delimiter(delimiter)
        .with_encoding(parse_csv_encoding(encoding)?)
        .with_write_bom(write_bom != 0);
    let options = CsvWriteOptions::new()
        .with_configuration(configuration)
        .with_print_header(print_header != 0)
        .with_overwrite_file(overwrite_file != 0);
    let count = if append {
        MiniExcel::append_csv(path, &rows, &options)
    } else {
        MiniExcel::save_csv(path, &rows, &options)
    }
    .map_err(|error| {
        set_last_error(error.to_string());
        ERROR_WRITE
    })?;
    let count = u32::try_from(count).map_err(|_| {
        set_last_error("row count exceeds the ABI limit");
        ERROR_WRITE
    })?;
    unsafe { ptr::write(out_row_count, count) };
    Ok(RESULT_BATCH)
}

fn decode_rows(bytes: &[u8]) -> Result<Vec<DynamicRow>, i32> {
    let mut reader = FrameInput::new(bytes);
    let rows = read_rows(&mut reader)?;
    reader.ensure_complete()?;
    Ok(rows)
}

fn configured_schema(payload: &serde_json::Value) -> Result<Option<Vec<String>>, i32> {
    let Some(schema) = payload.get("schema") else {
        return Ok(None);
    };
    let values =
        schema.as_array().ok_or_else(|| invalid_write_options("schema must be an array"))?;
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid_write_options("schema values must be strings"))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn configured_formula_columns(payload: &serde_json::Value) -> Result<Vec<String>, i32> {
    let Some(columns) = payload.get("formulaColumns") else {
        return Ok(Vec::new());
    };
    let values = columns
        .as_array()
        .ok_or_else(|| invalid_write_options("formulaColumns must be an array"))?;
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid_write_options("formulaColumns values must be strings"))
        })
        .collect()
}

fn write_configured_workbook(
    path: impl AsRef<Path>,
    rows: &[DynamicRow],
    schema: Option<&[String]>,
    options: &WriteOptions,
) -> Result<(), i32> {
    match schema {
        Some(schema) => MiniExcel::save_as_with_schema(path, schema, rows, options),
        None => MiniExcel::save_as_with_options(path, rows, options),
    }
    .map_err(|error| {
        set_last_error(error.to_string());
        ERROR_WRITE
    })
}

fn write_error(error: impl std::fmt::Display) -> i32 {
    set_last_error(format!("failed to write output: {error}"));
    ERROR_WRITE
}

#[cfg(windows)]
fn publish_staged_file(source: &Path, destination: &Path) -> Result<(), i32> {
    atomicwrites::replace_atomic(source, destination).map_err(write_error)
}

#[cfg(not(windows))]
fn publish_staged_file(source: &Path, destination: &Path) -> Result<(), i32> {
    std::fs::rename(source, destination).map_err(write_error)
}

fn configured_write_options(payload: &serde_json::Value) -> Result<WriteOptions, i32> {
    let mut options = WriteOptions::new()
        .with_sheet_name(json_string(payload, "sheetName", "Sheet1")?)
        .with_overwrite_file(json_bool(payload, "overwriteFile", false)?)
        .with_print_header(json_bool(payload, "printHeader", true)?)
        .with_auto_filter(json_bool(payload, "autoFilter", true)?)
        .with_right_to_left(json_bool(payload, "rightToLeft", false)?)
        .with_auto_width(json_bool(payload, "autoWidth", false)?)
        .with_wrap_cell_contents(json_bool(payload, "wrapCellContents", false)?)
        .with_min_width(json_f64(payload, "minWidth", 8.42857143)?)
        .with_max_width(json_f64(payload, "maxWidth", 200.0)?)
        .with_freeze_row_count(
            json_u64(payload, "freezeRowCount", 1)?
                .try_into()
                .map_err(|_| invalid_write_options("freezeRowCount exceeds UInt32"))?,
        )
        .with_freeze_column_count(
            json_u64(payload, "freezeColumnCount", 0)?
                .try_into()
                .map_err(|_| invalid_write_options("freezeColumnCount exceeds UInt16"))?,
        )
        .with_horizontal_alignment(parse_horizontal_alignment(json_string(
            payload,
            "horizontalAlignment",
            "left",
        )?)?)
        .with_vertical_alignment(parse_vertical_alignment(json_string(
            payload,
            "verticalAlignment",
            "bottom",
        )?)?)
        .with_table_style(match json_string(payload, "tableStyle", "default")?.as_str() {
            "none" => TableStyle::None,
            "default" => TableStyle::Default,
            _ => return Err(invalid_write_options("tableStyle must be none or default")),
        });
    let header_style = HeaderStyle::new()
        .with_wrap_text(json_bool(payload, "headerWrapText", false)?)
        .with_background_color(parse_rgb_color(&json_string(
            payload,
            "headerBackgroundColor",
            "4472C4",
        )?)?)
        .with_horizontal_alignment(parse_horizontal_alignment(json_string(
            payload,
            "headerHorizontalAlignment",
            "left",
        )?)?)
        .with_vertical_alignment(parse_vertical_alignment(json_string(
            payload,
            "headerVerticalAlignment",
            "bottom",
        )?)?);
    options = options.with_header_style(header_style);
    for (property, setter) in
        [("dateFormat", 0_u8), ("timeFormat", 1), ("dateTimeFormat", 2), ("durationFormat", 3)]
    {
        if let Some(value) = payload.get(property).and_then(serde_json::Value::as_str) {
            options = match setter {
                0 => options.with_date_format(value),
                1 => options.with_time_format(value),
                2 => options.with_datetime_format(value),
                _ => options.with_duration_format(value),
            };
        }
    }
    if let Some(values) = payload.get("columnFormats").and_then(serde_json::Value::as_object) {
        for (name, value) in values {
            options = options.with_column_format(
                name,
                value
                    .as_str()
                    .ok_or_else(|| invalid_write_options("columnFormats values must be strings"))?,
            );
        }
    }
    if let Some(values) = payload.get("columnWidths").and_then(serde_json::Value::as_object) {
        for (name, value) in values {
            options = options.with_column_width(
                name,
                value
                    .as_f64()
                    .ok_or_else(|| invalid_write_options("columnWidths values must be numbers"))?,
            );
        }
    }
    if let Some(values) = payload.get("hiddenColumns").and_then(serde_json::Value::as_object) {
        for (name, value) in values {
            options = options.with_column_hidden(
                name,
                value.as_bool().ok_or_else(|| {
                    invalid_write_options("hiddenColumns values must be booleans")
                })?,
            );
        }
    }
    Ok(options)
}

fn parse_horizontal_alignment(value: String) -> Result<HorizontalAlignment, i32> {
    match value.as_str() {
        "left" => Ok(HorizontalAlignment::Left),
        "center" => Ok(HorizontalAlignment::Center),
        "right" => Ok(HorizontalAlignment::Right),
        _ => Err(invalid_write_options("horizontal alignment must be left, center, or right")),
    }
}

fn parse_vertical_alignment(value: String) -> Result<VerticalAlignment, i32> {
    match value.as_str() {
        "bottom" => Ok(VerticalAlignment::Bottom),
        "center" => Ok(VerticalAlignment::Center),
        "top" => Ok(VerticalAlignment::Top),
        _ => Err(invalid_write_options("vertical alignment must be bottom, center, or top")),
    }
}

fn parse_rgb_color(value: &str) -> Result<RgbColor, i32> {
    let value = value.strip_prefix('#').unwrap_or(value);
    if value.len() != 6 {
        return Err(invalid_write_options("headerBackgroundColor must be a six-digit RGB value"));
    }
    let color = u32::from_str_radix(value, 16)
        .map_err(|_| invalid_write_options("headerBackgroundColor is not valid hexadecimal"))?;
    Ok(RgbColor::new(
        ((color >> 16) & 0xff) as u8,
        ((color >> 8) & 0xff) as u8,
        (color & 0xff) as u8,
    ))
}

fn json_string(payload: &serde_json::Value, name: &str, default: &str) -> Result<String, i32> {
    match payload.get(name) {
        None => Ok(default.to_owned()),
        Some(value) => value
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| invalid_write_options(&format!("{name} must be a string"))),
    }
}

fn json_bool(payload: &serde_json::Value, name: &str, default: bool) -> Result<bool, i32> {
    match payload.get(name) {
        None => Ok(default),
        Some(value) => value
            .as_bool()
            .ok_or_else(|| invalid_write_options(&format!("{name} must be a boolean"))),
    }
}

fn json_f64(payload: &serde_json::Value, name: &str, default: f64) -> Result<f64, i32> {
    match payload.get(name) {
        None => Ok(default),
        Some(value) => {
            value.as_f64().ok_or_else(|| invalid_write_options(&format!("{name} must be a number")))
        }
    }
}

fn json_u64(payload: &serde_json::Value, name: &str, default: u64) -> Result<u64, i32> {
    match payload.get(name) {
        None => Ok(default),
        Some(value) => value.as_u64().ok_or_else(|| {
            invalid_write_options(&format!("{name} must be a non-negative integer"))
        }),
    }
}

fn invalid_write_options(message: &str) -> i32 {
    set_last_error(format!("invalid write options: {message}"));
    ERROR_INVALID_ARGUMENT
}

fn declared_sheet_dimensions(path: &str) -> Result<Vec<DeclaredDimension>, i32> {
    let file = File::open(path).map_err(metadata_error)?;
    let mut archive = ZipArchive::new(file).map_err(metadata_error)?;
    let workbook = read_zip_entry(&mut archive, "xl/workbook.xml")?;
    let relationships = read_zip_entry(&mut archive, "xl/_rels/workbook.xml.rels")?;
    let sheet_relationship_ids = workbook_sheet_relationships(&workbook)?;
    let relationship_targets = workbook_relationship_targets(&relationships)?;
    let mut dimensions = Vec::with_capacity(sheet_relationship_ids.len());
    for relationship_id in sheet_relationship_ids {
        let target = relationship_targets.get(&relationship_id).ok_or_else(|| {
            set_last_error(format!("workbook relationship '{relationship_id}' was not found"));
            ERROR_QUERY
        })?;
        let worksheet_path = normalize_workbook_target(target);
        let worksheet = read_zip_entry(&mut archive, &worksheet_path)?;
        dimensions.push(worksheet_declared_dimension(&worksheet)?);
    }
    Ok(dimensions)
}

fn worksheet_physical_row_pattern(
    path: &str,
    sheet_name: Option<&str>,
    start_row: usize,
    end_row: Option<usize>,
) -> Result<(Vec<PhysicalRowAction>, Vec<MergedRangeInfo>), i32> {
    let file = File::open(path).map_err(metadata_error)?;
    let mut archive = ZipArchive::new(file).map_err(metadata_error)?;
    let workbook = read_zip_entry(&mut archive, "xl/workbook.xml")?;
    let relationships = read_zip_entry(&mut archive, "xl/_rels/workbook.xml.rels")?;
    let sheets = workbook_sheets(&workbook)?;
    let relationship_id = match sheet_name {
        Some(name) => sheets
            .iter()
            .find(|(sheet, _)| sheet.eq_ignore_ascii_case(name))
            .map(|(_, relationship)| relationship),
        None => sheets.first().map(|(_, relationship)| relationship),
    }
    .ok_or_else(|| {
        set_last_error(format!("worksheet '{}' was not found", sheet_name.unwrap_or("<first>")));
        ERROR_QUERY
    })?;
    let targets = workbook_relationship_targets(&relationships)?;
    let target = targets.get(relationship_id).ok_or_else(|| {
        set_last_error(format!("workbook relationship '{relationship_id}' was not found"));
        ERROR_QUERY
    })?;
    let worksheet = read_zip_entry(&mut archive, &normalize_workbook_target(target))?;
    physical_row_pattern(&worksheet, start_row, end_row)
}

fn physical_row_pattern(
    worksheet: &[u8],
    start_row: usize,
    end_row: Option<usize>,
) -> Result<(Vec<PhysicalRowAction>, Vec<MergedRangeInfo>), i32> {
    let mut reader = XmlReader::from_reader(worksheet);
    let mut pattern = Vec::new();
    let mut current_row: Option<(usize, Vec<usize>)> = None;
    let mut last_row = 0_usize;
    let mut skip_next_data_row = false;
    let mut merged_ranges = Vec::new();
    loop {
        match reader.read_event().map_err(metadata_error)? {
            Event::Start(event) if event.local_name().as_ref() == b"row" => {
                let row = row_number(&reader, &event, last_row + 1)?;
                last_row = row;
                current_row = Some((row, Vec::new()));
            }
            Event::Empty(event) if event.local_name().as_ref() == b"row" => {
                let row = row_number(&reader, &event, last_row + 1)?;
                last_row = row;
                if row >= start_row && end_row.is_none_or(|end| row <= end) {
                    pattern.push(PhysicalRowAction::Empty);
                    skip_next_data_row = true;
                }
            }
            Event::Start(event) | Event::Empty(event) if event.local_name().as_ref() == b"c" => {
                if let Some((_, columns)) = current_row.as_mut() {
                    let column = xml_attribute(&reader, &event, b"r")?
                        .as_deref()
                        .map(cell_column_index)
                        .transpose()?
                        .unwrap_or(columns.len() + 1);
                    columns.push(column);
                }
            }
            Event::End(event) if event.local_name().as_ref() == b"row" => {
                if let Some((row, columns)) = current_row.take() {
                    if !columns.is_empty()
                        && row >= start_row
                        && end_row.is_none_or(|end| row <= end)
                    {
                        if skip_next_data_row {
                            skip_next_data_row = false;
                            pattern.push(PhysicalRowAction::Skip);
                        } else {
                            pattern.push(PhysicalRowAction::Data { row, columns });
                        }
                    }
                }
            }
            Event::Empty(event) if event.local_name().as_ref() == b"mergeCell" => {
                if let Some(reference) = xml_attribute(&reader, &event, b"ref")? {
                    merged_ranges.push(parse_merged_range(&reference)?);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok((pattern, merged_ranges))
}

fn parse_merged_range(reference: &str) -> Result<MergedRangeInfo, i32> {
    let (start, end) = reference.split_once(':').unwrap_or((reference, reference));
    Ok(MergedRangeInfo {
        start_row: cell_row_index(start)?,
        start_column: cell_column_index(start)?,
        end_row: cell_row_index(end)?,
        end_column: cell_column_index(end)?,
    })
}

fn row_number(
    reader: &XmlReader<&[u8]>,
    event: &BytesStart<'_>,
    fallback: usize,
) -> Result<usize, i32> {
    match xml_attribute(reader, event, b"r")? {
        Some(value) => value.parse().map_err(metadata_error),
        None => Ok(fallback),
    }
}

fn cell_row_index(reference: &str) -> Result<usize, i32> {
    let digits = reference.trim_start_matches(|character: char| character.is_ascii_alphabetic());
    digits.parse().map_err(|error| {
        set_last_error(format!("invalid cell row in '{reference}': {error}"));
        ERROR_INVALID_ARGUMENT
    })
}

fn cell_column_index(reference: &str) -> Result<usize, i32> {
    let mut column = 0_usize;
    for character in reference.chars().take_while(char::is_ascii_alphabetic) {
        column = column
            .checked_mul(26)
            .and_then(|value| {
                value.checked_add(character.to_ascii_uppercase() as usize - 'A' as usize + 1)
            })
            .ok_or_else(|| {
                set_last_error(format!("cell column in '{reference}' exceeds the supported range"));
                ERROR_INVALID_ARGUMENT
            })?;
    }
    if column == 0 {
        set_last_error(format!("invalid cell column in '{reference}'"));
        return Err(ERROR_INVALID_ARGUMENT);
    }
    Ok(column)
}

fn read_zip_entry<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    path: &str,
) -> Result<Vec<u8>, i32> {
    let mut entry = archive.by_name(path).map_err(metadata_error)?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).map_err(metadata_error)?;
    Ok(bytes)
}

fn workbook_sheet_relationships(workbook: &[u8]) -> Result<Vec<String>, i32> {
    let mut reader = XmlReader::from_reader(workbook);
    let mut relationships = Vec::new();
    loop {
        match reader.read_event().map_err(metadata_error)? {
            Event::Start(event) | Event::Empty(event)
                if event.local_name().as_ref() == b"sheet" =>
            {
                if let Some(value) = xml_attribute(&reader, &event, b"r:id")? {
                    relationships.push(value);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(relationships)
}

fn workbook_sheets(workbook: &[u8]) -> Result<Vec<(String, String)>, i32> {
    let mut reader = XmlReader::from_reader(workbook);
    let mut sheets = Vec::new();
    loop {
        match reader.read_event().map_err(metadata_error)? {
            Event::Start(event) | Event::Empty(event)
                if event.local_name().as_ref() == b"sheet" =>
            {
                if let (Some(name), Some(relationship)) = (
                    xml_attribute(&reader, &event, b"name")?,
                    xml_attribute(&reader, &event, b"r:id")?,
                ) {
                    sheets.push((name, relationship));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(sheets)
}

fn workbook_relationship_targets(relationships: &[u8]) -> Result<HashMap<String, String>, i32> {
    let mut reader = XmlReader::from_reader(relationships);
    let mut targets = HashMap::new();
    loop {
        match reader.read_event().map_err(metadata_error)? {
            Event::Start(event) | Event::Empty(event)
                if event.local_name().as_ref() == b"Relationship" =>
            {
                if let (Some(id), Some(target)) = (
                    xml_attribute(&reader, &event, b"Id")?,
                    xml_attribute(&reader, &event, b"Target")?,
                ) {
                    targets.insert(id, target);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(targets)
}

fn worksheet_declared_dimension(worksheet: &[u8]) -> Result<DeclaredDimension, i32> {
    let mut reader = XmlReader::from_reader(worksheet);
    loop {
        match reader.read_event().map_err(metadata_error)? {
            Event::Start(event) | Event::Empty(event)
                if event.local_name().as_ref() == b"dimension" =>
            {
                let reference = xml_attribute(&reader, &event, b"ref")?;
                return Ok(match reference {
                    Some(reference) => {
                        let (start, end) = reference
                            .split_once(':')
                            .map_or((reference.as_str(), reference.as_str()), |value| value);
                        (Some(start.to_owned()), Some(end.to_owned()))
                    }
                    None => (None, None),
                });
            }
            Event::Start(event) if event.local_name().as_ref() == b"sheetData" => {
                return Ok((None, None));
            }
            Event::Eof => return Ok((None, None)),
            _ => {}
        }
    }
}

fn xml_attribute(
    reader: &XmlReader<&[u8]>,
    event: &BytesStart<'_>,
    name: &[u8],
) -> Result<Option<String>, i32> {
    for attribute in event.attributes() {
        let attribute = attribute.map_err(metadata_error)?;
        if attribute.key.as_ref() == name {
            return attribute
                .decode_and_unescape_value(reader.decoder())
                .map(|value| Some(value.into_owned()))
                .map_err(metadata_error);
        }
    }
    Ok(None)
}

fn normalize_workbook_target(target: &str) -> String {
    let target = target.trim_start_matches('/');
    if target.starts_with("xl/") { target.to_owned() } else { format!("xl/{target}") }
}

fn metadata_error(error: impl std::fmt::Display) -> i32 {
    set_last_error(format!("failed to read XLSX metadata: {error}"));
    ERROR_QUERY
}

#[allow(clippy::too_many_arguments)]
fn add_png_picture(
    path: &str,
    sheet_name: Option<&str>,
    cell_address: &str,
    image: &[u8],
    width_px: u32,
    height_px: u32,
    anchor_type: u8,
    location_x: i32,
    location_y: i32,
) -> Result<(), i32> {
    const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    if !image.starts_with(PNG_SIGNATURE) {
        set_last_error("only PNG picture data is currently supported");
        return Err(ERROR_INVALID_ARGUMENT);
    }
    if anchor_type > 2 {
        set_last_error("anchor_type must be 0 (one-cell), 1 (absolute), or 2 (two-cell)");
        return Err(ERROR_INVALID_ARGUMENT);
    }
    let column = cell_column_index(cell_address)? - 1;
    let row = cell_row_index(cell_address)? - 1;
    let file = File::open(path).map_err(write_error)?;
    let mut archive = ZipArchive::new(file).map_err(write_error)?;
    let names = archive.file_names().map(str::to_owned).collect::<Vec<_>>();
    let workbook = read_zip_entry(&mut archive, "xl/workbook.xml")?;
    let workbook_rels = read_zip_entry(&mut archive, "xl/_rels/workbook.xml.rels")?;
    let sheets = workbook_sheets(&workbook)?;
    let relationship_id = match sheet_name {
        Some(name) => sheets
            .iter()
            .find(|(sheet, _)| sheet.eq_ignore_ascii_case(name))
            .map(|(_, relationship)| relationship),
        None => sheets.first().map(|(_, relationship)| relationship),
    }
    .ok_or_else(|| {
        set_last_error(format!("worksheet '{}' was not found", sheet_name.unwrap_or("<first>")));
        ERROR_QUERY
    })?;
    let targets = workbook_relationship_targets(&workbook_rels)?;
    let worksheet_path =
        normalize_workbook_target(targets.get(relationship_id).ok_or_else(|| {
            set_last_error(format!("workbook relationship '{relationship_id}' was not found"));
            ERROR_QUERY
        })?);
    let worksheet_name = worksheet_path.rsplit('/').next().ok_or_else(|| {
        set_last_error("worksheet path has no file name");
        ERROR_QUERY
    })?;
    let worksheet_rels_path = format!("xl/worksheets/_rels/{worksheet_name}.rels");
    let mut worksheet_xml = String::from_utf8(read_zip_entry(&mut archive, &worksheet_path)?)
        .map_err(metadata_error)?;
    let mut worksheet_rels = read_optional_zip_entry(&mut archive, &worksheet_rels_path)?
        .map(String::from_utf8)
        .transpose()
        .map_err(metadata_error)?
        .unwrap_or_else(empty_relationships_xml);

    let existing_drawing_id = drawing_relationship_id(worksheet_xml.as_bytes())?;
    let (drawing_path, drawing_rel_id) = if let Some(id) = existing_drawing_id {
        let targets = workbook_relationship_targets(worksheet_rels.as_bytes())?;
        let target = targets.get(&id).ok_or_else(|| {
            set_last_error(format!("worksheet drawing relationship '{id}' was not found"));
            ERROR_QUERY
        })?;
        (normalize_part_target(&worksheet_path, target), id)
    } else {
        let index = next_numbered_part(&names, "xl/drawings/drawing", ".xml");
        let drawing_path = format!("xl/drawings/drawing{index}.xml");
        let relationship_id = next_relationship_id(worksheet_rels.as_bytes())?;
        worksheet_xml = ensure_relationship_namespace(&worksheet_xml);
        worksheet_xml = insert_before(
            &worksheet_xml,
            "</worksheet>",
            &format!("<drawing r:id=\"{relationship_id}\"/>"),
        )?;
        worksheet_rels = append_relationship(
            &worksheet_rels,
            &relationship_id,
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing",
            &format!("../drawings/drawing{index}.xml"),
        )?;
        (drawing_path, relationship_id)
    };
    let _ = drawing_rel_id;

    let drawing_name = drawing_path.rsplit('/').next().expect("drawing file name");
    let drawing_rels_path = format!("xl/drawings/_rels/{drawing_name}.rels");
    let mut drawing_xml = read_optional_zip_entry(&mut archive, &drawing_path)?
        .map(String::from_utf8)
        .transpose()
        .map_err(metadata_error)?
        .unwrap_or_else(empty_drawing_xml);
    let mut drawing_rels = read_optional_zip_entry(&mut archive, &drawing_rels_path)?
        .map(String::from_utf8)
        .transpose()
        .map_err(metadata_error)?
        .unwrap_or_else(empty_relationships_xml);
    let image_index = next_numbered_part(&names, "xl/media/image", ".png");
    let image_path = format!("xl/media/image{image_index}.png");
    let image_rel_id = next_relationship_id(drawing_rels.as_bytes())?;
    drawing_rels = append_relationship(
        &drawing_rels,
        &image_rel_id,
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image",
        &format!("../media/image{image_index}.png"),
    )?;
    let picture_id = drawing_anchor_count(drawing_xml.as_bytes())? + 2;
    let anchor = picture_anchor_xml(
        column,
        row,
        width_px,
        height_px,
        &image_rel_id,
        picture_id,
        anchor_type,
        location_x,
        location_y,
    );
    drawing_xml = insert_before(&drawing_xml, "</xdr:wsDr>", &anchor)?;

    let mut content_types = String::from_utf8(read_zip_entry(&mut archive, "[Content_Types].xml")?)
        .map_err(metadata_error)?;
    if !content_types.contains("ContentType=\"image/png\"") {
        content_types = insert_before(
            &content_types,
            "</Types>",
            "<Default Extension=\"png\" ContentType=\"image/png\"/>",
        )?;
    }
    let drawing_part = format!("/{drawing_path}");
    if !content_types.contains(&drawing_part) {
        content_types = insert_before(
            &content_types,
            "</Types>",
            &format!(
                "<Override PartName=\"{drawing_part}\" ContentType=\"application/vnd.openxmlformats-officedocument.drawing+xml\"/>"
            ),
        )?;
    }

    let mut replacements = BTreeMap::new();
    replacements.insert(worksheet_path, worksheet_xml.into_bytes());
    replacements.insert(worksheet_rels_path, worksheet_rels.into_bytes());
    replacements.insert(drawing_path, drawing_xml.into_bytes());
    replacements.insert(drawing_rels_path, drawing_rels.into_bytes());
    replacements.insert("[Content_Types].xml".to_owned(), content_types.into_bytes());
    replacements.insert(image_path, image.to_vec());
    rewrite_package(path, archive, replacements)
}

fn read_optional_zip_entry<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    path: &str,
) -> Result<Option<Vec<u8>>, i32> {
    match archive.by_name(path) {
        Ok(mut entry) => {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(write_error)?;
            Ok(Some(bytes))
        }
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(error) => Err(write_error(error)),
    }
}

fn rewrite_package(
    path: &str,
    archive: ZipArchive<File>,
    replacements: BTreeMap<String, Vec<u8>>,
) -> Result<(), i32> {
    write_rewritten_package(path, archive, replacements, true)
}

fn write_rewritten_package(
    path: &str,
    mut archive: ZipArchive<File>,
    replacements: BTreeMap<String, Vec<u8>>,
    overwrite: bool,
) -> Result<(), i32> {
    let destination = Path::new(path);
    if destination.exists() && !overwrite {
        return Err(write_error("the destination file already exists"));
    }
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::Builder::new()
        .prefix(".miniexcel-package-")
        .suffix(".xlsx")
        .tempfile_in(parent)
        .map_err(write_error)?;
    {
        let mut writer = ZipWriter::new(temporary.as_file_mut());
        let mut written = std::collections::HashSet::new();
        for index in 0..archive.len() {
            let entry = archive.by_index_raw(index).map_err(write_error)?;
            let name = entry.name().to_owned();
            if let Some(replacement) = replacements.get(&name) {
                writer.start_file(&name, entry.options()).map_err(write_error)?;
                writer.write_all(replacement).map_err(write_error)?;
            } else {
                writer.raw_copy_file(entry).map_err(write_error)?;
            }
            written.insert(name);
        }
        for (name, bytes) in &replacements {
            if !written.contains(name) {
                writer
                    .start_file(
                        name,
                        SimpleFileOptions::default()
                            .compression_method(CompressionMethod::Deflated),
                    )
                    .map_err(write_error)?;
                writer.write_all(bytes).map_err(write_error)?;
            }
        }
        writer.finish().map_err(write_error)?;
    }
    drop(archive);
    temporary.as_file().sync_all().map_err(write_error)?;
    if overwrite {
        let staging = temporary.into_temp_path();
        publish_staged_file(staging.as_ref(), destination)
    } else {
        temporary.persist(destination).map_err(|error| write_error(error.error))?;
        Ok(())
    }
}

fn fill_mapped_template(
    destination_path: &str,
    template_path: &str,
    payload: &serde_json::Value,
    overwrite: bool,
) -> Result<(), i32> {
    let sheet_name = payload
        .get("sheetName")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid_write_options("sheetName is required"))?;
    let cells = payload
        .get("cells")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| invalid_write_options("cells must be an array"))?;
    let file = File::open(template_path).map_err(write_error)?;
    let mut archive = ZipArchive::new(file).map_err(write_error)?;
    let workbook = read_zip_entry(&mut archive, "xl/workbook.xml")?;
    let workbook_rels = read_zip_entry(&mut archive, "xl/_rels/workbook.xml.rels")?;
    let relationship_id = workbook_sheets(&workbook)?
        .into_iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(sheet_name))
        .map(|(_, relationship)| relationship)
        .ok_or_else(|| {
            set_last_error(format!("worksheet '{sheet_name}' was not found"));
            ERROR_QUERY
        })?;
    let targets = workbook_relationship_targets(&workbook_rels)?;
    let worksheet_path =
        normalize_workbook_target(targets.get(&relationship_id).ok_or_else(|| {
            set_last_error(format!("workbook relationship '{relationship_id}' was not found"));
            ERROR_QUERY
        })?);
    let mut worksheet =
        String::from_utf8(read_zip_entry(&mut archive, &worksheet_path)?).map_err(write_error)?;
    let mut ordered_cells = cells.iter().collect::<Vec<_>>();
    ordered_cells.sort_by_key(|cell| {
        let address = cell.get("address").and_then(serde_json::Value::as_str).unwrap_or_default();
        (
            cell_row_index(address).unwrap_or(usize::MAX),
            cell_column_index(address).unwrap_or(usize::MAX),
        )
    });
    for cell in ordered_cells {
        let address = cell
            .get("address")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| invalid_write_options("each mapped cell requires an address"))?;
        let row = cell_row_index(address)?;
        let formula = cell.get("formula").and_then(serde_json::Value::as_bool).unwrap_or(false);
        let value = cell.get("value").unwrap_or(&serde_json::Value::Null);
        worksheet = upsert_worksheet_cell(&worksheet, address, row, value, formula)?;
    }
    let mut replacements = BTreeMap::new();
    replacements.insert(worksheet_path, worksheet.into_bytes());
    write_rewritten_package(destination_path, archive, replacements, overwrite)
}

fn upsert_worksheet_cell(
    worksheet: &str,
    address: &str,
    row: usize,
    value: &serde_json::Value,
    formula: bool,
) -> Result<String, i32> {
    if let Some((start, end, style)) = find_cell_element(worksheet, address) {
        let cell = mapped_cell_xml(address, value, formula, style.as_deref());
        return Ok(format!("{}{}{}", &worksheet[..start], cell, &worksheet[end..]));
    }
    let cell = mapped_cell_xml(address, value, formula, None);
    if let Some((start, tag_end, end, empty)) = find_row_element(worksheet, row) {
        if empty {
            let start_tag = worksheet[start..tag_end - 1].trim_end_matches('/');
            let replacement = format!("{start_tag}>{cell}</row>");
            return Ok(format!("{}{}{}", &worksheet[..start], replacement, &worksheet[end..]));
        }
        let insert_at = worksheet[start..end]
            .rfind("</row>")
            .map(|index| start + index)
            .ok_or_else(|| write_error(format!("row {row} has no closing tag")))?;
        return Ok(format!("{}{}{}", &worksheet[..insert_at], cell, &worksheet[insert_at..]));
    }
    let new_row = format!("<row r=\"{row}\">{cell}</row>");
    if let Some(insert_at) = worksheet.find("</sheetData>") {
        return Ok(format!("{}{}{}", &worksheet[..insert_at], new_row, &worksheet[insert_at..]));
    }
    if let Some(start) = worksheet.find("<sheetData") {
        let tag_end = start
            + worksheet[start..]
                .find('>')
                .ok_or_else(|| write_error("invalid sheetData element"))?
            + 1;
        if worksheet[start..tag_end].trim_end().ends_with("/>") {
            return Ok(format!(
                "{}<sheetData>{new_row}</sheetData>{}",
                &worksheet[..start],
                &worksheet[tag_end..]
            ));
        }
    }
    let insert_at = worksheet
        .find("</worksheet>")
        .ok_or_else(|| write_error("worksheet has no closing element"))?;
    let sheet_data = format!("<sheetData>{new_row}</sheetData>");
    Ok(format!("{}{}{}", &worksheet[..insert_at], sheet_data, &worksheet[insert_at..]))
}

fn find_cell_element(worksheet: &str, address: &str) -> Option<(usize, usize, Option<String>)> {
    let attribute = format!("r=\"{address}\"");
    let mut offset = 0;
    while let Some(relative) = worksheet[offset..].find("<c") {
        let start = offset + relative;
        let boundary = worksheet.as_bytes().get(start + 2).copied()?;
        if !boundary.is_ascii_whitespace() && boundary != b'>' && boundary != b'/' {
            offset = start + 2;
            continue;
        }
        let tag_end = start + worksheet[start..].find('>')? + 1;
        let tag = &worksheet[start..tag_end];
        if !tag.contains(&attribute) {
            offset = tag_end;
            continue;
        }
        let style = xml_tag_attribute(tag, "s");
        let end = if tag.trim_end().ends_with("/>") {
            tag_end
        } else {
            tag_end + worksheet[tag_end..].find("</c>")? + 4
        };
        return Some((start, end, style));
    }
    None
}

fn find_row_element(worksheet: &str, row: usize) -> Option<(usize, usize, usize, bool)> {
    let attribute = format!("r=\"{row}\"");
    let mut offset = 0;
    while let Some(relative) = worksheet[offset..].find("<row") {
        let start = offset + relative;
        let tag_end = start + worksheet[start..].find('>')? + 1;
        let tag = &worksheet[start..tag_end];
        if !tag.contains(&attribute) {
            offset = tag_end;
            continue;
        }
        let empty = tag.trim_end().ends_with("/>");
        let end = if empty { tag_end } else { tag_end + worksheet[tag_end..].find("</row>")? + 6 };
        return Some((start, tag_end, end, empty));
    }
    None
}

fn xml_tag_attribute(tag: &str, name: &str) -> Option<String> {
    let prefix = format!("{name}=\"");
    let start = tag.find(&prefix)? + prefix.len();
    let end = start + tag[start..].find('"')?;
    Some(tag[start..end].to_owned())
}

fn mapped_cell_xml(
    address: &str,
    value: &serde_json::Value,
    formula: bool,
    style: Option<&str>,
) -> String {
    let style = style.map_or_else(String::new, |value| format!(" s=\"{value}\""));
    if formula {
        let formula = value.as_str().unwrap_or_default().trim_start_matches('=');
        return format!("<c r=\"{address}\"{style}><f>{}</f></c>", xml_escape(formula));
    }
    match value {
        serde_json::Value::Null => format!("<c r=\"{address}\"{style}/>"),
        serde_json::Value::Bool(value) => {
            format!("<c r=\"{address}\"{style} t=\"b\"><v>{}</v></c>", u8::from(*value))
        }
        serde_json::Value::Number(value) => {
            format!("<c r=\"{address}\"{style}><v>{value}</v></c>")
        }
        value => {
            let text = value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string());
            format!(
                "<c r=\"{address}\"{style} t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
                xml_escape(&text)
            )
        }
    }
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn drawing_relationship_id(worksheet: &[u8]) -> Result<Option<String>, i32> {
    let mut reader = XmlReader::from_reader(worksheet);
    loop {
        match reader.read_event().map_err(metadata_error)? {
            Event::Start(event) | Event::Empty(event)
                if event.local_name().as_ref() == b"drawing" =>
            {
                return xml_attribute(&reader, &event, b"r:id");
            }
            Event::Eof => return Ok(None),
            _ => {}
        }
    }
}

fn drawing_anchor_count(drawing: &[u8]) -> Result<usize, i32> {
    let mut reader = XmlReader::from_reader(drawing);
    let mut count = 0;
    loop {
        match reader.read_event().map_err(metadata_error)? {
            Event::Start(event)
                if matches!(
                    event.local_name().as_ref(),
                    b"oneCellAnchor" | b"twoCellAnchor" | b"absoluteAnchor"
                ) =>
            {
                count += 1
            }
            Event::Eof => return Ok(count),
            _ => {}
        }
    }
}

fn next_relationship_id(relationships: &[u8]) -> Result<String, i32> {
    let targets = workbook_relationship_targets(relationships)?;
    let mut index = 1;
    loop {
        let candidate = format!("rId{index}");
        if !targets.contains_key(&candidate) {
            return Ok(candidate);
        }
        index += 1;
    }
}

fn next_numbered_part(names: &[String], prefix: &str, suffix: &str) -> usize {
    let mut index = 1;
    loop {
        let candidate = format!("{prefix}{index}{suffix}");
        if !names.iter().any(|name| name == &candidate) {
            return index;
        }
        index += 1;
    }
}

fn append_relationship(
    xml: &str,
    id: &str,
    relationship_type: &str,
    target: &str,
) -> Result<String, i32> {
    insert_before(
        xml,
        "</Relationships>",
        &format!("<Relationship Id=\"{id}\" Type=\"{relationship_type}\" Target=\"{target}\"/>"),
    )
}

fn insert_before(xml: &str, closing: &str, value: &str) -> Result<String, i32> {
    let index = xml.rfind(closing).ok_or_else(|| {
        set_last_error(format!("XML closing element '{closing}' was not found"));
        ERROR_WRITE
    })?;
    let mut result = String::with_capacity(xml.len() + value.len());
    result.push_str(&xml[..index]);
    result.push_str(value);
    result.push_str(&xml[index..]);
    Ok(result)
}

fn ensure_relationship_namespace(xml: &str) -> String {
    if xml.contains("xmlns:r=") {
        return xml.to_owned();
    }
    xml.replacen(
        "<worksheet ",
        "<worksheet xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" ",
        1,
    )
}

fn normalize_part_target(source_part: &str, target: &str) -> String {
    let mut parts = source_part
        .rsplit_once('/')
        .map_or(Vec::new(), |(parent, _)| parent.split('/').collect::<Vec<_>>());
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            value => parts.push(value),
        }
    }
    parts.join("/")
}

fn empty_relationships_xml() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>".to_owned()
}

fn empty_drawing_xml() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><xdr:wsDr xmlns:xdr=\"http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"></xdr:wsDr>".to_owned()
}

#[allow(clippy::too_many_arguments)]
fn picture_anchor_xml(
    column: usize,
    row: usize,
    width_px: u32,
    height_px: u32,
    relationship_id: &str,
    picture_id: usize,
    anchor_type: u8,
    location_x: i32,
    location_y: i32,
) -> String {
    let extent = format!(
        "<xdr:ext cx=\"{}\" cy=\"{}\"/>",
        u64::from(width_px) * 9525,
        u64::from(height_px) * 9525
    );
    let position = match anchor_type {
        1 => format!(
            "<xdr:pos x=\"{}\" y=\"{}\"/>{extent}",
            i64::from(location_x) * 9525,
            i64::from(location_y) * 9525
        ),
        2 => format!(
            "<xdr:from><xdr:col>{column}</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>{row}</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>{}</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>",
            column + 1,
            row + 1
        ),
        _ => format!(
            "<xdr:from><xdr:col>{column}</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>{row}</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from>{extent}"
        ),
    };
    let anchor = match anchor_type {
        1 => "absoluteAnchor",
        2 => "twoCellAnchor",
        _ => "oneCellAnchor",
    };
    let edit_as = if anchor_type == 2 { " editAs=\"twoCell\"" } else { "" };
    format!(
        "<xdr:{anchor}{edit_as}>{position}<xdr:pic><xdr:nvPicPr><xdr:cNvPr id=\"{picture_id}\" name=\"Image{picture_id}\"/><xdr:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></xdr:cNvPicPr></xdr:nvPicPr><xdr:blipFill><a:blip r:embed=\"{relationship_id}\" cstate=\"print\"/><a:stretch><a:fillRect/></a:stretch></xdr:blipFill><xdr:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></xdr:spPr></xdr:pic><xdr:clientData/></xdr:{anchor}>"
    )
}

fn decode_sheets(bytes: &[u8]) -> Result<Vec<(String, Vec<DynamicRow>)>, i32> {
    let mut reader = FrameInput::new(bytes);
    let sheet_count = reader.read_length()?;
    let mut sheets = Vec::with_capacity(sheet_count);
    for _ in 0..sheet_count {
        sheets.push((reader.read_string()?, read_rows(&mut reader)?));
    }
    reader.ensure_complete()?;
    Ok(sheets)
}

fn read_rows(reader: &mut FrameInput<'_>) -> Result<Vec<DynamicRow>, i32> {
    let row_count = reader.read_length()?;
    let mut rows = Vec::with_capacity(row_count);
    for _ in 0..row_count {
        let cell_count = reader.read_length()?;
        let mut row = DynamicRow::with_capacity(cell_count);
        for _ in 0..cell_count {
            let name = reader.read_string()?;
            let value = match reader.read_byte()? {
                0 => CellValue::Empty,
                1 => CellValue::Bool(reader.read_byte()? != 0),
                2 => CellValue::Int(reader.read_i64()?),
                3 => CellValue::Float(f64::from_bits(reader.read_u64()?)),
                4 => CellValue::String(reader.read_string()?),
                5 => CellValue::Date(
                    NaiveDate::parse_from_str(&reader.read_string()?, "%Y-%m-%d")
                        .map_err(invalid_frame_value)?,
                ),
                6 => CellValue::Time(
                    NaiveTime::parse_from_str(&reader.read_string()?, "%H:%M:%S%.f")
                        .map_err(invalid_frame_value)?,
                ),
                7 => CellValue::DateTime(
                    NaiveDateTime::parse_from_str(&reader.read_string()?, "%Y-%m-%dT%H:%M:%S%.f")
                        .map_err(invalid_frame_value)?,
                ),
                8 => CellValue::Duration(Duration::milliseconds(reader.read_i64()?)),
                9 => CellValue::Error(reader.read_string()?),
                tag => {
                    set_last_error(format!("input frame contains unsupported value tag {tag}"));
                    return Err(ERROR_INVALID_ARGUMENT);
                }
            };
            row.insert(name, value);
        }
        rows.push(row);
    }
    Ok(rows)
}

fn invalid_frame_value(error: chrono::ParseError) -> i32 {
    set_last_error(format!("input frame contains an invalid temporal value: {error}"));
    ERROR_INVALID_ARGUMENT
}

fn insert_options(
    sheet_name: &str,
    print_header: u8,
    replace_existing: u8,
    remove_supported_relationships: u8,
    overwrite_file: bool,
) -> InsertOptions {
    InsertOptions::new()
        .with_sheet_name(sheet_name)
        .with_print_header(print_header != 0)
        .with_existing_sheet_policy(if replace_existing == 0 {
            ExistingSheetPolicy::Reject
        } else {
            ExistingSheetPolicy::Replace
        })
        .with_target_relationship_policy(if remove_supported_relationships == 0 {
            TargetRelationshipPolicy::Reject
        } else {
            TargetRelationshipPolicy::RemoveSupported
        })
        .with_overwrite_file(overwrite_file)
}

fn write_row_count(count: usize, out_row_count: *mut u32) -> Result<i32, i32> {
    let count = u32::try_from(count).map_err(|_| {
        set_last_error("row count exceeds the ABI limit");
        ERROR_WRITE
    })?;
    unsafe { ptr::write(out_row_count, count) };
    Ok(RESULT_BATCH)
}

struct FrameInput<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> FrameInput<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn read_byte(&mut self) -> Result<u8, i32> {
        self.ensure_available(1)?;
        let value = self.bytes[self.offset];
        self.offset += 1;
        Ok(value)
    }

    fn read_u32(&mut self) -> Result<u32, i32> {
        self.ensure_available(4)?;
        let mut value = [0_u8; 4];
        value.copy_from_slice(&self.bytes[self.offset..self.offset + 4]);
        self.offset += 4;
        Ok(u32::from_le_bytes(value))
    }

    fn read_u64(&mut self) -> Result<u64, i32> {
        self.ensure_available(8)?;
        let mut value = [0_u8; 8];
        value.copy_from_slice(&self.bytes[self.offset..self.offset + 8]);
        self.offset += 8;
        Ok(u64::from_le_bytes(value))
    }

    fn read_i64(&mut self) -> Result<i64, i32> {
        self.read_u64().map(|value| i64::from_le_bytes(value.to_le_bytes()))
    }

    fn read_length(&mut self) -> Result<usize, i32> {
        self.read_u32().map(|value| value as usize)
    }

    fn read_string(&mut self) -> Result<String, i32> {
        let length = self.read_length()?;
        self.ensure_available(length)?;
        let value = std::str::from_utf8(&self.bytes[self.offset..self.offset + length])
            .map_err(|error| {
                set_last_error(error.to_string());
                ERROR_INVALID_ARGUMENT
            })?
            .to_owned();
        self.offset += length;
        Ok(value)
    }

    fn ensure_complete(&self) -> Result<(), i32> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            set_last_error("input frame contains trailing data");
            Err(ERROR_INVALID_ARGUMENT)
        }
    }

    fn ensure_available(&self, length: usize) -> Result<(), i32> {
        if self.offset <= self.bytes.len().saturating_sub(length) {
            Ok(())
        } else {
            set_last_error("input frame is truncated");
            Err(ERROR_INVALID_ARGUMENT)
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_the_supported_abi_version() {
        assert_eq!(miniexcel_abi_version(), 1);
    }

    #[test]
    fn rejects_missing_required_query_arguments() {
        let result = unsafe {
            miniexcel_query_open(ptr::null(), 0, ptr::null(), ptr::null(), ptr::null_mut())
        };

        assert_eq!(result, ERROR_INVALID_ARGUMENT);

        let mut length = 0;
        let error = unsafe { miniexcel_last_error(&mut length) };
        let message = unsafe { std::slice::from_raw_parts(error, length) };
        assert_eq!(message, b"path, start_cell, and out_handle are required");
    }

    #[test]
    fn rejects_missing_required_sheet_name_arguments() {
        let result = unsafe {
            miniexcel_get_sheet_names(
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };

        assert_eq!(result, ERROR_INVALID_ARGUMENT);

        let mut length = 0;
        let error = unsafe { miniexcel_last_error(&mut length) };
        let message = unsafe { std::slice::from_raw_parts(error, length) };
        assert_eq!(message, b"path, out_handle, out_data, and out_length are required");
    }
}
