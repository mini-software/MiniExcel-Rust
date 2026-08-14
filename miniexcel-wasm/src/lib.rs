#![forbid(unsafe_code)]

use std::str::FromStr;

use chrono::NaiveDate;
use miniexcel::{
    CellReference, CellValue, DynamicRow, HeaderMode, MiniExcel, ReadOptions, SheetType,
    SheetVisibility, WriteOptions,
};
use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};
use wasm_bindgen::prelude::*;

const DEFAULT_ROW_LIMIT: usize = 100;
const MAX_SAFE_JAVASCRIPT_INTEGER: i64 = 9_007_199_254_740_991;

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct PreviewOptions {
    sheet_name: Option<String>,
    has_header: bool,
    start_cell: Option<String>,
    end_cell: Option<String>,
    ignore_empty_rows: bool,
    limit: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewResponse {
    sheet_names: Vec<String>,
    sheet_info: Vec<SheetSummary>,
    selected_sheet: String,
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
    cell_types: Vec<Vec<&'static str>>,
    total_rows: usize,
    displayed_rows: usize,
    truncated: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SheetSummary {
    name: String,
    sheet_type: &'static str,
    visibility: &'static str,
}

#[wasm_bindgen(start)]
pub fn initialize() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub fn inspect_xlsx(bytes: &[u8], options_json: &str) -> Result<String, JsValue> {
    let options: PreviewOptions = serde_json::from_str(options_json)
        .map_err(|error| js_error(format!("Invalid preview options: {error}")))?;
    inspect(bytes, options).map_err(|error| js_error(error.to_string()))
}

#[wasm_bindgen]
pub fn create_demo_xlsx() -> Result<Vec<u8>, JsValue> {
    create_demo().map_err(|error| js_error(error.to_string()))
}

#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

fn inspect(bytes: &[u8], options: PreviewOptions) -> miniexcel::Result<String> {
    let sheet_info = MiniExcel::get_sheet_info_from_bytes(bytes)?;
    let sheet_names = sheet_info.iter().map(|sheet| sheet.name().to_owned()).collect::<Vec<_>>();
    let selected_sheet =
        options.sheet_name.clone().or_else(|| sheet_names.first().cloned()).unwrap_or_default();
    let start_cell = options
        .start_cell
        .as_deref()
        .map(CellReference::from_str)
        .transpose()?
        .unwrap_or(CellReference::A1);
    let mut read_options = ReadOptions::new()
        .with_start_cell(start_cell)
        .with_header_mode(if options.has_header { HeaderMode::FirstRow } else { HeaderMode::None })
        .with_ignore_empty_rows(options.ignore_empty_rows);
    if !selected_sheet.is_empty() {
        read_options = read_options.with_sheet_name(&selected_sheet);
    }
    if let Some(end_cell) = options.end_cell.as_deref() {
        read_options = read_options.with_end_cell(CellReference::from_str(end_cell)?);
    }

    let mut source_rows = MiniExcel::query_bytes(bytes, &read_options)?;
    let total_rows = source_rows.len();
    let limit = options.limit.unwrap_or(DEFAULT_ROW_LIMIT);
    if limit != 0 {
        source_rows.truncate(limit);
    }
    let columns = source_rows.first().map_or_else(Vec::new, |row| row.keys().cloned().collect());
    let rows = source_rows
        .iter()
        .map(|row| columns.iter().map(|column| cell_json(row.get(column))).collect())
        .collect::<Vec<_>>();
    let cell_types = source_rows
        .iter()
        .map(|row| columns.iter().map(|column| cell_type(row.get(column))).collect())
        .collect::<Vec<_>>();
    let displayed_rows = rows.len();
    let response = PreviewResponse {
        sheet_names,
        sheet_info: sheet_info
            .into_iter()
            .map(|sheet| SheetSummary {
                name: sheet.name().to_owned(),
                sheet_type: sheet_type_name(sheet.sheet_type()),
                visibility: sheet_visibility_name(sheet.visibility()),
            })
            .collect(),
        selected_sheet,
        columns,
        rows,
        cell_types,
        total_rows,
        displayed_rows,
        truncated: displayed_rows < total_rows,
    };
    Ok(serde_json::to_string(&response).expect("serializable preview response"))
}

fn sheet_type_name(sheet_type: SheetType) -> &'static str {
    match sheet_type {
        SheetType::Worksheet => "worksheet",
        SheetType::DialogSheet => "dialog",
        SheetType::MacroSheet => "macro",
        SheetType::ChartSheet => "chart",
        SheetType::Vba => "vba",
    }
}

fn sheet_visibility_name(visibility: SheetVisibility) -> &'static str {
    match visibility {
        SheetVisibility::Visible => "visible",
        SheetVisibility::Hidden => "hidden",
        SheetVisibility::VeryHidden => "very hidden",
    }
}

fn create_demo() -> miniexcel::Result<Vec<u8>> {
    let mut first = DynamicRow::new();
    first.insert("Name".to_owned(), CellValue::String("MiniExcel".to_owned()));
    first.insert("Version".to_owned(), CellValue::Int(2));
    first.insert("Active".to_owned(), CellValue::Bool(true));
    first.insert("Score".to_owned(), CellValue::Float(98.5));
    first.insert(
        "ReleasedOn".to_owned(),
        CellValue::Date(NaiveDate::from_ymd_opt(2026, 8, 14).expect("valid demo date")),
    );

    let mut second = DynamicRow::new();
    second.insert("Name".to_owned(), CellValue::String("Browser WASM".to_owned()));
    second.insert("Version".to_owned(), CellValue::Int(1));
    second.insert("Active".to_owned(), CellValue::Bool(true));
    second.insert("Score".to_owned(), CellValue::Float(91.25));
    second.insert("ReleasedOn".to_owned(), CellValue::Empty);

    MiniExcel::save_as_bytes(&[first, second], &WriteOptions::new().with_sheet_name("BrowserDemo"))
}

fn cell_json(value: Option<&CellValue>) -> Value {
    match value {
        None | Some(CellValue::Empty) => Value::Null,
        Some(CellValue::Bool(value)) => Value::Bool(*value),
        Some(CellValue::Int(value)) if value.abs() <= MAX_SAFE_JAVASCRIPT_INTEGER => {
            Value::Number(Number::from(*value))
        }
        Some(CellValue::Int(value)) => Value::String(value.to_string()),
        Some(CellValue::Float(value)) => Number::from_f64(*value)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(value.to_string())),
        Some(CellValue::String(value) | CellValue::Error(value)) => Value::String(value.clone()),
        Some(CellValue::Date(value)) => Value::String(value.format("%Y-%m-%d").to_string()),
        Some(CellValue::Time(value)) => Value::String(value.format("%H:%M:%S%.f").to_string()),
        Some(CellValue::DateTime(value)) => {
            Value::String(value.format("%Y-%m-%dT%H:%M:%S%.f").to_string())
        }
        Some(CellValue::Duration(value)) => {
            Value::String(format!("{} ms", value.num_milliseconds()))
        }
    }
}

fn cell_type(value: Option<&CellValue>) -> &'static str {
    match value {
        None | Some(CellValue::Empty) => "empty",
        Some(CellValue::Bool(_)) => "boolean",
        Some(CellValue::Int(_)) => "integer",
        Some(CellValue::Float(_)) => "number",
        Some(CellValue::String(_)) => "string",
        Some(CellValue::Date(_)) => "date",
        Some(CellValue::Time(_)) => "time",
        Some(CellValue::DateTime(_)) => "datetime",
        Some(CellValue::Duration(_)) => "duration",
        Some(CellValue::Error(_)) => "error",
    }
}

fn js_error(message: impl AsRef<str>) -> JsValue {
    JsValue::from_str(message.as_ref())
}

#[cfg(test)]
mod tests {
    use super::{PreviewOptions, create_demo, inspect};
    use serde_json::Value;

    #[test]
    fn generated_demo_can_be_inspected() {
        let bytes = create_demo().expect("create demo workbook");
        let response =
            inspect(&bytes, PreviewOptions { has_header: true, ..PreviewOptions::default() })
                .expect("inspect demo workbook");

        assert!(response.contains("BrowserDemo"));
        assert!(response.contains("MiniExcel"));
        assert!(response.contains("ReleasedOn"));
    }

    #[test]
    fn preview_honors_an_inclusive_end_cell() {
        let bytes = create_demo().expect("create demo workbook");
        let response = inspect(
            &bytes,
            PreviewOptions {
                has_header: true,
                end_cell: Some("B2".to_owned()),
                ..PreviewOptions::default()
            },
        )
        .expect("inspect bounded demo range");
        let response: Value = serde_json::from_str(&response).expect("parse preview response");

        assert_eq!(response["columns"], serde_json::json!(["Name", "Version"]));
        assert_eq!(response["totalRows"], 1);
        assert_eq!(response["rows"][0], serde_json::json!(["MiniExcel", 2]));
    }
}
