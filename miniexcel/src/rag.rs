use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::streaming::{StreamingStructuredRows, visit_structured_rows};
use crate::{
    CellReference, CellValue, Error, ReadOptions, Result, SheetInfo, SheetVisibility,
    StructuredCell, StructuredRow,
};

const DEFAULT_CHUNK_ROWS: usize = 25;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RagExportOptions {
    chunk_rows: usize,
    max_rows: Option<usize>,
    allow_hidden_sheets: bool,
    source_name: Option<String>,
}

impl RagExportOptions {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub const fn with_chunk_rows(mut self, chunk_rows: usize) -> Self {
        self.chunk_rows = chunk_rows;
        self
    }

    #[must_use]
    pub const fn with_max_rows(mut self, max_rows: usize) -> Self {
        self.max_rows = Some(max_rows);
        self
    }

    #[must_use]
    pub const fn with_allow_hidden_sheets(mut self, allow: bool) -> Self {
        self.allow_hidden_sheets = allow;
        self
    }

    #[must_use]
    pub fn with_source_name(mut self, name: impl Into<String>) -> Self {
        self.source_name = Some(name.into());
        self
    }

    #[must_use]
    pub const fn chunk_rows(&self) -> usize {
        self.chunk_rows
    }
}

impl Default for RagExportOptions {
    fn default() -> Self {
        Self {
            chunk_rows: DEFAULT_CHUNK_ROWS,
            max_rows: None,
            allow_hidden_sheets: false,
            source_name: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum RagValue {
    Empty,
    Bool(bool),
    Int(i64),
    Float(String),
    String(String),
    Date(String),
    Time(String),
    DateTime(String),
    DurationMilliseconds(i64),
    Error(String),
}

impl From<&CellValue> for RagValue {
    fn from(value: &CellValue) -> Self {
        match value {
            CellValue::Empty => Self::Empty,
            CellValue::Bool(value) => Self::Bool(*value),
            CellValue::Int(value) => Self::Int(*value),
            CellValue::Float(value) => Self::Float(value.to_string()),
            CellValue::String(value) => Self::String(value.clone()),
            CellValue::Date(value) => Self::Date(value.format("%Y-%m-%d").to_string()),
            CellValue::Time(value) => Self::Time(value.format("%H:%M:%S%.f").to_string()),
            CellValue::DateTime(value) => {
                Self::DateTime(value.format("%Y-%m-%dT%H:%M:%S%.f").to_string())
            }
            CellValue::Duration(value) => Self::DurationMilliseconds(value.num_milliseconds()),
            CellValue::Error(value) => Self::Error(value.clone()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FormulaCalculationStatus {
    NotApplicable,
    CachedOnly,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RagCell {
    row: u32,
    column: u32,
    address: String,
    value: RagValue,
    formula: Option<String>,
    calculation_status: FormulaCalculationStatus,
    style_id: u32,
    number_format: Option<String>,
}

impl RagCell {
    #[must_use]
    pub fn address(&self) -> &str {
        &self.address
    }

    #[must_use]
    pub fn value(&self) -> &RagValue {
        &self.value
    }

    #[must_use]
    pub fn formula(&self) -> Option<&str> {
        self.formula.as_deref()
    }
}

impl From<&StructuredCell> for RagCell {
    fn from(cell: &StructuredCell) -> Self {
        let formula = cell.formula().map(str::to_owned);
        Self {
            row: cell.row_index(),
            column: cell.column_index(),
            address: cell.address(),
            value: RagValue::from(cell.value()),
            calculation_status: if formula.is_some() {
                FormulaCalculationStatus::CachedOnly
            } else {
                FormulaCalculationStatus::NotApplicable
            },
            formula,
            style_id: cell.style_id(),
            number_format: cell.number_format().map(str::to_owned),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RagRow {
    row: u32,
    cells: Vec<RagCell>,
}

impl RagRow {
    #[must_use]
    pub const fn row_index(&self) -> u32 {
        self.row
    }

    #[must_use]
    pub fn cells(&self) -> &[RagCell] {
        &self.cells
    }
}

impl From<StructuredRow> for RagRow {
    fn from(row: StructuredRow) -> Self {
        Self { row: row.row_index(), cells: row.cells().iter().map(RagCell::from).collect() }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RagChunk {
    version: String,
    chunk_id: String,
    sheet_name: String,
    sheet_index: usize,
    data_range: String,
    header: Option<RagRow>,
    rows: Vec<RagRow>,
}

impl RagChunk {
    #[must_use]
    pub fn chunk_id(&self) -> &str {
        &self.chunk_id
    }

    #[must_use]
    pub fn data_range(&self) -> &str {
        &self.data_range
    }

    #[must_use]
    pub fn header(&self) -> Option<&RagRow> {
        self.header.as_ref()
    }

    #[must_use]
    pub fn rows(&self) -> &[RagRow] {
        &self.rows
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RagManifest {
    version: String,
    source_name: String,
    source_sha256: String,
    sheet_name: String,
    sheet_index: usize,
    sheet_visibility: String,
    start_cell: String,
    end_cell: Option<String>,
    has_header: bool,
    chunk_rows: usize,
    max_rows: Option<usize>,
    emitted_rows: usize,
    emitted_chunks: usize,
    jsonl_utf8_bytes: usize,
    approximate_tokens: usize,
    truncated: bool,
    continuation_row: Option<u32>,
    formula_calculation: String,
}

impl RagManifest {
    #[must_use]
    pub fn source_sha256(&self) -> &str {
        &self.source_sha256
    }

    #[must_use]
    pub const fn emitted_rows(&self) -> usize {
        self.emitted_rows
    }

    #[must_use]
    pub const fn emitted_chunks(&self) -> usize {
        self.emitted_chunks
    }

    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.truncated
    }
}

/// A path-based iterator that retains one output chunk and parser buffers at a time.
pub struct RagExport {
    rows: StreamingStructuredRows,
    header: Option<RagRow>,
    options: RagExportOptions,
    manifest: RagManifest,
    finished: bool,
}

impl RagExport {
    #[must_use]
    pub const fn manifest(&self) -> &RagManifest {
        &self.manifest
    }

    fn next_chunk(&mut self) -> Result<Option<RagChunk>> {
        if self.finished {
            return Ok(None);
        }
        if self.options.max_rows.is_some_and(|limit| self.manifest.emitted_rows >= limit) {
            if let Some(row) = self.rows.next().transpose()? {
                self.manifest.truncated = true;
                self.manifest.continuation_row = Some(row.row_index());
            }
            self.finished = true;
            return Ok(None);
        }

        let mut rows = Vec::with_capacity(self.options.chunk_rows);
        while rows.len() < self.options.chunk_rows {
            if self
                .options
                .max_rows
                .is_some_and(|limit| self.manifest.emitted_rows + rows.len() >= limit)
            {
                break;
            }
            match self.rows.next() {
                Some(row) => rows.push(RagRow::from(row?)),
                None => {
                    self.finished = true;
                    break;
                }
            }
        }
        if rows.is_empty() {
            return Ok(None);
        }
        let chunk = build_chunk(&self.manifest, self.header.clone(), rows);
        record_chunk(&mut self.manifest, &chunk)?;
        Ok(Some(chunk))
    }
}

impl Iterator for RagExport {
    type Item = Result<RagChunk>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_chunk().transpose()
    }
}

pub(crate) fn export_path(
    path: impl AsRef<Path>,
    options: &ReadOptions,
    export_options: &RagExportOptions,
) -> Result<RagExport> {
    validate_export_options(export_options)?;
    let path = path.as_ref();
    let sheet_info = crate::streaming::sheet_info(path)?;
    let selected_sheet = select_sheet(&sheet_info, options)?;
    enforce_visibility(selected_sheet, export_options)?;
    let hash = hash_reader(BufReader::new(File::open(path)?))?;
    let source_name = export_options.source_name.clone().unwrap_or_else(|| {
        path.file_name().and_then(|name| name.to_str()).unwrap_or("workbook.xlsx").to_owned()
    });
    let mut rows = StreamingStructuredRows::open(path, options)?;
    let header =
        if options.uses_headers(false) { rows.next().transpose()?.map(RagRow::from) } else { None };
    Ok(RagExport {
        rows,
        header,
        options: export_options.clone(),
        manifest: new_manifest(source_name, hash, selected_sheet, options, export_options),
        finished: false,
    })
}

pub(crate) fn export_bytes<F>(
    bytes: &[u8],
    options: &ReadOptions,
    export_options: &RagExportOptions,
    mut visitor: F,
) -> Result<RagManifest>
where
    F: FnMut(&RagChunk) -> Result<()>,
{
    validate_export_options(export_options)?;
    let sheet_info = crate::streaming::sheet_info_from_bytes(bytes)?;
    let selected_sheet = select_sheet(&sheet_info, options)?;
    enforce_visibility(selected_sheet, export_options)?;
    let source_name =
        export_options.source_name.clone().unwrap_or_else(|| "workbook.xlsx".to_owned());
    let hash = format_hash(Sha256::digest(bytes));
    let mut manifest = new_manifest(source_name, hash, selected_sheet, options, export_options);
    let mut header = None;
    let mut pending = Vec::with_capacity(export_options.chunk_rows);
    let mut stopped_at_limit = false;
    visit_structured_rows(bytes, options, |row| {
        if header.is_none() && options.uses_headers(false) {
            header = Some(RagRow::from(row));
            return Ok(true);
        }
        if export_options
            .max_rows
            .is_some_and(|limit| manifest.emitted_rows + pending.len() >= limit)
        {
            stopped_at_limit = true;
            manifest.continuation_row = Some(row.row_index());
            return Ok(false);
        }
        pending.push(RagRow::from(row));
        if pending.len() == export_options.chunk_rows {
            let chunk = build_chunk(&manifest, header.clone(), std::mem::take(&mut pending));
            visitor(&chunk)?;
            record_chunk(&mut manifest, &chunk)?;
            pending = Vec::with_capacity(export_options.chunk_rows);
        }
        Ok(true)
    })?;
    if !pending.is_empty() {
        let chunk = build_chunk(&manifest, header, pending);
        visitor(&chunk)?;
        record_chunk(&mut manifest, &chunk)?;
    }
    if stopped_at_limit {
        manifest.truncated = true;
    }
    Ok(manifest)
}

fn validate_export_options(options: &RagExportOptions) -> Result<()> {
    if options.chunk_rows == 0 {
        return Err(Error::invalid_query("RAG chunk_rows must be greater than zero"));
    }
    if options.max_rows == Some(0) {
        return Err(Error::invalid_query("RAG max_rows must be greater than zero"));
    }
    Ok(())
}

fn select_sheet<'a>(sheet_info: &'a [SheetInfo], options: &ReadOptions) -> Result<&'a SheetInfo> {
    match options.sheet_name() {
        Some(name) => sheet_info
            .iter()
            .find(|sheet| sheet.name() == name)
            .ok_or_else(|| Error::sheet_not_found(name)),
        None => sheet_info.first().ok_or_else(Error::no_worksheets),
    }
}

fn enforce_visibility(sheet: &SheetInfo, options: &RagExportOptions) -> Result<()> {
    if options.allow_hidden_sheets {
        return Ok(());
    }
    match sheet.visibility() {
        SheetVisibility::Visible => Ok(()),
        SheetVisibility::Hidden => Err(Error::hidden_sheet(sheet.name(), "hidden")),
        SheetVisibility::VeryHidden => Err(Error::hidden_sheet(sheet.name(), "very hidden")),
    }
}

fn new_manifest(
    source_name: String,
    source_sha256: String,
    sheet: &SheetInfo,
    read_options: &ReadOptions,
    export_options: &RagExportOptions,
) -> RagManifest {
    RagManifest {
        version: "miniexcel.rag-manifest/v1".to_owned(),
        source_name,
        source_sha256,
        sheet_name: sheet.name().to_owned(),
        sheet_index: sheet.index(),
        sheet_visibility: match sheet.visibility() {
            SheetVisibility::Visible => "visible",
            SheetVisibility::Hidden => "hidden",
            SheetVisibility::VeryHidden => "veryHidden",
        }
        .to_owned(),
        start_cell: read_options.start_cell().to_string(),
        end_cell: read_options.end_cell().map(|cell| cell.to_string()),
        has_header: read_options.uses_headers(false),
        chunk_rows: export_options.chunk_rows,
        max_rows: export_options.max_rows,
        emitted_rows: 0,
        emitted_chunks: 0,
        jsonl_utf8_bytes: 0,
        approximate_tokens: 0,
        truncated: false,
        continuation_row: None,
        formula_calculation: "not-performed; formula values are cached workbook results".to_owned(),
    }
}

fn build_chunk(manifest: &RagManifest, header: Option<RagRow>, rows: Vec<RagRow>) -> RagChunk {
    let first_row = rows.first().expect("chunk has rows").row;
    let last_row = rows.last().expect("chunk has rows").row;
    let (first_column, last_column) = rows
        .iter()
        .flat_map(|row| row.cells.iter().map(|cell| cell.column))
        .fold((u32::MAX, 0_u32), |(first, last), column| (first.min(column), last.max(column)));
    let first_column = if first_column == u32::MAX { 1 } else { first_column };
    let last_column = last_column.max(first_column);
    let start = CellReference::new((first_row - 1) as usize, (first_column - 1) as usize)
        .expect("RAG row is within Excel limits");
    let end = CellReference::new((last_row - 1) as usize, (last_column - 1) as usize)
        .expect("RAG row is within Excel limits");
    let data_range = format!("{start}:{end}");
    RagChunk {
        version: "miniexcel.rag-chunk/v1".to_owned(),
        chunk_id: format!(
            "{}:{}:{}",
            &manifest.source_sha256[..12],
            manifest.sheet_index,
            data_range
        ),
        sheet_name: manifest.sheet_name.clone(),
        sheet_index: manifest.sheet_index,
        data_range,
        header,
        rows,
    }
}

fn record_chunk(manifest: &mut RagManifest, chunk: &RagChunk) -> Result<()> {
    let bytes = serde_json::to_vec(chunk)
        .map_err(|error| Error::stream(format!("cannot serialize RAG chunk: {error}")))?;
    manifest.emitted_rows += chunk.rows.len();
    manifest.emitted_chunks += 1;
    manifest.jsonl_utf8_bytes += bytes.len() + 1;
    manifest.approximate_tokens = manifest.jsonl_utf8_bytes.div_ceil(4);
    Ok(())
}

fn hash_reader(mut reader: impl Read) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format_hash(hasher.finalize()))
}

fn format_hash(hash: impl AsRef<[u8]>) -> String {
    let bytes = hash.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("writing to a string cannot fail");
    }
    output
}
