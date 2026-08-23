use indexmap::IndexMap;
use std::path::PathBuf;

use crate::CellReference;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HeaderMode {
    #[default]
    Auto,
    None,
    FirstRow,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadOptions {
    sheet_name: Option<String>,
    start_cell: CellReference,
    end_cell: Option<CellReference>,
    header_mode: HeaderMode,
    ignore_empty_rows: bool,
    fill_merged_cells: bool,
    enable_shared_string_cache: bool,
    shared_string_cache_size: u64,
    shared_string_cache_path: PathBuf,
    trim_headers: bool,
}

impl ReadOptions {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_sheet_name(mut self, sheet_name: impl Into<String>) -> Self {
        self.sheet_name = Some(sheet_name.into());
        self
    }

    #[must_use]
    pub const fn with_start_cell(mut self, start_cell: CellReference) -> Self {
        self.start_cell = start_cell;
        self
    }

    #[must_use]
    pub const fn with_end_cell(mut self, end_cell: CellReference) -> Self {
        self.end_cell = Some(end_cell);
        self
    }

    #[must_use]
    pub const fn with_header_mode(mut self, header_mode: HeaderMode) -> Self {
        self.header_mode = header_mode;
        self
    }

    #[must_use]
    pub const fn with_ignore_empty_rows(mut self, ignore_empty_rows: bool) -> Self {
        self.ignore_empty_rows = ignore_empty_rows;
        self
    }

    #[must_use]
    pub const fn with_fill_merged_cells(mut self, fill_merged_cells: bool) -> Self {
        self.fill_merged_cells = fill_merged_cells;
        self
    }

    #[must_use]
    pub const fn with_shared_string_disk_cache(mut self, enabled: bool) -> Self {
        self.enable_shared_string_cache = enabled;
        self
    }

    #[must_use]
    pub const fn with_shared_string_cache_size(mut self, size: u64) -> Self {
        self.shared_string_cache_size = size;
        self
    }

    #[must_use]
    pub fn with_shared_string_cache_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.shared_string_cache_path = path.into();
        self
    }

    #[must_use]
    pub const fn with_trim_headers(mut self, trim_headers: bool) -> Self {
        self.trim_headers = trim_headers;
        self
    }

    #[must_use]
    pub(crate) fn sheet_name(&self) -> Option<&str> {
        self.sheet_name.as_deref()
    }

    #[must_use]
    pub(crate) const fn start_cell(&self) -> CellReference {
        self.start_cell
    }

    #[must_use]
    pub(crate) const fn end_cell(&self) -> Option<CellReference> {
        self.end_cell
    }

    #[must_use]
    pub(crate) const fn ignore_empty_rows(&self) -> bool {
        self.ignore_empty_rows
    }

    #[must_use]
    pub(crate) const fn fill_merged_cells(&self) -> bool {
        self.fill_merged_cells
    }

    #[must_use]
    pub(crate) const fn shared_string_disk_cache(&self) -> bool {
        self.enable_shared_string_cache
    }

    #[must_use]
    pub(crate) const fn shared_string_cache_size(&self) -> u64 {
        self.shared_string_cache_size
    }

    #[must_use]
    pub(crate) fn shared_string_cache_path(&self) -> &std::path::Path {
        &self.shared_string_cache_path
    }

    #[must_use]
    pub(crate) const fn trim_headers(&self) -> bool {
        self.trim_headers
    }

    pub(crate) const fn uses_headers(&self, typed: bool) -> bool {
        match self.header_mode {
            HeaderMode::Auto => typed,
            HeaderMode::None => false,
            HeaderMode::FirstRow => true,
        }
    }
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            sheet_name: None,
            start_cell: CellReference::A1,
            end_cell: None,
            header_mode: HeaderMode::Auto,
            ignore_empty_rows: false,
            fill_merged_cells: false,
            enable_shared_string_cache: true,
            shared_string_cache_size: 5 * 1024 * 1024,
            shared_string_cache_path: std::env::temp_dir(),
            trim_headers: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WriteOptions {
    sheet_name: String,
    overwrite_file: bool,
    print_header: bool,
    freeze_row_count: u32,
    freeze_column_count: u16,
    date_format: String,
    time_format: String,
    datetime_format: String,
    duration_format: String,
    column_formats: IndexMap<String, String>,
}

impl WriteOptions {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_sheet_name(mut self, sheet_name: impl Into<String>) -> Self {
        self.sheet_name = sheet_name.into();
        self
    }

    #[must_use]
    pub const fn with_overwrite_file(mut self, overwrite_file: bool) -> Self {
        self.overwrite_file = overwrite_file;
        self
    }

    #[must_use]
    pub const fn with_print_header(mut self, print_header: bool) -> Self {
        self.print_header = print_header;
        self
    }

    #[must_use]
    pub const fn with_freeze_row_count(mut self, count: u32) -> Self {
        self.freeze_row_count = count;
        self
    }

    #[must_use]
    pub const fn with_freeze_column_count(mut self, count: u16) -> Self {
        self.freeze_column_count = count;
        self
    }

    #[must_use]
    pub fn with_date_format(mut self, format: impl Into<String>) -> Self {
        self.date_format = format.into();
        self
    }

    #[must_use]
    pub fn with_time_format(mut self, format: impl Into<String>) -> Self {
        self.time_format = format.into();
        self
    }

    #[must_use]
    pub fn with_datetime_format(mut self, format: impl Into<String>) -> Self {
        self.datetime_format = format.into();
        self
    }

    #[must_use]
    pub fn with_duration_format(mut self, format: impl Into<String>) -> Self {
        self.duration_format = format.into();
        self
    }

    #[must_use]
    pub fn with_column_format(
        mut self,
        field_name: impl Into<String>,
        format: impl Into<String>,
    ) -> Self {
        self.column_formats.insert(field_name.into(), format.into());
        self
    }

    #[must_use]
    pub(crate) fn sheet_name(&self) -> &str {
        &self.sheet_name
    }

    #[must_use]
    pub(crate) const fn overwrite_file(&self) -> bool {
        self.overwrite_file
    }

    #[must_use]
    pub(crate) const fn print_header(&self) -> bool {
        self.print_header
    }

    #[must_use]
    pub(crate) const fn freeze_row_count(&self) -> u32 {
        self.freeze_row_count
    }

    #[must_use]
    pub(crate) const fn freeze_column_count(&self) -> u16 {
        self.freeze_column_count
    }

    #[must_use]
    pub(crate) fn date_format(&self) -> &str {
        &self.date_format
    }

    #[must_use]
    pub(crate) fn time_format(&self) -> &str {
        &self.time_format
    }

    #[must_use]
    pub(crate) fn datetime_format(&self) -> &str {
        &self.datetime_format
    }

    #[must_use]
    pub(crate) fn duration_format(&self) -> &str {
        &self.duration_format
    }

    #[must_use]
    pub(crate) fn column_formats(&self) -> &IndexMap<String, String> {
        &self.column_formats
    }
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            sheet_name: "Sheet1".to_owned(),
            overwrite_file: false,
            print_header: true,
            freeze_row_count: 1,
            freeze_column_count: 0,
            date_format: "yyyy-mm-dd".to_owned(),
            time_format: "hh:mm:ss".to_owned(),
            datetime_format: "yyyy-mm-dd hh:mm:ss".to_owned(),
            duration_format: "[h]:mm:ss".to_owned(),
            column_formats: IndexMap::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TemplateOptions {
    overwrite_file: bool,
    ignore_missing_variables: bool,
}

impl TemplateOptions {
    #[must_use]
    pub const fn new() -> Self {
        Self { overwrite_file: false, ignore_missing_variables: true }
    }

    #[must_use]
    pub const fn with_overwrite_file(mut self, overwrite_file: bool) -> Self {
        self.overwrite_file = overwrite_file;
        self
    }

    #[must_use]
    pub const fn with_ignore_missing_variables(mut self, ignore: bool) -> Self {
        self.ignore_missing_variables = ignore;
        self
    }

    #[must_use]
    pub(crate) const fn overwrite_file(&self) -> bool {
        self.overwrite_file
    }

    #[must_use]
    pub(crate) const fn ignore_missing_variables(&self) -> bool {
        self.ignore_missing_variables
    }
}

impl Default for TemplateOptions {
    fn default() -> Self {
        Self::new()
    }
}
