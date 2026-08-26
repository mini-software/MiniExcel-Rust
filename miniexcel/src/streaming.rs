#[cfg(all(feature = "async", not(target_arch = "wasm32")))]
mod async_query;
mod comments;
mod ooxml;
mod shared_strings;

use std::collections::HashMap;
use std::io::{Read, Seek};
use std::iter::FusedIterator;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::Arc;

use calamine::{Data, RangeDeserializerBuilder};
use serde::de::DeserializeOwned;

use crate::reader::{
    column_names, header_names, into_cell_value, row_to_range, to_cell_value, trim_header_row,
};
use crate::{DynamicRow, Error, ReadOptions, Result, StructuredCell, StructuredRow};

use self::ooxml::{StreamingRawRows, StreamingTableRawRows};

#[cfg(all(feature = "async", not(target_arch = "wasm32")))]
pub(crate) use async_query::spawn as spawn_async_query;

pub(crate) fn comments(
    path: impl AsRef<Path>,
    sheet_name: Option<&str>,
) -> Result<crate::SheetComments> {
    comments::get_comments(path, sheet_name)
}

pub(crate) fn comments_from_bytes(
    bytes: &[u8],
    sheet_name: Option<&str>,
) -> Result<crate::SheetComments> {
    comments::get_comments_from_bytes(bytes, sheet_name)
}

pub(crate) fn comments_from_reader<R>(
    reader: &mut R,
    sheet_name: Option<&str>,
) -> Result<crate::SheetComments>
where
    R: Read + Seek,
{
    comments::get_comments_from_reader(reader, sheet_name)
}

enum Headers {
    FirstRow(Vec<Option<String>>),
    ColumnLetters { start_column: usize, headers: Option<Vec<Option<String>>> },
}

impl Headers {
    fn for_width(&mut self, width: usize) -> &[Option<String>] {
        match self {
            Self::FirstRow(headers) => headers,
            Self::ColumnLetters { start_column, headers } => {
                headers.get_or_insert_with(|| column_names(*start_column, width))
            }
        }
    }

    fn columns(&self) -> Vec<String> {
        match self {
            Self::FirstRow(headers) => headers.iter().flatten().cloned().collect(),
            Self::ColumnLetters { headers, .. } => {
                headers.as_deref().unwrap_or_default().iter().flatten().cloned().collect()
            }
        }
    }
}

/// A bounded-memory iterator over dynamic XLSX rows.
pub(crate) struct StreamingRows {
    rows: StreamingRawRows,
    headers: Headers,
}

impl StreamingRows {
    pub(crate) fn open(path: impl AsRef<Path>, options: &ReadOptions) -> Result<Self> {
        let mut rows = StreamingRawRows::open(path, options, false)?;
        let headers = if options.uses_headers(false) {
            let headers =
                rows.next().transpose()?.map_or_else(Vec::new, |row| header_names(&row.values));
            Headers::FirstRow(headers)
        } else {
            Headers::ColumnLetters { start_column: options.start_cell().column(), headers: None }
        };
        Ok(Self { rows, headers })
    }

    pub(crate) fn next_with_excel_row(&mut self) -> Option<Result<(usize, DynamicRow)>> {
        let selected_row = match self.rows.next()? {
            Ok(row) => row,
            Err(error) => return Some(Err(error)),
        };
        let excel_row = selected_row.excel_row + 1;
        Some(Ok((excel_row, to_dynamic_row(&mut self.headers, selected_row))))
    }

    pub(crate) fn sheet_name(&self) -> &str {
        self.rows.sheet_name()
    }

    pub(crate) fn columns(&self) -> Vec<String> {
        self.headers.columns()
    }
}

impl Iterator for StreamingRows {
    type Item = Result<DynamicRow>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_with_excel_row().map(|result| result.map(|(_, row)| row))
    }
}

impl FusedIterator for StreamingRows {}

/// A bounded-memory iterator over dynamic rows in a named OpenXML table.
pub(crate) struct StreamingTableRows {
    rows: StreamingTableRawRows,
    headers: Headers,
}

impl StreamingTableRows {
    pub(crate) fn open(
        path: impl AsRef<Path>,
        table_name: &str,
        sheet_name: Option<&str>,
    ) -> Result<Self> {
        let rows = StreamingTableRawRows::open(path, table_name, sheet_name)?;
        let headers = Headers::FirstRow(rows.headers().to_vec());
        Ok(Self { rows, headers })
    }
}

impl Iterator for StreamingTableRows {
    type Item = Result<DynamicRow>;

    fn next(&mut self) -> Option<Self::Item> {
        let selected_row = match self.rows.next()? {
            Ok(row) => row,
            Err(error) => return Some(Err(error)),
        };
        Some(Ok(to_dynamic_row(&mut self.headers, selected_row)))
    }
}

impl FusedIterator for StreamingTableRows {}

/// A bounded-memory iterator over sparse structure-preserving XLSX rows.
pub(crate) struct StreamingStructuredRows {
    rows: StreamingRawRows,
    sheet_name: Arc<str>,
}

impl StreamingStructuredRows {
    pub(crate) fn open(path: impl AsRef<Path>, options: &ReadOptions) -> Result<Self> {
        let rows = StreamingRawRows::open(path, options, true)?;
        let sheet_name = Arc::from(rows.sheet_name());
        Ok(Self { rows, sheet_name })
    }
}

impl Iterator for StreamingStructuredRows {
    type Item = Result<StructuredRow>;

    fn next(&mut self) -> Option<Self::Item> {
        let selected_row = match self.rows.next()? {
            Ok(row) => row,
            Err(error) => return Some(Err(error)),
        };
        Some(Ok(to_structured_row(Arc::clone(&self.sheet_name), selected_row)))
    }
}

impl FusedIterator for StreamingStructuredRows {}

pub(crate) fn query_bytes(bytes: &[u8], options: &ReadOptions) -> Result<Vec<DynamicRow>> {
    let mut rows = Vec::new();
    visit_dynamic_rows(bytes, options, |_, _, row| {
        rows.push(row);
        Ok(true)
    })?;
    Ok(rows)
}

pub(crate) fn query_table_bytes(
    bytes: &[u8],
    table_name: &str,
    sheet_name: Option<&str>,
) -> Result<Vec<DynamicRow>> {
    let mut rows = Vec::new();
    visit_table_dynamic_rows(bytes, table_name, sheet_name, |_, _, row| {
        rows.push(row);
        Ok(true)
    })?;
    Ok(rows)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuerySummary {
    sheet_name: String,
    columns: Vec<String>,
    visited_rows: usize,
}

impl QuerySummary {
    #[must_use]
    pub fn sheet_name(&self) -> &str {
        &self.sheet_name
    }

    #[must_use]
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    #[must_use]
    pub const fn visited_rows(&self) -> usize {
        self.visited_rows
    }
}

pub type ByteQuerySummary = QuerySummary;

pub(crate) fn visit_structured_rows<F>(
    bytes: &[u8],
    options: &ReadOptions,
    mut visitor: F,
) -> Result<String>
where
    F: FnMut(StructuredRow) -> Result<bool>,
{
    let mut shared_sheet_name = None;
    ooxml::visit_raw_rows(bytes, options, true, |sheet_name, selected_row| {
        let sheet_name =
            Arc::clone(shared_sheet_name.get_or_insert_with(|| Arc::<str>::from(sheet_name)));
        visitor(to_structured_row(sheet_name, selected_row))
    })
}

pub(crate) fn visit_structured_rows_from_reader<R, F>(
    reader: &mut R,
    options: &ReadOptions,
    mut visitor: F,
) -> Result<String>
where
    R: Read + Seek,
    F: FnMut(StructuredRow) -> Result<bool>,
{
    let mut shared_sheet_name = None;
    ooxml::visit_raw_rows_from_reader(
        reader,
        options,
        true,
        !cfg!(target_arch = "wasm32"),
        |sheet_name, selected_row| {
            let sheet_name =
                Arc::clone(shared_sheet_name.get_or_insert_with(|| Arc::<str>::from(sheet_name)));
            visitor(to_structured_row(sheet_name, selected_row))
        },
    )
}

pub(crate) fn read_mapped_values_from_reader<R>(
    reader: &mut R,
    options: &ReadOptions,
    positions: &[(usize, usize)],
) -> Result<(String, Vec<Data>)>
where
    R: Read + Seek,
{
    let mut by_row = HashMap::<usize, Vec<(usize, usize)>>::new();
    for (index, (row, column)) in positions.iter().copied().enumerate() {
        by_row.entry(row).or_default().push((column, index));
    }
    let mut values = vec![Data::Empty; positions.len()];
    let sheet_name = ooxml::visit_raw_rows_from_reader(
        reader,
        options,
        false,
        !cfg!(target_arch = "wasm32"),
        |_, selected_row| {
            if let Some(columns) = by_row.get(&selected_row.excel_row) {
                for (column, index) in columns {
                    if let Some(offset) = column.checked_sub(selected_row.start_column) {
                        if let Some(value) = selected_row.values.get(offset) {
                            values[*index] = value.clone();
                        }
                    }
                }
            }
            Ok(true)
        },
    )?;
    Ok((sheet_name, values))
}

pub(crate) fn visit_dynamic_rows<F>(
    bytes: &[u8],
    options: &ReadOptions,
    mut visitor: F,
) -> Result<QuerySummary>
where
    F: FnMut(&str, usize, DynamicRow) -> Result<bool>,
{
    let mut headers = (!options.uses_headers(false)).then(|| Headers::ColumnLetters {
        start_column: options.start_cell().column(),
        headers: None,
    });
    let mut visited_rows = 0;
    let sheet_name = ooxml::visit_raw_rows(bytes, options, false, |sheet_name, selected_row| {
        if headers.is_none() {
            headers = Some(Headers::FirstRow(header_names(&selected_row.values)));
            return Ok(true);
        }
        let excel_row = selected_row.excel_row + 1;
        let row = to_dynamic_row(headers.as_mut().expect("headers initialized"), selected_row);
        visited_rows += 1;
        visitor(sheet_name, excel_row, row)
    })?;
    let columns = headers.map_or_else(Vec::new, |headers| headers.columns());
    Ok(QuerySummary { sheet_name, columns, visited_rows })
}

pub(crate) fn visit_dynamic_rows_from_reader<R, F>(
    reader: &mut R,
    options: &ReadOptions,
    mut visitor: F,
) -> Result<QuerySummary>
where
    R: Read + Seek,
    F: FnMut(&str, usize, DynamicRow) -> Result<bool>,
{
    let mut headers = (!options.uses_headers(false)).then(|| Headers::ColumnLetters {
        start_column: options.start_cell().column(),
        headers: None,
    });
    let mut visited_rows = 0;
    let sheet_name = ooxml::visit_raw_rows_from_reader(
        reader,
        options,
        false,
        !cfg!(target_arch = "wasm32"),
        |sheet_name, selected_row| {
            if headers.is_none() {
                headers = Some(Headers::FirstRow(header_names(&selected_row.values)));
                return Ok(true);
            }
            let excel_row = selected_row.excel_row + 1;
            let row = to_dynamic_row(headers.as_mut().expect("headers initialized"), selected_row);
            visited_rows += 1;
            visitor(sheet_name, excel_row, row)
        },
    )?;
    let columns = headers.map_or_else(Vec::new, |headers| headers.columns());
    Ok(QuerySummary { sheet_name, columns, visited_rows })
}

pub(crate) fn visit_table_dynamic_rows<F>(
    bytes: &[u8],
    table_name: &str,
    sheet_name: Option<&str>,
    mut visitor: F,
) -> Result<QuerySummary>
where
    F: FnMut(&str, usize, DynamicRow) -> Result<bool>,
{
    let mut headers = None::<Headers>;
    let mut visited_rows = 0;
    let ready = ooxml::visit_table_raw_rows(
        bytes,
        table_name,
        sheet_name,
        |resolved_sheet, resolved_headers, selected_row| {
            if headers.is_none() {
                headers = Some(Headers::FirstRow(resolved_headers.to_vec()));
            }
            let excel_row = selected_row.excel_row + 1;
            let row =
                to_dynamic_row(headers.as_mut().expect("table headers initialized"), selected_row);
            visited_rows += 1;
            visitor(resolved_sheet, excel_row, row)
        },
    )?;
    let columns = ready.headers.iter().flatten().cloned().collect();
    Ok(QuerySummary { sheet_name: ready.sheet_name, columns, visited_rows })
}

pub(crate) fn visit_table_dynamic_rows_from_reader<R, F>(
    reader: &mut R,
    table_name: &str,
    sheet_name: Option<&str>,
    mut visitor: F,
) -> Result<QuerySummary>
where
    R: Read + Seek,
    F: FnMut(&str, usize, DynamicRow) -> Result<bool>,
{
    let mut visited_rows = 0;
    let mut table_headers = None::<Headers>;
    let ready = ooxml::visit_table_raw_rows_from_reader(
        reader,
        table_name,
        sheet_name,
        !cfg!(target_arch = "wasm32"),
        |resolved_sheet, resolved_headers, selected_row| {
            if table_headers.is_none() {
                table_headers = Some(Headers::FirstRow(resolved_headers.to_vec()));
            }
            let excel_row = selected_row.excel_row + 1;
            let row = to_dynamic_row(
                table_headers.as_mut().expect("table headers initialized"),
                selected_row,
            );
            visited_rows += 1;
            visitor(resolved_sheet, excel_row, row)
        },
    )?;
    let columns = ready.headers.iter().flatten().cloned().collect();
    Ok(QuerySummary { sheet_name: ready.sheet_name, columns, visited_rows })
}

pub(crate) fn visit_typed_rows_from_reader<R, T, F>(
    reader: &mut R,
    options: &ReadOptions,
    mut visitor: F,
) -> Result<QuerySummary>
where
    R: Read + Seek,
    T: DeserializeOwned,
    F: FnMut(&str, usize, T) -> Result<bool>,
{
    let uses_headers = options.uses_headers(true);
    let mut headers = None::<Vec<Data>>;
    let mut columns = None::<Headers>;
    let mut visited_rows = 0;
    let sheet_name = ooxml::visit_raw_rows_from_reader(
        reader,
        options,
        false,
        !cfg!(target_arch = "wasm32"),
        |sheet_name, mut selected_row| {
            if uses_headers && headers.is_none() {
                if options.trim_headers() {
                    trim_header_row(&mut selected_row.values);
                }
                columns = Some(Headers::FirstRow(header_names(&selected_row.values)));
                headers = Some(selected_row.values);
                return Ok(true);
            }
            if columns.is_none() {
                columns = Some(Headers::ColumnLetters {
                    start_column: selected_row.start_column,
                    headers: None,
                });
            }
            columns.as_mut().expect("columns initialized").for_width(selected_row.values.len());
            let excel_row = selected_row.excel_row + 1;
            let value =
                deserialize_selected_row::<T>(sheet_name, headers.as_deref(), selected_row)?;
            visited_rows += 1;
            visitor(sheet_name, excel_row, value)
        },
    )?;
    let columns = columns.map_or_else(Vec::new, |headers| headers.columns());
    Ok(QuerySummary { sheet_name, columns, visited_rows })
}

pub(crate) fn visit_table_typed_rows_from_reader<R, T, F>(
    reader: &mut R,
    table_name: &str,
    sheet_name: Option<&str>,
    mut visitor: F,
) -> Result<QuerySummary>
where
    R: Read + Seek,
    T: DeserializeOwned,
    F: FnMut(&str, usize, T) -> Result<bool>,
{
    let mut visited_rows = 0;
    let mut headers = None::<Vec<Data>>;
    let ready = ooxml::visit_table_raw_rows_from_reader(
        reader,
        table_name,
        sheet_name,
        !cfg!(target_arch = "wasm32"),
        |resolved_sheet, resolved_headers, selected_row| {
            let excel_row = selected_row.excel_row + 1;
            if headers.is_none() {
                let mut resolved = resolved_headers
                    .iter()
                    .map(|header| Data::String(header.clone().unwrap_or_default()))
                    .collect::<Vec<_>>();
                trim_header_row(&mut resolved);
                headers = Some(resolved);
            }
            let value =
                deserialize_selected_row::<T>(resolved_sheet, headers.as_deref(), selected_row)?;
            visited_rows += 1;
            visitor(resolved_sheet, excel_row, value)
        },
    )?;
    let columns = ready.headers.iter().flatten().cloned().collect();
    Ok(QuerySummary { sheet_name: ready.sheet_name, columns, visited_rows })
}

pub(crate) fn sheet_names_from_reader<R>(reader: &mut R) -> Result<Vec<String>>
where
    R: Read + Seek,
{
    ooxml::sheet_names_from_reader(reader)
}

pub(crate) fn sheet_info_from_reader<R>(reader: &mut R) -> Result<Vec<crate::SheetInfo>>
where
    R: Read + Seek,
{
    ooxml::sheet_info_from_reader(reader)
}

pub(crate) fn sheet_dimensions_from_reader<R>(reader: &mut R) -> Result<Vec<crate::ExcelRange>>
where
    R: Read + Seek,
{
    ooxml::sheet_dimensions_from_reader(reader)
}

pub(crate) fn sheet_names_from_bytes(bytes: &[u8]) -> Result<Vec<String>> {
    ooxml::sheet_names_from_bytes(bytes)
}

pub(crate) fn sheet_names(path: impl AsRef<Path>) -> Result<Vec<String>> {
    ooxml::sheet_names(path)
}

pub(crate) fn sheet_info(path: impl AsRef<Path>) -> Result<Vec<crate::SheetInfo>> {
    ooxml::sheet_info(path)
}

pub(crate) fn sheet_info_from_bytes(bytes: &[u8]) -> Result<Vec<crate::SheetInfo>> {
    ooxml::sheet_info_from_bytes(bytes)
}

pub(crate) fn sheet_dimensions(path: impl AsRef<Path>) -> Result<Vec<crate::ExcelRange>> {
    ooxml::sheet_dimensions(path)
}

pub(crate) fn sheet_dimensions_from_bytes(bytes: &[u8]) -> Result<Vec<crate::ExcelRange>> {
    ooxml::sheet_dimensions_from_bytes(bytes)
}

fn to_dynamic_row(headers: &mut Headers, selected_row: crate::reader::SelectedRow) -> DynamicRow {
    let headers = headers.for_width(selected_row.values.len());
    let mut row = DynamicRow::with_capacity(headers.len());
    let mut values = selected_row.values.into_iter();
    for header in headers {
        let value = values.next();
        let Some(header) = header else {
            continue;
        };
        let value = value.map_or(crate::CellValue::Empty, into_cell_value);
        row.insert(header.clone(), value);
    }
    row
}

fn to_structured_row(
    sheet_name: Arc<str>,
    selected_row: crate::reader::SelectedRow,
) -> StructuredRow {
    let cells = selected_row
        .cells
        .into_iter()
        .map(|metadata| {
            let offset = metadata
                .excel_column
                .checked_sub(selected_row.start_column)
                .expect("selected cell is within the selected range");
            let value =
                selected_row.values.get(offset).expect("selected cell has an aligned value");
            StructuredCell::new(
                selected_row.excel_row,
                metadata.excel_column,
                to_cell_value(value),
                metadata.formula,
                metadata.style_id,
                metadata.number_format,
            )
        })
        .collect();
    StructuredRow::new(sheet_name, selected_row.excel_row, cells)
}

/// A bounded-memory iterator that deserializes XLSX rows through Serde.
pub(crate) struct StreamingTypedRows<T> {
    rows: StreamingRawRows,
    headers: Option<Vec<Data>>,
    sheet_name: String,
    marker: PhantomData<fn() -> T>,
}

pub(crate) struct StreamingTableTypedRows<T> {
    rows: StreamingTableRawRows,
    headers: Vec<Data>,
    sheet_name: String,
    marker: PhantomData<fn() -> T>,
}

impl<T> StreamingTableTypedRows<T>
where
    T: DeserializeOwned,
{
    pub(crate) fn open(
        path: impl AsRef<Path>,
        table_name: &str,
        sheet_name: Option<&str>,
    ) -> Result<Self> {
        let rows = StreamingTableRawRows::open(path, table_name, sheet_name)?;
        let sheet_name = rows.sheet_name().to_owned();
        let mut headers = rows
            .headers()
            .iter()
            .map(|header| Data::String(header.clone().unwrap_or_default()))
            .collect::<Vec<_>>();
        trim_header_row(&mut headers);
        Ok(Self { rows, headers, sheet_name, marker: PhantomData })
    }
}

impl<T> Iterator for StreamingTableTypedRows<T>
where
    T: DeserializeOwned,
{
    type Item = Result<T>;

    fn next(&mut self) -> Option<Self::Item> {
        let row = match self.rows.next()? {
            Ok(row) => row,
            Err(error) => return Some(Err(error)),
        };
        Some(deserialize_selected_row(&self.sheet_name, Some(&self.headers), row))
    }
}

impl<T> FusedIterator for StreamingTableTypedRows<T> where T: DeserializeOwned {}

impl<T> StreamingTypedRows<T>
where
    T: DeserializeOwned,
{
    pub(crate) fn open(path: impl AsRef<Path>, options: &ReadOptions) -> Result<Self> {
        let mut rows = StreamingRawRows::open(path, options, false)?;
        let sheet_name = rows.sheet_name().to_owned();
        let headers = if options.uses_headers(true) {
            rows.next().transpose()?.map(|mut row| {
                if options.trim_headers() {
                    trim_header_row(&mut row.values);
                }
                row.values
            })
        } else {
            None
        };
        Ok(Self { rows, headers, sheet_name, marker: PhantomData })
    }
}

impl<T> Iterator for StreamingTypedRows<T>
where
    T: DeserializeOwned,
{
    type Item = Result<T>;

    fn next(&mut self) -> Option<Self::Item> {
        let row = match self.rows.next()? {
            Ok(row) => row,
            Err(error) => return Some(Err(error)),
        };
        Some(deserialize_selected_row(&self.sheet_name, self.headers.as_deref(), row))
    }
}

impl<T> FusedIterator for StreamingTypedRows<T> where T: DeserializeOwned {}

fn deserialize_selected_row<T>(
    sheet_name: &str,
    headers: Option<&[Data]>,
    row: crate::reader::SelectedRow,
) -> Result<T>
where
    T: DeserializeOwned,
{
    let excel_row = row.excel_row + 1;
    let range = row_to_range(headers, &row.values);
    let mut builder = RangeDeserializerBuilder::new();
    builder.has_headers(headers.is_some());
    builder
        .from_range::<Data, T>(&range)
        .and_then(|mut iterator| {
            iterator.next().unwrap_or_else(|| {
                Err(calamine::DeError::Custom(
                    "the selected Excel row did not produce a value".to_owned(),
                ))
            })
        })
        .map_err(|source| Error::deserialize(sheet_name, excel_row, source))
}
