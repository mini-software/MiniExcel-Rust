mod ooxml;

use std::iter::FusedIterator;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::Arc;

use calamine::{Data, RangeDeserializerBuilder};
use serde::de::DeserializeOwned;

use crate::reader::{column_names, header_names, row_to_range, to_cell_value, trim_header_row};
use crate::{DynamicRow, Error, ReadOptions, Result, StructuredCell, StructuredRow};

use self::ooxml::StreamingRawRows;

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ByteQuerySummary {
    sheet_name: String,
    columns: Vec<String>,
    visited_rows: usize,
}

impl ByteQuerySummary {
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

pub(crate) fn visit_dynamic_rows<F>(
    bytes: &[u8],
    options: &ReadOptions,
    mut visitor: F,
) -> Result<ByteQuerySummary>
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
    Ok(ByteQuerySummary { sheet_name, columns, visited_rows })
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
    for (column, header) in headers.iter().enumerate() {
        let Some(header) = header else {
            continue;
        };
        let value = selected_row.values.get(column).map_or(crate::CellValue::Empty, to_cell_value);
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
        let range = row_to_range(self.headers.as_deref(), &row.values);
        let mut builder = RangeDeserializerBuilder::new();
        builder.has_headers(self.headers.is_some());
        let result = builder
            .from_range::<Data, T>(&range)
            .and_then(|mut iterator| {
                iterator.next().unwrap_or_else(|| {
                    Err(calamine::DeError::Custom(
                        "the selected Excel row did not produce a value".to_owned(),
                    ))
                })
            })
            .map_err(|source| Error::deserialize(&self.sheet_name, row.excel_row + 1, source));
        Some(result)
    }
}

impl<T> FusedIterator for StreamingTypedRows<T> where T: DeserializeOwned {}
