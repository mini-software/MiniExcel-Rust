use thiserror::Error;

#[derive(Debug, Error)]
#[error(transparent)]
pub struct Error(#[from] ErrorKind);

#[derive(Debug, Error)]
enum ErrorKind {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to read XLSX data: {0}")]
    Read(#[from] calamine::XlsxError),

    #[error("failed to stream XLSX data: {0}")]
    Stream(String),

    #[error("failed to write XLSX data: {0}")]
    Write(#[from] rust_xlsxwriter::XlsxError),

    #[error("failed to read CSV record {record}: {message}")]
    CsvRead { record: u64, message: String },

    #[error("failed to deserialize CSV record {record}: {message}")]
    CsvDeserialize { record: u64, message: String },

    #[error("failed to write CSV data: {0}")]
    CsvWrite(String),

    #[error("CSV text cannot be represented by encoding '{0}'")]
    CsvEncoding(String),

    #[error("failed to fill XLSX template: {0}")]
    Template(String),

    #[error("invalid A1 cell reference: {0}")]
    InvalidCellReference(String),

    #[error("invalid cell range: end cell {end} precedes start cell {start}")]
    InvalidCellRange { start: String, end: String },

    #[error("worksheet '{0}' was not found")]
    SheetNotFound(String),

    #[error("table '{0}' was not found")]
    TableNotFound(String),

    #[error("invalid table '{name}': {reason}")]
    InvalidTable { name: String, reason: String },

    #[error("invalid comments for worksheet '{sheet}': {reason}")]
    InvalidComments { sheet: String, reason: String },

    #[error("the workbook does not contain any worksheets")]
    NoWorksheets,

    #[error("invalid worksheet name '{name}': {reason}")]
    InvalidSheetName { name: String, reason: &'static str },

    #[error("worksheet name '{0}' is already in use")]
    DuplicateSheetName(String),

    #[error("cannot write headers for an empty data set without an explicit schema")]
    MissingSchema,

    #[error("worksheet data exceeds Excel limits: {rows} rows, {columns} columns")]
    WorksheetLimit { rows: usize, columns: usize },

    #[error("column name '{0}' appears more than once in the schema")]
    DuplicateColumnName(String),

    #[error("invalid write options: {0}")]
    InvalidWriteOptions(String),

    #[error("the workbook must contain at least one visible worksheet")]
    NoVisibleWorksheets,

    #[error("worksheet visibility was configured for unknown worksheet '{0}'")]
    UnknownSheetVisibility(String),

    #[error("failed to inspect XLSX package: {0}")]
    #[cfg(not(target_arch = "wasm32"))]
    InsertPackage(String),

    #[error("unsafe XLSX package: {0}")]
    #[cfg(not(target_arch = "wasm32"))]
    UnsafePackage(String),

    #[error("unsupported XLSX package feature: {0}")]
    #[cfg(not(target_arch = "wasm32"))]
    UnsupportedPackageFeature(String),

    #[error("worksheet '{0}' already exists")]
    #[cfg(not(target_arch = "wasm32"))]
    ExistingWorksheet(String),

    #[error("atomic workbook commit failed: {0}")]
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    AtomicCommit(String),

    #[error("operation was cancelled")]
    #[cfg(all(feature = "async", not(target_arch = "wasm32")))]
    Cancelled,

    #[error("failed to deserialize worksheet '{sheet}' at Excel row {row}: {source}")]
    Deserialize {
        sheet: String,
        row: usize,
        #[source]
        source: calamine::DeError,
    },

    #[error("invalid analytics query: {0}")]
    InvalidQuery(String),

    #[error("analytics failed in worksheet '{sheet}' at {column}{row}: {message}")]
    Analytics { sheet: String, row: usize, column: String, message: String },

    #[error("analytics query exceeded max_groups ({limit})")]
    GroupLimit { limit: usize },

    #[error("RAG export of {visibility} worksheet '{sheet}' requires explicit opt-in")]
    HiddenSheet { sheet: String, visibility: &'static str },
}

impl Error {
    #[cfg(all(feature = "async", not(target_arch = "wasm32")))]
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        matches!(self.0, ErrorKind::Cancelled)
    }

    pub(crate) fn template(message: impl Into<String>) -> Self {
        ErrorKind::Template(message.into()).into()
    }

    pub(crate) fn csv_read(record: u64, message: impl Into<String>) -> Self {
        ErrorKind::CsvRead { record, message: message.into() }.into()
    }

    pub(crate) fn csv_deserialize(record: u64, message: impl Into<String>) -> Self {
        ErrorKind::CsvDeserialize { record, message: message.into() }.into()
    }

    pub(crate) fn csv_write(message: impl Into<String>) -> Self {
        ErrorKind::CsvWrite(message.into()).into()
    }

    pub(crate) fn csv_encoding(encoding: impl Into<String>) -> Self {
        ErrorKind::CsvEncoding(encoding.into()).into()
    }

    pub(crate) fn stream(message: impl Into<String>) -> Self {
        ErrorKind::Stream(message.into()).into()
    }

    pub(crate) fn invalid_cell_reference(reference: impl Into<String>) -> Self {
        ErrorKind::InvalidCellReference(reference.into()).into()
    }

    pub(crate) fn invalid_cell_range(start: impl Into<String>, end: impl Into<String>) -> Self {
        ErrorKind::InvalidCellRange { start: start.into(), end: end.into() }.into()
    }

    pub(crate) fn sheet_not_found(sheet_name: impl Into<String>) -> Self {
        ErrorKind::SheetNotFound(sheet_name.into()).into()
    }

    pub(crate) fn table_not_found(table_name: impl Into<String>) -> Self {
        ErrorKind::TableNotFound(table_name.into()).into()
    }

    pub(crate) fn invalid_table(name: impl Into<String>, reason: impl Into<String>) -> Self {
        ErrorKind::InvalidTable { name: name.into(), reason: reason.into() }.into()
    }

    pub(crate) fn invalid_comments(sheet: impl Into<String>, reason: impl Into<String>) -> Self {
        ErrorKind::InvalidComments { sheet: sheet.into(), reason: reason.into() }.into()
    }

    pub(crate) fn no_worksheets() -> Self {
        ErrorKind::NoWorksheets.into()
    }

    pub(crate) fn invalid_sheet_name(name: impl Into<String>, reason: &'static str) -> Self {
        ErrorKind::InvalidSheetName { name: name.into(), reason }.into()
    }

    pub(crate) fn duplicate_sheet_name(name: impl Into<String>) -> Self {
        ErrorKind::DuplicateSheetName(name.into()).into()
    }

    pub(crate) fn missing_schema() -> Self {
        ErrorKind::MissingSchema.into()
    }

    pub(crate) fn worksheet_limit(rows: usize, columns: usize) -> Self {
        ErrorKind::WorksheetLimit { rows, columns }.into()
    }

    pub(crate) fn duplicate_column_name(name: impl Into<String>) -> Self {
        ErrorKind::DuplicateColumnName(name.into()).into()
    }

    pub(crate) fn invalid_write_options(message: impl Into<String>) -> Self {
        ErrorKind::InvalidWriteOptions(message.into()).into()
    }

    pub(crate) fn no_visible_worksheets() -> Self {
        ErrorKind::NoVisibleWorksheets.into()
    }

    pub(crate) fn unknown_sheet_visibility(sheet_name: impl Into<String>) -> Self {
        ErrorKind::UnknownSheetVisibility(sheet_name.into()).into()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn insert_package(message: impl Into<String>) -> Self {
        ErrorKind::InsertPackage(message.into()).into()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn unsafe_package(message: impl Into<String>) -> Self {
        ErrorKind::UnsafePackage(message.into()).into()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn unsupported_package_feature(message: impl Into<String>) -> Self {
        ErrorKind::UnsupportedPackageFeature(message.into()).into()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn existing_worksheet(sheet_name: impl Into<String>) -> Self {
        ErrorKind::ExistingWorksheet(sheet_name.into()).into()
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn atomic_commit(message: impl Into<String>) -> Self {
        ErrorKind::AtomicCommit(message.into()).into()
    }

    #[cfg(all(feature = "async", not(target_arch = "wasm32")))]
    pub(crate) fn cancelled() -> Self {
        ErrorKind::Cancelled.into()
    }

    pub(crate) fn deserialize(
        sheet: impl Into<String>,
        row: usize,
        source: calamine::DeError,
    ) -> Self {
        ErrorKind::Deserialize { sheet: sheet.into(), row, source }.into()
    }

    pub(crate) fn invalid_query(message: impl Into<String>) -> Self {
        ErrorKind::InvalidQuery(message.into()).into()
    }

    pub(crate) fn analytics(
        sheet: impl Into<String>,
        row: usize,
        column: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        ErrorKind::Analytics {
            sheet: sheet.into(),
            row,
            column: column.into(),
            message: message.into(),
        }
        .into()
    }

    pub(crate) fn group_limit(limit: usize) -> Self {
        ErrorKind::GroupLimit { limit }.into()
    }

    pub(crate) fn hidden_sheet(sheet: impl Into<String>, visibility: &'static str) -> Self {
        ErrorKind::HiddenSheet { sheet: sheet.into(), visibility }.into()
    }
}

impl From<std::io::Error> for Error {
    fn from(source: std::io::Error) -> Self {
        ErrorKind::Io(source).into()
    }
}

impl From<calamine::XlsxError> for Error {
    fn from(source: calamine::XlsxError) -> Self {
        ErrorKind::Read(source).into()
    }
}

impl From<rust_xlsxwriter::XlsxError> for Error {
    fn from(source: rust_xlsxwriter::XlsxError) -> Self {
        ErrorKind::Write(source).into()
    }
}

pub type Result<T> = std::result::Result<T, Error>;
