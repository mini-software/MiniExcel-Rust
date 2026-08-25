use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::streaming::{
    StreamingRows, StreamingStructuredRows, StreamingTableRows, StreamingTableTypedRows,
    StreamingTypedRows,
};
use crate::writer::XlsxWriter;
#[cfg(not(target_arch = "wasm32"))]
use crate::writer::{
    validate_dimensions, validate_insert_sheet_options, validate_schema,
    validate_single_sheet_options,
};
use crate::{
    AnalysisResult, ByteQuerySummary, CsvReadOptions, CsvWriteOptions, DynamicRow, ExcelRange,
    QueryPlan, QuerySummary, RagChunk, RagExport, RagExportOptions, RagManifest, ReadOptions,
    Result, SheetInfo, StructuredRow, TemplateOptions, WriteOptions,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::{ExistingSheetPolicy, InsertOptions, TargetRelationshipPolicy};

/// Convenience entry points for the common path-based MiniExcel workflow.
pub struct MiniExcel;

impl MiniExcel {
    /// Returns worksheet names in workbook order.
    pub fn get_sheet_names(path: impl AsRef<Path>) -> Result<Vec<String>> {
        crate::streaming::sheet_names(path)
    }

    /// Returns worksheet names from an in-memory XLSX workbook.
    pub fn get_sheet_names_from_bytes(bytes: &[u8]) -> Result<Vec<String>> {
        crate::streaming::sheet_names_from_bytes(bytes)
    }

    /// Returns worksheet names from a borrowed seekable XLSX reader.
    pub fn get_sheet_names_from_reader<R>(reader: &mut R) -> Result<Vec<String>>
    where
        R: Read + Seek,
    {
        crate::streaming::sheet_names_from_reader(reader)
    }

    /// Returns worksheet metadata in workbook order.
    pub fn get_sheet_info(path: impl AsRef<Path>) -> Result<Vec<SheetInfo>> {
        crate::streaming::sheet_info(path)
    }

    /// Returns worksheet metadata from an in-memory XLSX workbook.
    pub fn get_sheet_info_from_bytes(bytes: &[u8]) -> Result<Vec<SheetInfo>> {
        crate::streaming::sheet_info_from_bytes(bytes)
    }

    /// Returns worksheet metadata from a borrowed seekable XLSX reader.
    pub fn get_sheet_info_from_reader<R>(reader: &mut R) -> Result<Vec<SheetInfo>>
    where
        R: Read + Seek,
    {
        crate::streaming::sheet_info_from_reader(reader)
    }

    /// Returns the used range of each worksheet in workbook order.
    pub fn get_sheet_dimensions(path: impl AsRef<Path>) -> Result<Vec<ExcelRange>> {
        crate::streaming::sheet_dimensions(path)
    }

    /// Returns worksheet used ranges from an in-memory XLSX workbook.
    pub fn get_sheet_dimensions_from_bytes(bytes: &[u8]) -> Result<Vec<ExcelRange>> {
        crate::streaming::sheet_dimensions_from_bytes(bytes)
    }

    /// Returns worksheet dimensions from a borrowed seekable XLSX reader.
    pub fn get_sheet_dimensions_from_reader<R>(reader: &mut R) -> Result<Vec<ExcelRange>>
    where
        R: Read + Seek,
    {
        crate::streaming::sheet_dimensions_from_reader(reader)
    }

    /// Returns threaded comments and legacy notes for a worksheet.
    ///
    /// When `sheet_name` is `None`, the first worksheet is selected.
    pub fn get_comments(
        path: impl AsRef<Path>,
        sheet_name: Option<&str>,
    ) -> Result<crate::SheetComments> {
        crate::streaming::comments(path, sheet_name)
    }

    /// Returns threaded comments and legacy notes from in-memory XLSX data.
    pub fn get_comments_from_bytes(
        bytes: &[u8],
        sheet_name: Option<&str>,
    ) -> Result<crate::SheetComments> {
        crate::streaming::comments_from_bytes(bytes, sheet_name)
    }

    /// Returns threaded comments and legacy notes from a borrowed XLSX reader.
    ///
    /// The reader remains open and its final position is unspecified.
    pub fn get_comments_from_reader<R>(
        reader: &mut R,
        sheet_name: Option<&str>,
    ) -> Result<crate::SheetComments>
    where
        R: Read + Seek,
    {
        crate::streaming::comments_from_reader(reader, sheet_name)
    }

    /// Streams dynamic rows from the first worksheet without a header row.
    pub fn query(
        path: impl AsRef<Path>,
    ) -> Result<Box<dyn Iterator<Item = Result<DynamicRow>> + Send>> {
        Self::query_with_options(path, &ReadOptions::default())
    }

    /// Streams dynamic rows using explicit read options.
    pub fn query_with_options(
        path: impl AsRef<Path>,
        options: &ReadOptions,
    ) -> Result<Box<dyn Iterator<Item = Result<DynamicRow>> + Send>> {
        Ok(Box::new(StreamingRows::open(path, options)?))
    }

    /// Streams dynamic rows from a named OpenXML table.
    ///
    /// Table names are matched case-insensitively. When `sheet_name` is `None`, only the first
    /// worksheet is searched. Column names and bounds come from table metadata.
    pub fn query_table(
        path: impl AsRef<Path>,
        table_name: &str,
        sheet_name: Option<&str>,
    ) -> Result<Box<dyn Iterator<Item = Result<DynamicRow>> + Send>> {
        Ok(Box::new(StreamingTableRows::open(path, table_name, sheet_name)?))
    }

    /// Streams and deserializes rows from a named OpenXML table through Serde.
    pub fn query_table_as<T>(
        path: impl AsRef<Path>,
        table_name: &str,
        sheet_name: Option<&str>,
    ) -> Result<Box<dyn Iterator<Item = Result<T>> + Send>>
    where
        T: DeserializeOwned + 'static,
    {
        Ok(Box::new(StreamingTableTypedRows::open(path, table_name, sheet_name)?))
    }

    /// Streams dynamic CSV records. Values remain strings and columns use Excel-style letters.
    pub fn query_csv(
        path: impl AsRef<Path>,
    ) -> Result<Box<dyn Iterator<Item = Result<DynamicRow>> + Send>> {
        Self::query_csv_with_options(path, &CsvReadOptions::default())
    }

    /// Streams dynamic CSV records using explicit delimiter, encoding, header, and null options.
    pub fn query_csv_with_options(
        path: impl AsRef<Path>,
        options: &CsvReadOptions,
    ) -> Result<Box<dyn Iterator<Item = Result<DynamicRow>> + Send>> {
        Ok(Box::new(crate::csv_io::query_path(path, options)?))
    }

    /// Streams and deserializes headered CSV records through Serde.
    pub fn query_csv_as<T>(
        path: impl AsRef<Path>,
    ) -> Result<Box<dyn Iterator<Item = Result<T>> + Send>>
    where
        T: DeserializeOwned + 'static,
    {
        Self::query_csv_as_with_options(path, &CsvReadOptions::default())
    }

    /// Streams and deserializes CSV records through Serde using explicit options.
    pub fn query_csv_as_with_options<T>(
        path: impl AsRef<Path>,
        options: &CsvReadOptions,
    ) -> Result<Box<dyn Iterator<Item = Result<T>> + Send>>
    where
        T: DeserializeOwned + 'static,
    {
        Ok(Box::new(crate::csv_io::query_path_as(path, options)?))
    }

    /// Streams sparse rows while preserving cell coordinates, formulas, and number formats.
    ///
    /// Header mode does not consume the first row because structured reads expose source rows
    /// exactly as represented in the worksheet.
    pub fn query_structured(
        path: impl AsRef<Path>,
    ) -> Result<Box<dyn Iterator<Item = Result<StructuredRow>> + Send>> {
        Self::query_structured_with_options(path, &ReadOptions::default())
    }

    /// Streams sparse structure-preserving rows using worksheet and range options.
    pub fn query_structured_with_options(
        path: impl AsRef<Path>,
        options: &ReadOptions,
    ) -> Result<Box<dyn Iterator<Item = Result<StructuredRow>> + Send>> {
        Ok(Box::new(StreamingStructuredRows::open(path, options)?))
    }

    /// Returns the selected dynamic column names, or an empty vector when no data rows exist.
    pub fn get_columns(path: impl AsRef<Path>, options: &ReadOptions) -> Result<Vec<String>> {
        let mut rows = Self::query_with_options(path, options)?;
        Ok(rows.next().transpose()?.map_or_else(Vec::new, |row| row.into_keys().collect()))
    }

    /// Reads dynamic rows from an in-memory XLSX workbook.
    ///
    /// Unlike path queries, this method materializes the selected rows and is intended for
    /// browser uploads and other environments without filesystem access.
    pub fn query_bytes(bytes: &[u8], options: &ReadOptions) -> Result<Vec<DynamicRow>> {
        crate::streaming::query_bytes(bytes, options)
    }

    /// Reads dynamic rows from a named OpenXML table in an in-memory workbook.
    pub fn query_table_bytes(
        bytes: &[u8],
        table_name: &str,
        sheet_name: Option<&str>,
    ) -> Result<Vec<DynamicRow>> {
        crate::streaming::query_table_bytes(bytes, table_name, sheet_name)
    }

    /// Reads dynamic CSV records from bytes.
    pub fn query_csv_bytes(bytes: &[u8], options: &CsvReadOptions) -> Result<Vec<DynamicRow>> {
        crate::csv_io::query_bytes(bytes, options)
    }

    /// Reads typed CSV records from bytes through Serde.
    pub fn query_csv_as_bytes<T>(bytes: &[u8], options: &CsvReadOptions) -> Result<Vec<T>>
    where
        T: DeserializeOwned,
    {
        crate::csv_io::query_bytes_as(bytes, options)
    }

    /// Streams dynamic CSV records from a borrowed reader and leaves it open.
    pub fn query_csv_from_reader<'a, R>(
        reader: &'a mut R,
        options: &CsvReadOptions,
    ) -> Result<Box<dyn Iterator<Item = Result<DynamicRow>> + 'a>>
    where
        R: Read + 'a,
    {
        Ok(Box::new(crate::csv_io::CsvRows::new(reader, options, false)?))
    }

    /// Streams typed CSV records from a borrowed reader and leaves it open.
    pub fn query_csv_as_from_reader<'a, T, R>(
        reader: &'a mut R,
        options: &CsvReadOptions,
    ) -> Result<Box<dyn Iterator<Item = Result<T>> + 'a>>
    where
        T: DeserializeOwned + 'a,
        R: Read + 'a,
    {
        Ok(Box::new(crate::csv_io::CsvTypedRows::new(reader, options)?))
    }

    /// Returns dynamic CSV column names, including for header-only input.
    pub fn get_csv_columns(
        path: impl AsRef<Path>,
        options: &CsvReadOptions,
    ) -> Result<Vec<String>> {
        crate::csv_io::get_columns(BufReader::new(File::open(path)?), options)
    }

    /// Returns dynamic CSV column names from bytes.
    pub fn get_csv_columns_from_bytes(
        bytes: &[u8],
        options: &CsvReadOptions,
    ) -> Result<Vec<String>> {
        crate::csv_io::get_columns(Cursor::new(bytes), options)
    }

    /// Returns dynamic CSV column names from a borrowed reader.
    pub fn get_csv_columns_from_reader<R>(
        reader: &mut R,
        options: &CsvReadOptions,
    ) -> Result<Vec<String>>
    where
        R: Read,
    {
        crate::csv_io::get_columns(reader, options)
    }

    /// Visits in-memory worksheet rows without materializing the complete selection.
    pub fn visit_rows_from_bytes<F>(
        bytes: &[u8],
        options: &ReadOptions,
        mut visitor: F,
    ) -> Result<ByteQuerySummary>
    where
        F: FnMut(usize, &DynamicRow) -> Result<bool>,
    {
        crate::streaming::visit_dynamic_rows(bytes, options, |_, excel_row, row| {
            visitor(excel_row, &row)
        })
    }

    /// Visits dynamic rows from a borrowed seekable XLSX reader without taking ownership.
    pub fn visit_rows_from_reader<R, F>(
        reader: &mut R,
        options: &ReadOptions,
        mut visitor: F,
    ) -> Result<QuerySummary>
    where
        R: Read + Seek,
        F: FnMut(usize, &DynamicRow) -> Result<bool>,
    {
        crate::streaming::visit_dynamic_rows_from_reader(reader, options, |_, excel_row, row| {
            visitor(excel_row, &row)
        })
    }

    /// Visits dynamic rows from a named OpenXML table in a borrowed reader.
    pub fn visit_table_rows_from_reader<R, F>(
        reader: &mut R,
        table_name: &str,
        sheet_name: Option<&str>,
        mut visitor: F,
    ) -> Result<QuerySummary>
    where
        R: Read + Seek,
        F: FnMut(usize, &DynamicRow) -> Result<bool>,
    {
        crate::streaming::visit_table_dynamic_rows_from_reader(
            reader,
            table_name,
            sheet_name,
            |_, excel_row, row| visitor(excel_row, &row),
        )
    }

    /// Visits typed rows from a borrowed seekable XLSX reader without taking ownership.
    pub fn visit_rows_as_from_reader<T, R, F>(
        reader: &mut R,
        options: &ReadOptions,
        mut visitor: F,
    ) -> Result<QuerySummary>
    where
        T: DeserializeOwned,
        R: Read + Seek,
        F: FnMut(usize, &T) -> Result<bool>,
    {
        crate::streaming::visit_typed_rows_from_reader(reader, options, |_, excel_row, row| {
            visitor(excel_row, &row)
        })
    }

    /// Visits Serde-deserialized rows from a named OpenXML table in a borrowed reader.
    pub fn visit_table_rows_as_from_reader<T, R, F>(
        reader: &mut R,
        table_name: &str,
        sheet_name: Option<&str>,
        mut visitor: F,
    ) -> Result<QuerySummary>
    where
        T: DeserializeOwned,
        R: Read + Seek,
        F: FnMut(usize, &T) -> Result<bool>,
    {
        crate::streaming::visit_table_typed_rows_from_reader(
            reader,
            table_name,
            sheet_name,
            |_, excel_row, row| visitor(excel_row, &row),
        )
    }

    /// Visits sparse structure-preserving rows from a borrowed seekable XLSX reader.
    pub fn visit_structured_rows_from_reader<R, F>(
        reader: &mut R,
        options: &ReadOptions,
        mut visitor: F,
    ) -> Result<String>
    where
        R: Read + Seek,
        F: FnMut(&StructuredRow) -> Result<bool>,
    {
        crate::streaming::visit_structured_rows_from_reader(reader, options, |row| visitor(&row))
    }

    /// Returns selected dynamic column names from a borrowed seekable XLSX reader.
    pub fn get_columns_from_reader<R>(reader: &mut R, options: &ReadOptions) -> Result<Vec<String>>
    where
        R: Read + Seek,
    {
        let summary =
            crate::streaming::visit_dynamic_rows_from_reader(reader, options, |_, _, _| Ok(false))?;
        Ok(summary.columns().to_vec())
    }

    /// Streams worksheet rows into a grouped analytical query.
    ///
    /// Source rows are not retained. Memory used by grouping is limited by
    /// [`QueryPlan::with_max_groups`].
    pub fn analyze_with_options(
        path: impl AsRef<Path>,
        options: &ReadOptions,
        plan: &QueryPlan,
    ) -> Result<AnalysisResult> {
        crate::analytics::analyze_path(path, options, plan)
    }

    /// Analyzes an in-memory XLSX workbook without materializing its source rows.
    pub fn analyze_bytes(
        bytes: &[u8],
        options: &ReadOptions,
        plan: &QueryPlan,
    ) -> Result<AnalysisResult> {
        crate::analytics::analyze_bytes(bytes, options, plan)
    }

    /// Streams provenance-preserving JSON-ready chunks from a path-based workbook.
    pub fn export_rag(
        path: impl AsRef<Path>,
        options: &ReadOptions,
        export_options: &RagExportOptions,
    ) -> Result<RagExport> {
        crate::rag::export_path(path, options, export_options)
    }

    /// Visits RAG chunks from in-memory XLSX data without retaining all chunks.
    pub fn visit_rag_chunks_from_bytes<F>(
        bytes: &[u8],
        options: &ReadOptions,
        export_options: &RagExportOptions,
        visitor: F,
    ) -> Result<RagManifest>
    where
        F: FnMut(&RagChunk) -> Result<()>,
    {
        crate::rag::export_bytes(bytes, options, export_options, visitor)
    }

    /// Streams and deserializes rows from the first worksheet through Serde.
    pub fn query_as<T>(path: impl AsRef<Path>) -> Result<Box<dyn Iterator<Item = Result<T>> + Send>>
    where
        T: DeserializeOwned + 'static,
    {
        Self::query_as_with_options(path, &ReadOptions::default())
    }

    /// Streams and deserializes rows through Serde using explicit read options.
    pub fn query_as_with_options<T>(
        path: impl AsRef<Path>,
        options: &ReadOptions,
    ) -> Result<Box<dyn Iterator<Item = Result<T>> + Send>>
    where
        T: DeserializeOwned + 'static,
    {
        Ok(Box::new(StreamingTypedRows::open(path, options)?))
    }

    /// Creates a new XLSX workbook from dynamic rows.
    pub fn save_as(path: impl AsRef<Path>, rows: &[DynamicRow]) -> Result<()> {
        Self::save_as_with_options(path, rows, &WriteOptions::default())
    }

    /// Creates a new XLSX workbook from dynamic rows using explicit options.
    pub fn save_as_with_options(
        path: impl AsRef<Path>,
        rows: &[DynamicRow],
        options: &WriteOptions,
    ) -> Result<()> {
        let mut writer = XlsxWriter::new();
        writer.add_rows(rows, options)?;
        writer.save(path, options.overwrite_file())
    }

    /// Creates a new XLSX workbook containing multiple dynamic worksheets.
    ///
    /// Returns data-row counts in the same order as the supplied worksheets.
    pub fn save_as_sheets<'a, I, N>(
        path: impl AsRef<Path>,
        sheets: I,
        options: &WriteOptions,
    ) -> Result<Vec<usize>>
    where
        I: IntoIterator<Item = (N, &'a [DynamicRow])>,
        N: AsRef<str>,
    {
        let mut writer = XlsxWriter::new();
        let mut row_counts = Vec::new();
        for (sheet_name, rows) in sheets {
            let sheet_options = options.clone().with_sheet_name(sheet_name.as_ref());
            writer.add_rows(rows, &sheet_options)?;
            row_counts.push(rows.len());
        }
        if row_counts.is_empty() {
            return Err(crate::Error::no_worksheets());
        }
        writer.save(path, options.overwrite_file())?;
        Ok(row_counts)
    }

    /// Creates an in-memory XLSX workbook from dynamic rows.
    pub fn save_as_bytes(rows: &[DynamicRow], options: &WriteOptions) -> Result<Vec<u8>> {
        let mut writer = XlsxWriter::new();
        writer.add_rows(rows, options)?;
        writer.save_to_bytes()
    }

    /// Writes a dynamic XLSX workbook to a borrowed writer without closing it.
    pub fn save_as_to_writer<W>(
        writer: &mut W,
        rows: &[DynamicRow],
        options: &WriteOptions,
    ) -> Result<()>
    where
        W: Write + Send,
    {
        let mut xlsx_writer = XlsxWriter::new();
        xlsx_writer.add_rows(rows, options)?;
        xlsx_writer.save_to_writer(writer)
    }

    /// Writes an explicit-schema dynamic XLSX workbook to a borrowed writer.
    pub fn save_as_with_schema_to_writer<W>(
        writer: &mut W,
        schema: &[String],
        rows: &[DynamicRow],
        options: &WriteOptions,
    ) -> Result<()>
    where
        W: Write + Send,
    {
        let mut xlsx_writer = XlsxWriter::new();
        xlsx_writer.add_rows_with_schema(schema, rows, options)?;
        xlsx_writer.save_to_writer(writer)
    }

    /// Writes multiple dynamic worksheets to a borrowed writer.
    pub fn save_as_sheets_to_writer<'a, W, I, N>(
        writer: &mut W,
        sheets: I,
        options: &WriteOptions,
    ) -> Result<Vec<usize>>
    where
        W: Write + Send,
        I: IntoIterator<Item = (N, &'a [DynamicRow])>,
        N: AsRef<str>,
    {
        let mut xlsx_writer = XlsxWriter::new();
        let mut row_counts = Vec::new();
        for (sheet_name, rows) in sheets {
            let sheet_options = options.clone().with_sheet_name(sheet_name.as_ref());
            xlsx_writer.add_rows(rows, &sheet_options)?;
            row_counts.push(rows.len());
        }
        if row_counts.is_empty() {
            return Err(crate::Error::no_worksheets());
        }
        xlsx_writer.save_to_writer(writer)?;
        Ok(row_counts)
    }

    /// Creates a new XLSX workbook using an explicit dynamic schema.
    pub fn save_as_with_schema(
        path: impl AsRef<Path>,
        schema: &[String],
        rows: &[DynamicRow],
        options: &WriteOptions,
    ) -> Result<()> {
        let mut writer = XlsxWriter::new();
        writer.add_rows_with_schema(schema, rows, options)?;
        writer.save(path, options.overwrite_file())
    }

    /// Creates a new XLSX workbook from Serde-serializable rows.
    pub fn save_as_serialized<T>(path: impl AsRef<Path>, rows: &[T]) -> Result<()>
    where
        T: Serialize,
    {
        Self::save_as_serialized_with_options(path, rows, &WriteOptions::default())
    }

    /// Creates a new XLSX workbook from Serde rows using explicit options.
    pub fn save_as_serialized_with_options<T>(
        path: impl AsRef<Path>,
        rows: &[T],
        options: &WriteOptions,
    ) -> Result<()>
    where
        T: Serialize,
    {
        let mut writer = XlsxWriter::new();
        writer.add_serialized(rows, options)?;
        writer.save(path, options.overwrite_file())
    }

    /// Writes Serde rows to a borrowed writer without closing it.
    pub fn save_as_serialized_to_writer<T, W>(
        writer: &mut W,
        rows: &[T],
        options: &WriteOptions,
    ) -> Result<()>
    where
        T: Serialize,
        W: Write + Send,
    {
        let mut xlsx_writer = XlsxWriter::new();
        xlsx_writer.add_serialized(rows, options)?;
        xlsx_writer.save_to_writer(writer)
    }

    /// Writes multiple same-type Serde worksheets to a borrowed writer.
    pub fn save_as_serialized_sheets_to_writer<'a, T, W, I, N>(
        writer: &mut W,
        sheets: I,
        options: &WriteOptions,
    ) -> Result<Vec<usize>>
    where
        T: Serialize + 'a,
        W: Write + Send,
        I: IntoIterator<Item = (N, &'a [T])>,
        N: AsRef<str>,
    {
        let mut xlsx_writer = XlsxWriter::new();
        let mut row_counts = Vec::new();
        for (sheet_name, rows) in sheets {
            let sheet_options = options.clone().with_sheet_name(sheet_name.as_ref());
            xlsx_writer.add_serialized(rows, &sheet_options)?;
            row_counts.push(rows.len());
        }
        if row_counts.is_empty() {
            return Err(crate::Error::no_worksheets());
        }
        xlsx_writer.save_to_writer(writer)?;
        Ok(row_counts)
    }

    /// Creates a new XLSX workbook containing multiple Serde-serializable worksheets.
    ///
    /// All worksheets use the same row type. Returns data-row counts in input order.
    pub fn save_as_serialized_sheets<'a, T, I, N>(
        path: impl AsRef<Path>,
        sheets: I,
        options: &WriteOptions,
    ) -> Result<Vec<usize>>
    where
        T: Serialize + 'a,
        I: IntoIterator<Item = (N, &'a [T])>,
        N: AsRef<str>,
    {
        let mut writer = XlsxWriter::new();
        let mut row_counts = Vec::new();
        for (sheet_name, rows) in sheets {
            let sheet_options = options.clone().with_sheet_name(sheet_name.as_ref());
            writer.add_serialized(rows, &sheet_options)?;
            row_counts.push(rows.len());
        }
        if row_counts.is_empty() {
            return Err(crate::Error::no_worksheets());
        }
        writer.save(path, options.overwrite_file())?;
        Ok(row_counts)
    }

    /// Writes dynamic rows to a CSV path.
    pub fn save_csv(
        path: impl AsRef<Path>,
        rows: &[DynamicRow],
        options: &CsvWriteOptions,
    ) -> Result<usize> {
        crate::csv_io::save_dynamic(path, None, rows, options)
    }

    /// Writes dynamic rows with an explicit CSV schema.
    pub fn save_csv_with_schema(
        path: impl AsRef<Path>,
        schema: &[String],
        rows: &[DynamicRow],
        options: &CsvWriteOptions,
    ) -> Result<usize> {
        crate::csv_io::save_dynamic(path, Some(schema), rows, options)
    }

    /// Writes Serde rows to a CSV path.
    pub fn save_csv_serialized<T>(
        path: impl AsRef<Path>,
        rows: &[T],
        options: &CsvWriteOptions,
    ) -> Result<usize>
    where
        T: Serialize,
    {
        crate::csv_io::save_serialized(path, rows, options)
    }

    /// Writes dynamic CSV rows to a borrowed writer and leaves it open.
    pub fn save_csv_to_writer<W>(
        writer: &mut W,
        rows: &[DynamicRow],
        options: &CsvWriteOptions,
    ) -> Result<usize>
    where
        W: Write,
    {
        crate::csv_io::write_dynamic(writer, None, rows, options, true)
    }

    /// Writes explicit-schema dynamic CSV rows to a borrowed writer.
    pub fn save_csv_with_schema_to_writer<W>(
        writer: &mut W,
        schema: &[String],
        rows: &[DynamicRow],
        options: &CsvWriteOptions,
    ) -> Result<usize>
    where
        W: Write,
    {
        crate::csv_io::write_dynamic(writer, Some(schema), rows, options, true)
    }

    /// Writes Serde CSV rows to a borrowed writer.
    pub fn save_csv_serialized_to_writer<T, W>(
        writer: &mut W,
        rows: &[T],
        options: &CsvWriteOptions,
    ) -> Result<usize>
    where
        T: Serialize,
        W: Write,
    {
        crate::csv_io::write_serialized(writer, rows, options, true)
    }

    /// Creates CSV bytes from dynamic rows.
    pub fn save_csv_bytes(rows: &[DynamicRow], options: &CsvWriteOptions) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        crate::csv_io::write_dynamic(&mut output, None, rows, options, true)?;
        Ok(output)
    }

    /// Creates CSV bytes from explicit-schema dynamic rows.
    pub fn save_csv_with_schema_bytes(
        schema: &[String],
        rows: &[DynamicRow],
        options: &CsvWriteOptions,
    ) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        crate::csv_io::write_dynamic(&mut output, Some(schema), rows, options, true)?;
        Ok(output)
    }

    /// Appends dynamic rows to a CSV path. Existing files receive neither BOM nor header.
    pub fn append_csv(
        path: impl AsRef<Path>,
        rows: &[DynamicRow],
        options: &CsvWriteOptions,
    ) -> Result<usize> {
        crate::csv_io::append_dynamic(path, None, rows, options)
    }

    /// Appends explicit-schema dynamic rows to a CSV path.
    pub fn append_csv_with_schema(
        path: impl AsRef<Path>,
        schema: &[String],
        rows: &[DynamicRow],
        options: &CsvWriteOptions,
    ) -> Result<usize> {
        crate::csv_io::append_dynamic(path, Some(schema), rows, options)
    }

    /// Appends Serde rows to a CSV path.
    pub fn append_csv_serialized<T>(
        path: impl AsRef<Path>,
        rows: &[T],
        options: &CsvWriteOptions,
    ) -> Result<usize>
    where
        T: Serialize,
    {
        crate::csv_io::append_serialized(path, rows, options)
    }

    /// Appends dynamic rows to a borrowed seekable writer.
    pub fn append_csv_to_writer<W>(
        writer: &mut W,
        rows: &[DynamicRow],
        options: &CsvWriteOptions,
    ) -> Result<usize>
    where
        W: Write + Seek,
    {
        let empty = writer.seek(SeekFrom::End(0))? == 0;
        crate::csv_io::write_dynamic(writer, None, rows, options, empty)
    }

    /// Appends explicit-schema dynamic rows to a borrowed seekable writer.
    pub fn append_csv_with_schema_to_writer<W>(
        writer: &mut W,
        schema: &[String],
        rows: &[DynamicRow],
        options: &CsvWriteOptions,
    ) -> Result<usize>
    where
        W: Write + Seek,
    {
        let empty = writer.seek(SeekFrom::End(0))? == 0;
        crate::csv_io::write_dynamic(writer, Some(schema), rows, options, empty)
    }

    /// Appends Serde rows to a borrowed seekable writer.
    pub fn append_csv_serialized_to_writer<T, W>(
        writer: &mut W,
        rows: &[T],
        options: &CsvWriteOptions,
    ) -> Result<usize>
    where
        T: Serialize,
        W: Write + Seek,
    {
        let empty = writer.seek(SeekFrom::End(0))? == 0;
        crate::csv_io::write_serialized(writer, rows, options, empty)
    }

    /// Creates CSV bytes from Serde rows.
    pub fn save_csv_serialized_bytes<T>(rows: &[T], options: &CsvWriteOptions) -> Result<Vec<u8>>
    where
        T: Serialize,
    {
        let mut output = Cursor::new(Vec::new());
        crate::csv_io::write_serialized(&mut output, rows, options, true)?;
        Ok(output.into_inner())
    }

    /// Inserts dynamic rows as a new worksheet, or creates a workbook when the path is missing.
    ///
    /// Existing workbooks are replaced atomically only after the rewritten package validates.
    /// Returns the number of data rows written.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn insert(
        path: impl AsRef<Path>,
        rows: &[DynamicRow],
        options: &InsertOptions,
    ) -> Result<usize> {
        let path = path.as_ref();
        validate_insert_options(path, options)?;
        if !path.exists() {
            let mut writer = XlsxWriter::new();
            writer.add_rows(rows, options.write_options())?;
            writer.save(path, false)?;
            return Ok(rows.len());
        }
        crate::insert::atomic::insert_to_path(
            path,
            options.write_options().sheet_name(),
            options.existing_sheet_policy(),
            options.target_relationship_policy(),
            || crate::insert::donor::DonorBuilder::from_dynamic(rows, options.write_options()),
        )
    }

    /// Inserts a one-pass dynamic row iterator using an explicit schema.
    ///
    /// Iterator items may report producer errors. Source rows are spooled to disk and the writer
    /// retains only the current row while the producer is consumed exactly once. The generated
    /// donor worksheet XML is materialized for style rebasing.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn insert_with_schema<I>(
        path: impl AsRef<Path>,
        schema: &[String],
        rows: I,
        options: &InsertOptions,
    ) -> Result<usize>
    where
        I: IntoIterator<Item = Result<DynamicRow>>,
    {
        let path = path.as_ref();
        validate_insert_options(path, options)?;
        validate_schema(schema)?;
        validate_dimensions(0, schema.len(), options.write_options().print_header())?;
        if !path.exists() {
            return crate::insert::donor::save_dynamic_iter_to_path(
                path,
                schema,
                rows,
                options.write_options(),
            );
        }
        crate::insert::atomic::insert_to_path(
            path,
            options.write_options().sheet_name(),
            options.existing_sheet_policy(),
            options.target_relationship_policy(),
            || {
                crate::insert::donor::DonorBuilder::from_dynamic_iter(
                    schema,
                    rows,
                    options.write_options(),
                )
            },
        )
    }

    /// Inserts Serde-serializable rows as a new worksheet.
    ///
    /// A missing path creates a new workbook. Existing paths are updated atomically. Returns the
    /// number of data rows written.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn insert_serialized<T>(
        path: impl AsRef<Path>,
        rows: &[T],
        options: &InsertOptions,
    ) -> Result<usize>
    where
        T: Serialize,
    {
        let path = path.as_ref();
        validate_insert_options(path, options)?;
        if !path.exists() {
            let mut writer = XlsxWriter::new();
            writer.add_serialized(rows, options.write_options())?;
            writer.save(path, false)?;
            return Ok(rows.len());
        }
        crate::insert::atomic::insert_to_path(
            path,
            options.write_options().sheet_name(),
            options.existing_sheet_policy(),
            options.target_relationship_policy(),
            || crate::insert::donor::DonorBuilder::from_serialized(rows, options.write_options()),
        )
    }

    /// Inserts dynamic rows from a borrowed XLSX reader into a separate borrowed writer.
    ///
    /// The source and destination remain open. The destination must be empty and is not truncated
    /// or rolled back on failure. Source and destination must not alias the same underlying stream.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn insert_from_reader_to_writer<R, W>(
        source: &mut R,
        destination: &mut W,
        rows: &[DynamicRow],
        options: &InsertOptions,
    ) -> Result<usize>
    where
        R: Read + Seek,
        W: Write + Seek,
    {
        validate_existing_insert_options(options)?;
        crate::insert::rewrite::insert_worksheet_from_reader_to_writer(
            source,
            destination,
            options.write_options().sheet_name(),
            options.existing_sheet_policy(),
            options.target_relationship_policy(),
            || crate::insert::donor::DonorBuilder::from_dynamic(rows, options.write_options()),
        )
    }

    /// Inserts a one-pass dynamic row iterator into separate borrowed XLSX streams.
    ///
    /// The source and destination remain open. The destination must be empty and is not truncated
    /// or rolled back on failure. Source rows are consumed once after package preflight succeeds.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn insert_with_schema_from_reader_to_writer<R, W, I>(
        source: &mut R,
        destination: &mut W,
        schema: &[String],
        rows: I,
        options: &InsertOptions,
    ) -> Result<usize>
    where
        R: Read + Seek,
        W: Write + Seek,
        I: IntoIterator<Item = Result<DynamicRow>>,
    {
        validate_existing_insert_options(options)?;
        validate_schema(schema)?;
        validate_dimensions(0, schema.len(), options.write_options().print_header())?;
        crate::insert::rewrite::insert_worksheet_from_reader_to_writer(
            source,
            destination,
            options.write_options().sheet_name(),
            options.existing_sheet_policy(),
            options.target_relationship_policy(),
            || {
                crate::insert::donor::DonorBuilder::from_dynamic_iter(
                    schema,
                    rows,
                    options.write_options(),
                )
            },
        )
    }

    /// Inserts Serde-serializable rows into separate borrowed XLSX streams.
    ///
    /// The source and destination remain open. The destination must be empty and is not truncated
    /// or rolled back on failure. Source and destination must not alias the same underlying stream.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn insert_serialized_from_reader_to_writer<T, R, W>(
        source: &mut R,
        destination: &mut W,
        rows: &[T],
        options: &InsertOptions,
    ) -> Result<usize>
    where
        T: Serialize,
        R: Read + Seek,
        W: Write + Seek,
    {
        validate_existing_insert_options(options)?;
        crate::insert::rewrite::insert_worksheet_from_reader_to_writer(
            source,
            destination,
            options.write_options().sheet_name(),
            options.existing_sheet_policy(),
            options.target_relationship_policy(),
            || crate::insert::donor::DonorBuilder::from_serialized(rows, options.write_options()),
        )
    }

    /// Inserts rows from an async producer while XLSX work runs on a blocking worker thread.
    ///
    /// This API is available with the `async` feature. It does not make ZIP or filesystem I/O
    /// asynchronous. The path must reference an existing XLSX workbook.
    #[cfg(all(feature = "async", not(target_arch = "wasm32")))]
    pub async fn insert_with_schema_async<S>(
        path: impl AsRef<Path>,
        schema: &[String],
        rows: S,
        options: &InsertOptions,
    ) -> Result<usize>
    where
        S: futures_core::Stream<Item = Result<DynamicRow>>,
    {
        Self::insert_with_schema_async_with_cancellation(
            path,
            schema,
            rows,
            options,
            crate::CancellationToken::new(),
        )
        .await
    }

    /// Inserts rows from an async producer with cooperative cancellation.
    ///
    /// Cancellation before the commit boundary preserves the original workbook. Dropping the
    /// returned future requests cancellation; worker cleanup then completes in the background.
    #[cfg(all(feature = "async", not(target_arch = "wasm32")))]
    pub async fn insert_with_schema_async_with_cancellation<S>(
        path: impl AsRef<Path>,
        schema: &[String],
        rows: S,
        options: &InsertOptions,
        cancellation: crate::CancellationToken,
    ) -> Result<usize>
    where
        S: futures_core::Stream<Item = Result<DynamicRow>>,
    {
        if cancellation.is_cancelled() {
            return Err(crate::Error::cancelled());
        }
        let path = path.as_ref();
        validate_insert_options(path, options)?;
        std::fs::metadata(path)?;
        validate_schema(schema)?;
        validate_dimensions(0, schema.len(), options.write_options().print_header())?;
        crate::insert::async_insert::insert_with_schema_async(
            path.to_owned(),
            schema.to_vec(),
            rows,
            options.clone(),
            cancellation,
        )
        .await
    }

    /// Fills an existing XLSX template and writes a new workbook.
    ///
    /// Supports `{{name}}` scalar placeholders and single-row expansion for array paths such as
    /// `{{items.name}}`. Existing workbook styles and unrelated package parts are preserved.
    pub fn save_as_template<T>(
        path: impl AsRef<Path>,
        template_path: impl AsRef<Path>,
        value: &T,
        options: &TemplateOptions,
    ) -> Result<()>
    where
        T: Serialize,
    {
        crate::template::fill_path(path, template_path, value, options)
    }

    /// Fills an in-memory XLSX template and returns the generated workbook bytes.
    pub fn save_as_template_bytes<T>(
        template_bytes: &[u8],
        value: &T,
        options: &TemplateOptions,
    ) -> Result<Vec<u8>>
    where
        T: Serialize,
    {
        crate::template::fill_bytes(template_bytes, value, options)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn validate_insert_options(path: &Path, options: &InsertOptions) -> Result<()> {
    if path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("xlsm"))
    {
        return Err(crate::Error::unsupported_package_feature(
            "Insert does not support macro-enabled .xlsm paths",
        ));
    }
    validate_insert_policies(options)?;
    if path.exists() {
        validate_insert_sheet_options(options.write_options())
    } else {
        validate_single_sheet_options(options.write_options())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn validate_existing_insert_options(options: &InsertOptions) -> Result<()> {
    validate_insert_policies(options)?;
    validate_insert_sheet_options(options.write_options())
}

#[cfg(not(target_arch = "wasm32"))]
fn validate_insert_policies(options: &InsertOptions) -> Result<()> {
    if options.write_options().overwrite_file() {
        return Err(crate::Error::invalid_write_options(
            "overwrite_file does not apply to Insert; use ExistingSheetPolicy",
        ));
    }
    if options.existing_sheet_policy() == ExistingSheetPolicy::Reject
        && options.target_relationship_policy() != TargetRelationshipPolicy::Reject
    {
        return Err(crate::Error::invalid_write_options(
            "target relationship removal requires ExistingSheetPolicy::Replace",
        ));
    }
    Ok(())
}
