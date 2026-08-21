use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::streaming::{StreamingRows, StreamingStructuredRows, StreamingTypedRows};
use crate::writer::XlsxWriter;
use crate::{
    AnalysisResult, ByteQuerySummary, DynamicRow, ExcelRange, QueryPlan, RagChunk, RagExport,
    RagExportOptions, RagManifest, ReadOptions, Result, SheetInfo, StructuredRow, WriteOptions,
};

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

    /// Returns worksheet metadata in workbook order.
    pub fn get_sheet_info(path: impl AsRef<Path>) -> Result<Vec<SheetInfo>> {
        crate::streaming::sheet_info(path)
    }

    /// Returns worksheet metadata from an in-memory XLSX workbook.
    pub fn get_sheet_info_from_bytes(bytes: &[u8]) -> Result<Vec<SheetInfo>> {
        crate::streaming::sheet_info_from_bytes(bytes)
    }

    /// Returns the used range of each worksheet in workbook order.
    pub fn get_sheet_dimensions(path: impl AsRef<Path>) -> Result<Vec<ExcelRange>> {
        crate::streaming::sheet_dimensions(path)
    }

    /// Returns worksheet used ranges from an in-memory XLSX workbook.
    pub fn get_sheet_dimensions_from_bytes(bytes: &[u8]) -> Result<Vec<ExcelRange>> {
        crate::streaming::sheet_dimensions_from_bytes(bytes)
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
}
