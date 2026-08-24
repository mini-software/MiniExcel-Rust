use std::collections::HashSet;
use std::fs::OpenOptions;
use std::path::Path;

use indexmap::IndexSet;
use rust_xlsxwriter::{
    Color, CustomSerializeField, Format, FormatAlign, FormatBorder, SerializeFieldOptions,
    Workbook, Worksheet,
};
use serde::Serialize;

use crate::{
    CellValue, DynamicRow, Error, HorizontalAlignment, Result, SheetVisibility, TableStyle,
    VerticalAlignment, WriteOptions,
};

const MAX_EXCEL_ROWS: usize = 1_048_576;
const MAX_EXCEL_COLUMNS: usize = 16_384;

pub(crate) struct XlsxWriter {
    workbook: Workbook,
    sheet_names: HashSet<String>,
    requested_visibilities: HashSet<String>,
    matched_visibilities: HashSet<String>,
    visible_worksheets: usize,
    has_active_worksheet: bool,
}

impl XlsxWriter {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn add_rows(&mut self, rows: &[DynamicRow], options: &WriteOptions) -> Result<()> {
        let mut schema = IndexSet::new();
        for row in rows {
            schema.extend(row.keys().cloned());
        }

        if schema.is_empty() && (!rows.is_empty() || options.print_header()) {
            return Err(Error::missing_schema());
        }

        let schema: Vec<String> = schema.into_iter().collect();
        self.add_rows_with_schema(&schema, rows, options)
    }

    pub(crate) fn add_rows_with_schema(
        &mut self,
        schema: &[String],
        rows: &[DynamicRow],
        options: &WriteOptions,
    ) -> Result<()> {
        validate_sheet_name(options.sheet_name(), &self.sheet_names)?;
        validate_schema(schema)?;
        validate_dimensions(rows.len(), schema.len(), options.print_header())?;

        let mut worksheet = new_worksheet(options)?;

        let mut output_row = 0_u32;
        if options.print_header() {
            let header_format = header_format(options);
            for (column, header) in schema.iter().enumerate() {
                worksheet.write_string_with_format(0, column as u16, header, &header_format)?;
            }
            output_row = 1;
        }

        let formats = CellFormats::new(options);
        let mut widths = AutoWidthCollector::new(schema.len(), options)?;
        for row in rows {
            for (column, header) in schema.iter().enumerate() {
                let value = row.get(header).unwrap_or(&CellValue::Empty);
                write_cell(&mut worksheet, output_row, column as u16, value, &formats)?;
                widths.observe(column, value_width(value));
            }
            output_row += 1;
        }

        widths.apply(&mut worksheet)?;

        if options.auto_filter() && !schema.is_empty() {
            worksheet.autofilter(0, 0, output_row.saturating_sub(1), schema.len() as u16 - 1)?;
        }

        self.push_worksheet(worksheet, options);
        Ok(())
    }

    pub(crate) fn save(&mut self, path: impl AsRef<Path>, overwrite_file: bool) -> Result<()> {
        self.validate_workbook()?;
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .create_new(!overwrite_file)
            .truncate(overwrite_file)
            .open(path)?;
        self.workbook.save_to_writer(file)?;
        Ok(())
    }

    pub(crate) fn save_to_bytes(&mut self) -> Result<Vec<u8>> {
        self.validate_workbook()?;
        Ok(self.workbook.save_to_buffer()?)
    }

    pub(crate) fn add_serialized<T>(&mut self, rows: &[T], options: &WriteOptions) -> Result<()>
    where
        T: Serialize,
    {
        validate_sheet_name(options.sheet_name(), &self.sheet_names)?;
        validate_dimensions(rows.len(), 1, options.print_header())?;
        let Some(first) = rows.first() else {
            if options.print_header() {
                return Err(Error::missing_schema());
            }

            let worksheet = new_worksheet(options)?;
            self.push_worksheet(worksheet, options);
            return Ok(());
        };

        let mut worksheet = new_worksheet(options)?;
        let custom_headers = serialized_field_options(first, options)?;
        let mut header_options = SerializeFieldOptions::new()
            .hide_headers(!options.print_header())
            .set_header_format(header_format(options));
        if !custom_headers.is_empty() {
            header_options = header_options.set_custom_headers(&custom_headers);
        }
        worksheet.serialize_headers_with_options(0, 0, first, &header_options)?;
        for row in rows {
            worksheet.serialize(row)?;
        }

        let mut widths = AutoWidthCollector::new(0, options)?;
        if options.auto_width() {
            for row in rows {
                widths.observe_serialized(row)?;
            }
            widths.apply(&mut worksheet)?;
        }

        if options.auto_filter() {
            let struct_name = std::any::type_name::<T>().rsplit("::").next().unwrap_or_default();
            let (first_row, first_column, last_row, last_column) =
                worksheet.get_serialize_dimensions(struct_name)?;
            worksheet.autofilter(first_row, first_column, last_row, last_column)?;
        }

        self.push_worksheet(worksheet, options);
        Ok(())
    }

    fn push_worksheet(&mut self, mut worksheet: Worksheet, options: &WriteOptions) {
        self.requested_visibilities.extend(options.sheet_visibilities().keys().cloned());
        let normalized_name = normalized_sheet_name(options.sheet_name());
        if options.sheet_visibilities().contains_key(&normalized_name) {
            self.matched_visibilities.insert(normalized_name.clone());
        }
        match options.sheet_visibility(options.sheet_name()) {
            SheetVisibility::Visible => {
                self.visible_worksheets += 1;
                if !self.has_active_worksheet {
                    worksheet.set_active(true);
                    self.has_active_worksheet = true;
                }
            }
            SheetVisibility::Hidden => {
                worksheet.set_hidden(true);
            }
            SheetVisibility::VeryHidden => {
                worksheet.set_very_hidden(true);
            }
        }
        self.workbook.push_worksheet(worksheet);
        self.sheet_names.insert(normalized_name);
    }

    fn validate_workbook(&self) -> Result<()> {
        if let Some(name) =
            self.requested_visibilities.difference(&self.matched_visibilities).next()
        {
            return Err(Error::unknown_sheet_visibility(name));
        }
        if self.visible_worksheets == 0 {
            return Err(Error::no_visible_worksheets());
        }
        Ok(())
    }
}

impl Default for XlsxWriter {
    fn default() -> Self {
        Self {
            workbook: Workbook::new(),
            sheet_names: HashSet::new(),
            requested_visibilities: HashSet::new(),
            matched_visibilities: HashSet::new(),
            visible_worksheets: 0,
            has_active_worksheet: false,
        }
    }
}

fn new_worksheet(options: &WriteOptions) -> Result<Worksheet> {
    let mut worksheet = Worksheet::new();
    worksheet.set_name(options.sheet_name())?;
    worksheet.set_right_to_left(options.right_to_left());
    worksheet.set_freeze_panes(options.freeze_row_count(), options.freeze_column_count())?;
    Ok(worksheet)
}

struct CellFormats {
    blank: Format,
    ordinary: Format,
    date: Format,
    time: Format,
    datetime: Format,
    duration: Format,
}

struct AutoWidthCollector {
    widths: Vec<f64>,
    minimum: f64,
    maximum: f64,
    enabled: bool,
}

impl AutoWidthCollector {
    fn new(columns: usize, options: &WriteOptions) -> Result<Self> {
        validate_auto_width_options(options)?;
        const PADDING: f64 = 5.0 / 7.0;
        Ok(Self {
            widths: vec![options.min_width() + PADDING; columns],
            minimum: options.min_width() + PADDING,
            maximum: options.max_width() + PADDING,
            enabled: options.auto_width(),
        })
    }

    fn observe(&mut self, column: usize, length: Option<usize>) {
        if !self.enabled {
            return;
        }
        let Some(length) = length else {
            return;
        };
        if column >= self.widths.len() {
            self.widths.resize(column + 1, self.minimum);
        }
        const PADDING: f64 = 5.0 / 7.0;
        let width = length as f64 + PADDING;
        self.widths[column] = self.widths[column].max(width).min(self.maximum);
    }

    fn apply(&self, worksheet: &mut Worksheet) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        for (column, width) in self.widths.iter().enumerate() {
            let pixels = (*width * 7.0).round() as u32;
            worksheet.set_column_width_pixels(column as u16, pixels)?;
        }
        Ok(())
    }

    fn observe_serialized<T>(&mut self, row: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        let value = serde_json::to_value(row).map_err(|error| {
            Error::invalid_write_options(format!(
                "cannot inspect serialized row for auto width: {error}"
            ))
        })?;
        let fields = value.as_object().ok_or_else(|| {
            Error::invalid_write_options("auto width requires rows serialized as structs")
        })?;
        for (column, value) in fields.values().enumerate() {
            self.observe(column, json_value_width(value));
        }
        Ok(())
    }
}

fn json_value_width(value: &serde_json::Value) -> Option<usize> {
    match value {
        serde_json::Value::Null => None,
        serde_json::Value::Bool(_) => Some(1),
        serde_json::Value::Number(value) => Some(value.to_string().len()),
        serde_json::Value::String(value) => Some(xml_text_length(value)),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => None,
    }
}

fn validate_auto_width_options(options: &WriteOptions) -> Result<()> {
    let minimum = options.min_width();
    let maximum = options.max_width();
    if !minimum.is_finite() || !maximum.is_finite() || minimum < 0.0 || maximum < 0.0 {
        return Err(Error::invalid_write_options(
            "auto-width bounds must be finite and non-negative",
        ));
    }
    if minimum > maximum {
        return Err(Error::invalid_write_options("auto-width minimum cannot exceed maximum"));
    }
    Ok(())
}

fn value_width(value: &CellValue) -> Option<usize> {
    match value {
        CellValue::Empty => None,
        CellValue::Bool(_) => Some(1),
        CellValue::Int(value) => Some(value.to_string().len()),
        CellValue::Float(value) => Some(value.to_string().len()),
        CellValue::String(value) | CellValue::Error(value) => Some(xml_text_length(value)),
        CellValue::Date(value) => Some(excel_date_serial(*value).to_string().len()),
        CellValue::Time(value) => Some(excel_time_serial(*value).to_string().len()),
        CellValue::DateTime(value) => Some(
            (excel_date_serial(value.date()) + excel_time_serial(value.time())).to_string().len(),
        ),
        CellValue::Duration(value) => {
            Some((value.num_milliseconds() as f64 / 86_400_000.0).to_string().len())
        }
    }
}

fn xml_text_length(value: &str) -> usize {
    value
        .chars()
        .map(|character| match character {
            '&' => 5,
            '<' | '>' => 4,
            _ => character.len_utf16(),
        })
        .sum()
}

fn excel_date_serial(value: chrono::NaiveDate) -> f64 {
    let epoch = chrono::NaiveDate::from_ymd_opt(1899, 12, 30).expect("valid Excel epoch");
    (value - epoch).num_days() as f64
}

fn excel_time_serial(value: chrono::NaiveTime) -> f64 {
    use chrono::Timelike;
    let seconds = value.num_seconds_from_midnight() as f64;
    let nanos = value.nanosecond() as f64 / 1_000_000_000.0;
    (seconds + nanos) / 86_400.0
}

impl CellFormats {
    fn new(options: &WriteOptions) -> Self {
        Self {
            blank: Format::new().set_num_format("@"),
            ordinary: body_format(options, options.wrap_cell_contents(), None),
            date: body_format(options, false, Some(options.date_format())),
            time: body_format(options, false, Some(options.time_format())),
            datetime: body_format(options, false, Some(options.datetime_format())),
            duration: body_format(options, false, Some(options.duration_format())),
        }
    }
}

fn body_format(options: &WriteOptions, wrap: bool, number_format: Option<&str>) -> Format {
    if options.table_style() == TableStyle::None {
        return number_format
            .map_or_else(Format::new, |format| Format::new().set_num_format(format));
    }
    let horizontal = match options.horizontal_alignment() {
        HorizontalAlignment::Left => FormatAlign::General,
        HorizontalAlignment::Center => FormatAlign::Center,
        HorizontalAlignment::Right => FormatAlign::Right,
    };
    let vertical = match options.vertical_alignment() {
        VerticalAlignment::Bottom => FormatAlign::Bottom,
        VerticalAlignment::Center => FormatAlign::VerticalCenter,
        VerticalAlignment::Top => FormatAlign::Top,
    };
    let mut format =
        Format::new().set_border(FormatBorder::Thin).set_align(horizontal).set_align(vertical);
    if wrap {
        format = format.set_text_wrap();
    }
    if let Some(number_format) = number_format {
        format = format.set_num_format(number_format);
    }
    format
}

fn header_format(options: &WriteOptions) -> Format {
    if options.table_style() == TableStyle::None {
        return Format::new();
    }
    let style = options.header_style();
    let horizontal = match style.horizontal_alignment() {
        HorizontalAlignment::Left => FormatAlign::General,
        HorizontalAlignment::Center => FormatAlign::Center,
        HorizontalAlignment::Right => FormatAlign::Right,
    };
    let vertical = match style.vertical_alignment() {
        VerticalAlignment::Bottom => FormatAlign::Bottom,
        VerticalAlignment::Center => FormatAlign::VerticalCenter,
        VerticalAlignment::Top => FormatAlign::Top,
    };
    let mut format = Format::new()
        .set_font_color(Color::White)
        .set_background_color(Color::RGB(style.background_color().value()))
        .set_border(FormatBorder::Thin)
        .set_align(horizontal)
        .set_align(vertical);
    if style.wrap_text() {
        format = format.set_text_wrap();
    }
    format
}

fn write_cell(
    worksheet: &mut Worksheet,
    row: u32,
    column: u16,
    value: &CellValue,
    formats: &CellFormats,
) -> Result<()> {
    match value {
        CellValue::Empty => {
            worksheet.write_blank(row, column, &formats.blank)?;
        }
        CellValue::Bool(value) => {
            worksheet.write_boolean_with_format(row, column, *value, &formats.ordinary)?;
        }
        CellValue::Int(value) => {
            worksheet.write_with_format(row, column, *value, &formats.ordinary)?;
        }
        CellValue::Float(value) => {
            worksheet.write_number_with_format(row, column, *value, &formats.ordinary)?;
        }
        CellValue::String(value) | CellValue::Error(value) => {
            worksheet.write_string_with_format(row, column, value, &formats.ordinary)?;
        }
        CellValue::Date(value) => {
            worksheet.write_datetime_with_format(row, column, value, &formats.date)?;
        }
        CellValue::Time(value) => {
            worksheet.write_datetime_with_format(row, column, value, &formats.time)?;
        }
        CellValue::DateTime(value) => {
            worksheet.write_datetime_with_format(row, column, value, &formats.datetime)?;
        }
        CellValue::Duration(value) => {
            let excel_days = value.num_milliseconds() as f64 / 86_400_000.0;
            worksheet.write_number_with_format(row, column, excel_days, &formats.duration)?;
        }
    }
    Ok(())
}

fn serialized_field_options<T>(
    first: &T,
    options: &WriteOptions,
) -> Result<Vec<CustomSerializeField>>
where
    T: Serialize,
{
    let value = serde_json::to_value(first).map_err(|error| {
        Error::invalid_write_options(format!("cannot inspect serialized row fields: {error}"))
    })?;
    let fields = value.as_object().ok_or_else(|| {
        Error::invalid_write_options("typed writing requires rows serialized as structs")
    })?;
    Ok(fields
        .keys()
        .map(|field_name| {
            let number_format = options.column_formats().get(field_name).map(String::as_str);
            let wrap = options.wrap_cell_contents() && number_format.is_none();
            CustomSerializeField::new(field_name).set_value_format(body_format(
                options,
                wrap,
                number_format,
            ))
        })
        .collect())
}

fn validate_sheet_name(name: &str, existing_names: &HashSet<String>) -> Result<()> {
    if name.is_empty() {
        return Err(invalid_sheet_name(name, "name cannot be blank"));
    }
    if name.chars().count() > 31 {
        return Err(invalid_sheet_name(name, "name cannot exceed 31 characters"));
    }
    if name.chars().any(|character| matches!(character, '[' | ']' | ':' | '*' | '?' | '/' | '\\')) {
        return Err(invalid_sheet_name(name, "name contains an invalid character"));
    }
    if name.starts_with('\'') || name.ends_with('\'') {
        return Err(invalid_sheet_name(name, "name cannot start or end with an apostrophe"));
    }
    if existing_names.contains(&normalized_sheet_name(name)) {
        return Err(Error::duplicate_sheet_name(name));
    }
    Ok(())
}

fn invalid_sheet_name(name: &str, reason: &'static str) -> Error {
    Error::invalid_sheet_name(name, reason)
}

fn normalized_sheet_name(name: &str) -> String {
    name.to_lowercase()
}

fn validate_schema(schema: &[String]) -> Result<()> {
    let mut names = HashSet::with_capacity(schema.len());
    for name in schema {
        if !names.insert(name) {
            return Err(Error::duplicate_column_name(name));
        }
    }
    Ok(())
}

fn validate_dimensions(rows: usize, columns: usize, print_header: bool) -> Result<()> {
    let output_rows = rows.saturating_add(usize::from(print_header));
    if output_rows > MAX_EXCEL_ROWS || columns > MAX_EXCEL_COLUMNS {
        return Err(Error::worksheet_limit(output_rows, columns));
    }
    Ok(())
}
