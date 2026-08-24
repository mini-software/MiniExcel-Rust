use indexmap::IndexMap;
use std::path::PathBuf;

use crate::{CellReference, SheetVisibility};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HeaderMode {
    #[default]
    Auto,
    None,
    FirstRow,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HorizontalAlignment {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum VerticalAlignment {
    #[default]
    Bottom,
    Center,
    Top,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TableStyle {
    None,
    #[default]
    Default,
}

/// Controls how insert operations handle an existing worksheet with the target name.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ExistingSheetPolicy {
    /// Reject the operation without modifying the workbook.
    #[default]
    Reject,
    /// Replace the existing worksheet.
    ///
    /// Replacement is reserved for a later compatibility stage and is currently rejected.
    Replace,
}

/// Controls how relationships owned by a replaced worksheet are handled.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TargetRelationshipPolicy {
    /// Reject replacement when the target worksheet owns package relationships.
    #[default]
    Reject,
    /// Remove relationship types that MiniExcel explicitly supports.
    ///
    /// Relationship removal is reserved for worksheet replacement and is currently rejected.
    RemoveSupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RgbColor {
    red: u8,
    green: u8,
    blue: u8,
}

impl RgbColor {
    #[must_use]
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    #[must_use]
    pub const fn value(self) -> u32 {
        ((self.red as u32) << 16) | ((self.green as u32) << 8) | self.blue as u32
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeaderStyle {
    wrap_text: bool,
    background_color: RgbColor,
    horizontal_alignment: HorizontalAlignment,
    vertical_alignment: VerticalAlignment,
}

impl HeaderStyle {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            wrap_text: false,
            background_color: RgbColor::new(0x44, 0x72, 0xC4),
            horizontal_alignment: HorizontalAlignment::Left,
            vertical_alignment: VerticalAlignment::Bottom,
        }
    }

    #[must_use]
    pub const fn with_wrap_text(mut self, enabled: bool) -> Self {
        self.wrap_text = enabled;
        self
    }

    #[must_use]
    pub const fn with_background_color(mut self, color: RgbColor) -> Self {
        self.background_color = color;
        self
    }

    #[must_use]
    pub const fn with_horizontal_alignment(mut self, alignment: HorizontalAlignment) -> Self {
        self.horizontal_alignment = alignment;
        self
    }

    #[must_use]
    pub const fn with_vertical_alignment(mut self, alignment: VerticalAlignment) -> Self {
        self.vertical_alignment = alignment;
        self
    }

    pub(crate) const fn wrap_text(self) -> bool {
        self.wrap_text
    }

    pub(crate) const fn background_color(self) -> RgbColor {
        self.background_color
    }

    pub(crate) const fn horizontal_alignment(self) -> HorizontalAlignment {
        self.horizontal_alignment
    }

    pub(crate) const fn vertical_alignment(self) -> VerticalAlignment {
        self.vertical_alignment
    }
}

impl Default for HeaderStyle {
    fn default() -> Self {
        Self::new()
    }
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
            shared_string_cache_path: default_shared_string_cache_path(),
            trim_headers: true,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn default_shared_string_cache_path() -> PathBuf {
    std::env::temp_dir()
}

#[cfg(target_arch = "wasm32")]
fn default_shared_string_cache_path() -> PathBuf {
    PathBuf::new()
}

#[derive(Clone, Debug, PartialEq)]
pub struct WriteOptions {
    sheet_name: String,
    overwrite_file: bool,
    print_header: bool,
    auto_filter: bool,
    right_to_left: bool,
    auto_width: bool,
    wrap_cell_contents: bool,
    horizontal_alignment: HorizontalAlignment,
    vertical_alignment: VerticalAlignment,
    header_style: HeaderStyle,
    table_style: TableStyle,
    min_width: f64,
    max_width: f64,
    freeze_row_count: u32,
    freeze_column_count: u16,
    date_format: String,
    time_format: String,
    datetime_format: String,
    duration_format: String,
    column_formats: IndexMap<String, String>,
    column_widths: IndexMap<String, f64>,
    hidden_columns: IndexMap<String, bool>,
    sheet_visibilities: IndexMap<String, SheetVisibility>,
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
    pub const fn with_auto_filter(mut self, enabled: bool) -> Self {
        self.auto_filter = enabled;
        self
    }

    #[must_use]
    pub const fn with_right_to_left(mut self, enabled: bool) -> Self {
        self.right_to_left = enabled;
        self
    }

    #[must_use]
    pub const fn with_auto_width(mut self, enabled: bool) -> Self {
        self.auto_width = enabled;
        self
    }

    #[must_use]
    pub const fn with_wrap_cell_contents(mut self, enabled: bool) -> Self {
        self.wrap_cell_contents = enabled;
        self
    }

    #[must_use]
    pub const fn with_horizontal_alignment(mut self, alignment: HorizontalAlignment) -> Self {
        self.horizontal_alignment = alignment;
        self
    }

    #[must_use]
    pub const fn with_vertical_alignment(mut self, alignment: VerticalAlignment) -> Self {
        self.vertical_alignment = alignment;
        self
    }

    #[must_use]
    pub const fn with_header_style(mut self, style: HeaderStyle) -> Self {
        self.header_style = style;
        self
    }

    #[must_use]
    pub const fn with_table_style(mut self, style: TableStyle) -> Self {
        self.table_style = style;
        self
    }

    #[must_use]
    pub const fn with_min_width(mut self, width: f64) -> Self {
        self.min_width = width;
        self
    }

    #[must_use]
    pub const fn with_max_width(mut self, width: f64) -> Self {
        self.max_width = width;
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
    pub fn with_column_width(mut self, field_name: impl Into<String>, width: f64) -> Self {
        self.column_widths.insert(field_name.into(), width);
        self
    }

    #[must_use]
    pub fn with_column_hidden(mut self, field_name: impl Into<String>, hidden: bool) -> Self {
        self.hidden_columns.insert(field_name.into(), hidden);
        self
    }

    #[must_use]
    pub fn with_sheet_visibility(
        mut self,
        sheet_name: impl Into<String>,
        visibility: SheetVisibility,
    ) -> Self {
        self.sheet_visibilities.insert(sheet_name.into().to_lowercase(), visibility);
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
    pub(crate) const fn auto_filter(&self) -> bool {
        self.auto_filter
    }

    #[must_use]
    pub(crate) const fn right_to_left(&self) -> bool {
        self.right_to_left
    }

    #[must_use]
    pub(crate) const fn auto_width(&self) -> bool {
        self.auto_width
    }

    #[must_use]
    pub(crate) const fn wrap_cell_contents(&self) -> bool {
        self.wrap_cell_contents
    }

    #[must_use]
    pub(crate) const fn horizontal_alignment(&self) -> HorizontalAlignment {
        self.horizontal_alignment
    }

    #[must_use]
    pub(crate) const fn vertical_alignment(&self) -> VerticalAlignment {
        self.vertical_alignment
    }

    #[must_use]
    pub(crate) const fn header_style(&self) -> HeaderStyle {
        self.header_style
    }

    #[must_use]
    pub(crate) const fn table_style(&self) -> TableStyle {
        self.table_style
    }

    #[must_use]
    pub(crate) const fn min_width(&self) -> f64 {
        self.min_width
    }

    #[must_use]
    pub(crate) const fn max_width(&self) -> f64 {
        self.max_width
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

    #[must_use]
    pub(crate) fn column_width(&self, field_name: &str) -> Option<f64> {
        self.column_widths.get(field_name).copied()
    }

    #[must_use]
    pub(crate) fn column_widths(&self) -> &IndexMap<String, f64> {
        &self.column_widths
    }

    #[must_use]
    pub(crate) fn column_hidden(&self, field_name: &str) -> bool {
        self.hidden_columns.get(field_name).copied().unwrap_or(false)
    }

    #[must_use]
    pub(crate) fn sheet_visibility(&self, sheet_name: &str) -> SheetVisibility {
        self.sheet_visibilities
            .get(&sheet_name.to_lowercase())
            .copied()
            .unwrap_or(SheetVisibility::Visible)
    }

    #[must_use]
    pub(crate) fn sheet_visibilities(&self) -> &IndexMap<String, SheetVisibility> {
        &self.sheet_visibilities
    }
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            sheet_name: "Sheet1".to_owned(),
            overwrite_file: false,
            print_header: true,
            auto_filter: true,
            right_to_left: false,
            auto_width: false,
            wrap_cell_contents: false,
            horizontal_alignment: HorizontalAlignment::Left,
            vertical_alignment: VerticalAlignment::Bottom,
            header_style: HeaderStyle::new(),
            table_style: TableStyle::Default,
            min_width: 8.428_571_43,
            max_width: 200.0,
            freeze_row_count: 1,
            freeze_column_count: 0,
            date_format: "yyyy-mm-dd".to_owned(),
            time_format: "hh:mm:ss".to_owned(),
            datetime_format: "yyyy-mm-dd hh:mm:ss".to_owned(),
            duration_format: "[h]:mm:ss".to_owned(),
            column_formats: IndexMap::new(),
            column_widths: IndexMap::new(),
            hidden_columns: IndexMap::new(),
            sheet_visibilities: IndexMap::new(),
        }
    }
}

/// Options for inserting a worksheet into an XLSX path.
///
/// The contained [`WriteOptions`] configure the generated worksheet. Existing worksheets are
/// rejected by default; replacement policies are exposed for API stability but are not yet
/// implemented. Path insertion is available on native targets only. Inserted worksheets must be
/// visible, and `WriteOptions::with_overwrite_file()` is rejected because workbook replacement is
/// controlled by [`ExistingSheetPolicy`].
#[derive(Clone, Debug, PartialEq)]
pub struct InsertOptions {
    write_options: WriteOptions,
    existing_sheet_policy: ExistingSheetPolicy,
    target_relationship_policy: TargetRelationshipPolicy,
}

impl InsertOptions {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces all worksheet-writing options used by the insert operation.
    #[must_use]
    pub fn with_write_options(mut self, write_options: WriteOptions) -> Self {
        self.write_options = write_options;
        self
    }

    /// Sets the inserted worksheet name while retaining the other write options.
    #[must_use]
    pub fn with_sheet_name(mut self, sheet_name: impl Into<String>) -> Self {
        self.write_options = self.write_options.with_sheet_name(sheet_name);
        self
    }

    /// Enables or disables the worksheet header row.
    #[must_use]
    pub fn with_print_header(mut self, print_header: bool) -> Self {
        self.write_options = self.write_options.with_print_header(print_header);
        self
    }

    #[must_use]
    /// Sets the behavior for an existing worksheet with the requested name.
    ///
    /// [`ExistingSheetPolicy::Replace`] is currently rejected before any output is created.
    pub const fn with_existing_sheet_policy(mut self, policy: ExistingSheetPolicy) -> Self {
        self.existing_sheet_policy = policy;
        self
    }

    #[must_use]
    /// Sets the relationship policy reserved for worksheet replacement.
    ///
    /// [`TargetRelationshipPolicy::RemoveSupported`] is currently rejected.
    pub const fn with_target_relationship_policy(
        mut self,
        policy: TargetRelationshipPolicy,
    ) -> Self {
        self.target_relationship_policy = policy;
        self
    }

    #[must_use]
    pub const fn write_options(&self) -> &WriteOptions {
        &self.write_options
    }

    #[must_use]
    pub const fn existing_sheet_policy(&self) -> ExistingSheetPolicy {
        self.existing_sheet_policy
    }

    #[must_use]
    pub const fn target_relationship_policy(&self) -> TargetRelationshipPolicy {
        self.target_relationship_policy
    }
}

impl Default for InsertOptions {
    fn default() -> Self {
        Self {
            write_options: WriteOptions::new(),
            existing_sheet_policy: ExistingSheetPolicy::Reject,
            target_relationship_policy: TargetRelationshipPolicy::Reject,
        }
    }
}

impl From<WriteOptions> for InsertOptions {
    fn from(write_options: WriteOptions) -> Self {
        Self::new().with_write_options(write_options)
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
