#![forbid(unsafe_code)]

use std::fmt::Write as _;
use std::str::FromStr;

use chrono::NaiveDate;
use miniexcel::{
    CellReference, CellValue, DynamicRow, HeaderMode, MiniExcel, QueryPlan, RagChunk,
    RagExportOptions, RagManifest, ReadOptions, SheetInfo, SheetType, SheetVisibility,
    WriteOptions,
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
    allow_hidden_sheets: bool,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AnalysisResponse {
    selected_sheet: String,
    columns: Vec<String>,
    rows: Vec<AnalysisResponseRow>,
    stats: Value,
    plan: Value,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AnalysisResponseRow {
    values: Vec<Value>,
    cell_types: Vec<&'static str>,
    source_rows: Vec<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RagExportResponse {
    chunks: Vec<RagChunk>,
    chunks_jsonl: String,
    chunks_markdown: String,
    manifest: RagManifest,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SimpleMarkdownResponse {
    markdown: String,
    selected_sheet: String,
    emitted_rows: usize,
}

#[wasm_bindgen]
pub struct WorkbookSession {
    bytes: Vec<u8>,
}

#[wasm_bindgen]
impl WorkbookSession {
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &[u8]) -> Self {
        Self { bytes: bytes.to_vec() }
    }

    #[wasm_bindgen(js_name = inspect)]
    pub fn inspect_workbook(&self, options_json: &str) -> Result<String, JsValue> {
        inspect_xlsx(&self.bytes, options_json)
    }

    #[wasm_bindgen(js_name = analyze)]
    pub fn analyze_workbook(&self, options_json: &str, plan_json: &str) -> Result<String, JsValue> {
        analyze_xlsx(&self.bytes, options_json, plan_json)
    }

    #[wasm_bindgen(js_name = exportRag)]
    pub fn export_rag_workbook(
        &self,
        options_json: &str,
        export_options_json: &str,
    ) -> Result<String, JsValue> {
        export_rag_xlsx(&self.bytes, options_json, export_options_json)
    }

    #[wasm_bindgen(js_name = exportSimpleMarkdown)]
    pub fn export_simple_markdown_workbook(&self, options_json: &str) -> Result<String, JsValue> {
        export_simple_markdown_xlsx(&self.bytes, options_json)
    }
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
pub fn analyze_xlsx(bytes: &[u8], options_json: &str, plan_json: &str) -> Result<String, JsValue> {
    let options: PreviewOptions = serde_json::from_str(options_json)
        .map_err(|error| js_error(format!("Invalid analysis options: {error}")))?;
    let plan: QueryPlan = serde_json::from_str(plan_json)
        .map_err(|error| js_error(format!("Invalid query plan: {error}")))?;
    analyze(bytes, options, &plan).map_err(|error| js_error(error.to_string()))
}

#[wasm_bindgen]
pub fn export_rag_xlsx(
    bytes: &[u8],
    options_json: &str,
    export_options_json: &str,
) -> Result<String, JsValue> {
    let options: PreviewOptions = serde_json::from_str(options_json)
        .map_err(|error| js_error(format!("Invalid RAG read options: {error}")))?;
    let export_options: RagExportOptions = serde_json::from_str(export_options_json)
        .map_err(|error| js_error(format!("Invalid RAG export options: {error}")))?;
    export_rag(bytes, options, &export_options).map_err(|error| js_error(error.to_string()))
}

#[wasm_bindgen]
pub fn export_simple_markdown_xlsx(bytes: &[u8], options_json: &str) -> Result<String, JsValue> {
    let options: PreviewOptions = serde_json::from_str(options_json)
        .map_err(|error| js_error(format!("Invalid Markdown read options: {error}")))?;
    ensure_simple_markdown_visibility(bytes, &options).map_err(js_error)?;
    export_simple_markdown(bytes, options).map_err(|error| js_error(error.to_string()))
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
    let (sheet_info, selected_sheet, read_options) = prepare_read(bytes, &options)?;
    let sheet_names = sheet_info.iter().map(|sheet| sheet.name().to_owned()).collect::<Vec<_>>();
    let limit = options.limit.unwrap_or(DEFAULT_ROW_LIMIT);
    let mut source_rows = Vec::with_capacity(limit.min(DEFAULT_ROW_LIMIT));
    let summary = MiniExcel::visit_rows_from_bytes(bytes, &read_options, |_, row| {
        if limit == 0 || source_rows.len() < limit {
            source_rows.push(row.clone());
        }
        Ok(true)
    })?;
    let total_rows = summary.visited_rows();
    let columns = summary.columns().to_vec();
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
        sheet_info: sheet_summaries(sheet_info),
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

fn analyze(bytes: &[u8], options: PreviewOptions, plan: &QueryPlan) -> miniexcel::Result<String> {
    let (_, selected_sheet, read_options) = prepare_read(bytes, &options)?;
    let result = MiniExcel::analyze_bytes(bytes, &read_options, plan)?;
    let columns = plan
        .group_by()
        .iter()
        .cloned()
        .chain(plan.aggregates().iter().map(|aggregate| aggregate.alias().to_owned()))
        .collect::<Vec<_>>();
    let rows = result
        .rows()
        .iter()
        .map(|row| AnalysisResponseRow {
            values: columns.iter().map(|column| cell_json(row.values().get(column))).collect(),
            cell_types: columns.iter().map(|column| cell_type(row.values().get(column))).collect(),
            source_rows: row.source_rows().to_vec(),
        })
        .collect();
    let response = AnalysisResponse {
        selected_sheet,
        columns,
        rows,
        stats: serde_json::to_value(result.stats()).expect("serializable analysis stats"),
        plan: serde_json::to_value(plan).expect("serializable query plan"),
    };
    Ok(serde_json::to_string(&response).expect("serializable analysis response"))
}

fn export_rag(
    bytes: &[u8],
    options: PreviewOptions,
    export_options: &RagExportOptions,
) -> miniexcel::Result<String> {
    let (_, _, read_options) = prepare_read(bytes, &options)?;
    let mut chunks = Vec::new();
    let mut chunks_jsonl = String::new();
    let mut chunks_markdown = Vec::new();
    let manifest =
        MiniExcel::visit_rag_chunks_from_bytes(bytes, &read_options, export_options, |chunk| {
            chunks_jsonl.push_str(
                &serde_json::to_string(chunk)
                    .map_err(|error| miniexcel::Error::from(std::io::Error::other(error)))?,
            );
            chunks_jsonl.push('\n');
            chunk.write_markdown(&mut chunks_markdown)?;
            chunks.push(chunk.clone());
            Ok(())
        })?;
    let mut markdown_start = Vec::new();
    manifest.write_markdown_stream_start(&mut markdown_start)?;
    let markdown_start_len = markdown_start.len();
    chunks_markdown.reserve(markdown_start_len);
    chunks_markdown.extend_from_slice(&markdown_start);
    chunks_markdown.rotate_right(markdown_start_len);
    manifest.write_markdown_stream_end(&mut chunks_markdown)?;
    let chunks_markdown =
        String::from_utf8(chunks_markdown).expect("Markdown serializer emits UTF-8");
    let response = RagExportResponse { chunks, chunks_jsonl, chunks_markdown, manifest };
    Ok(serde_json::to_string(&response).expect("serializable RAG export response"))
}

fn export_simple_markdown(bytes: &[u8], options: PreviewOptions) -> miniexcel::Result<String> {
    let (_, selected_sheet, read_options) = prepare_read(bytes, &options)?;
    let mut markdown = String::new();
    let mut columns = Vec::new();
    let summary = MiniExcel::visit_rows_from_bytes(bytes, &read_options, |_, row| {
        if columns.is_empty() {
            columns.extend(row.keys().cloned());
            write_simple_table_header(&mut markdown, &columns);
        }
        write_simple_table_row(&mut markdown, &columns, row);
        Ok(true)
    })?;
    if columns.is_empty() {
        columns.extend(summary.columns().iter().cloned());
        if !columns.is_empty() {
            write_simple_table_header(&mut markdown, &columns);
        }
    }
    let response =
        SimpleMarkdownResponse { markdown, selected_sheet, emitted_rows: summary.visited_rows() };
    Ok(serde_json::to_string(&response).expect("serializable Markdown response"))
}

fn ensure_simple_markdown_visibility(bytes: &[u8], options: &PreviewOptions) -> Result<(), String> {
    let sheets = MiniExcel::get_sheet_info_from_bytes(bytes).map_err(|error| error.to_string())?;
    let selected = options
        .sheet_name
        .as_deref()
        .and_then(|name| sheets.iter().find(|sheet| sheet.name() == name))
        .or_else(|| sheets.first())
        .ok_or_else(|| "the workbook does not contain any worksheets".to_owned())?;
    if selected.visibility() != SheetVisibility::Visible && !options.allow_hidden_sheets {
        return Err(format!(
            "Markdown export of {} worksheet '{}' requires explicit opt-in",
            sheet_visibility_name(selected.visibility()),
            selected.name(),
        ));
    }
    Ok(())
}

fn write_simple_table_header(markdown: &mut String, columns: &[String]) {
    markdown.push('|');
    for column in columns {
        let _ = write!(markdown, " {} |", escape_simple_markdown_cell(column));
    }
    markdown.push('\n');
    markdown.push('|');
    for _ in columns {
        markdown.push_str(" --- |");
    }
    markdown.push('\n');
}

fn write_simple_table_row(markdown: &mut String, columns: &[String], row: &DynamicRow) {
    markdown.push('|');
    for column in columns {
        let value = row.get(column).map_or_else(String::new, simple_cell_text);
        let _ = write!(markdown, " {} |", escape_simple_markdown_cell(&value));
    }
    markdown.push('\n');
}

fn simple_cell_text(value: &CellValue) -> String {
    match value {
        CellValue::Empty => String::new(),
        CellValue::Bool(value) => value.to_string(),
        CellValue::Int(value) => value.to_string(),
        CellValue::Float(value) => value.to_string(),
        CellValue::String(value) | CellValue::Error(value) => value.clone(),
        CellValue::Date(value) => value.format("%Y-%m-%d").to_string(),
        CellValue::Time(value) => value.format("%H:%M:%S%.f").to_string(),
        CellValue::DateTime(value) => value.format("%Y-%m-%dT%H:%M:%S%.f").to_string(),
        CellValue::Duration(value) => format!("{} ms", value.num_milliseconds()),
    }
}

fn escape_simple_markdown_cell(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                output.push_str("<br>");
            }
            '\n' => output.push_str("<br>"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '\\' | '|' | '`' | '*' | '_' | '[' | ']' | '(' | ')' | '#' | '+' | '-' | '!' | '~' => {
                output.push('\\');
                output.push(character);
            }
            _ => output.push(character),
        }
    }
    output
}

fn prepare_read(
    bytes: &[u8],
    options: &PreviewOptions,
) -> miniexcel::Result<(Vec<SheetInfo>, String, ReadOptions)> {
    let sheet_info = MiniExcel::get_sheet_info_from_bytes(bytes)?;
    let selected_sheet = options
        .sheet_name
        .clone()
        .or_else(|| sheet_info.first().map(|sheet| sheet.name().to_owned()))
        .unwrap_or_default();
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
    Ok((sheet_info, selected_sheet, read_options))
}

fn sheet_summaries(sheet_info: Vec<SheetInfo>) -> Vec<SheetSummary> {
    sheet_info
        .into_iter()
        .map(|sheet| SheetSummary {
            name: sheet.name().to_owned(),
            sheet_type: sheet_type_name(sheet.sheet_type()),
            visibility: sheet_visibility_name(sheet.visibility()),
        })
        .collect()
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
    let rows = [
        demo_row("MiniExcel", "Core", "East", "Ready", 1_200, true, 98.5, true),
        demo_row("Browser WASM", "Browser", "West", "Ready", 850, true, 91.25, false),
        demo_row("Streaming Query", "Core", "East", "Review", 650, true, 93.0, false),
        demo_row("RAG Export", "AI", "North", "Ready", 1_100, true, 96.0, false),
        demo_row("Parity", "Core", "West", "Held", 400, false, 88.0, false),
        demo_row("CLI", "Tools", "South", "Ready", 730, true, 90.0, false),
    ];
    MiniExcel::save_as_bytes(&rows, &WriteOptions::new().with_sheet_name("BrowserDemo"))
}

#[allow(clippy::too_many_arguments)]
fn demo_row(
    name: &str,
    category: &str,
    region: &str,
    status: &str,
    amount: i64,
    active: bool,
    score: f64,
    has_release_date: bool,
) -> DynamicRow {
    let mut row = DynamicRow::new();
    row.insert("Name".to_owned(), CellValue::String(name.to_owned()));
    row.insert("Category".to_owned(), CellValue::String(category.to_owned()));
    row.insert("Region".to_owned(), CellValue::String(region.to_owned()));
    row.insert("Status".to_owned(), CellValue::String(status.to_owned()));
    row.insert("Amount".to_owned(), CellValue::Int(amount));
    row.insert("Active".to_owned(), CellValue::Bool(active));
    row.insert("Score".to_owned(), CellValue::Float(score));
    row.insert(
        "ReleasedOn".to_owned(),
        if has_release_date {
            CellValue::Date(NaiveDate::from_ymd_opt(2026, 8, 14).expect("valid demo date"))
        } else {
            CellValue::Empty
        },
    );
    row
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
    use super::{
        PreviewOptions, analyze, create_demo, ensure_simple_markdown_visibility, export_rag,
        export_simple_markdown, inspect,
    };
    use miniexcel::{
        AggregateOp, AggregateSpec, ComparisonOp, FilterExpr, MiniExcel, QueryLiteral, QueryPlan,
        RagExportOptions,
    };
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

        assert_eq!(response["columns"], serde_json::json!(["Name", "Category"]));
        assert_eq!(response["totalRows"], 1);
        assert_eq!(response["rows"][0], serde_json::json!(["MiniExcel", "Core"]));
    }

    #[test]
    fn preview_counts_all_rows_but_retains_only_the_limit() {
        let bytes = create_demo().expect("create demo workbook");
        let response = inspect(
            &bytes,
            PreviewOptions { has_header: true, limit: Some(2), ..PreviewOptions::default() },
        )
        .expect("inspect bounded demo preview");
        let response: Value = serde_json::from_str(&response).expect("parse preview response");

        assert_eq!(response["totalRows"], 6);
        assert_eq!(response["displayedRows"], 2);
        assert_eq!(response["rows"].as_array().expect("preview rows").len(), 2);
        assert_eq!(response["truncated"], true);
    }

    #[test]
    fn simple_markdown_exports_all_selected_rows_with_safe_gfm_cells() {
        let mut first = miniexcel::DynamicRow::new();
        first.insert("Name".to_owned(), miniexcel::CellValue::String("Alice | Admin".to_owned()));
        first.insert(
            "Note".to_owned(),
            miniexcel::CellValue::String("<b>line 1</b>\n[link](https://example.com)".to_owned()),
        );
        let bytes = MiniExcel::save_as_bytes(
            &[first],
            &miniexcel::WriteOptions::new().with_sheet_name("Orders"),
        )
        .expect("create Markdown workbook");
        let response = export_simple_markdown(
            &bytes,
            PreviewOptions { has_header: true, limit: Some(1), ..PreviewOptions::default() },
        )
        .expect("export simple Markdown");
        let response: Value = serde_json::from_str(&response).expect("parse Markdown response");

        assert_eq!(response["selectedSheet"], "Orders");
        assert_eq!(response["emittedRows"], 1);
        assert_eq!(
            response["markdown"],
            "| Name | Note |\n| --- | --- |\n| Alice \\| Admin | &lt;b&gt;line 1&lt;/b&gt;<br>\\[link\\]\\(https://example.com\\) |\n"
        );
    }

    #[test]
    fn simple_markdown_requires_hidden_sheet_opt_in() {
        let bytes = include_bytes!("../../tests/data/xlsx/TestMultiSheetWithHiddenSheet.xlsx");
        let mut options = PreviewOptions {
            sheet_name: Some("HiddenSheet4".to_owned()),
            has_header: true,
            ..PreviewOptions::default()
        };

        let error = ensure_simple_markdown_visibility(bytes, &options)
            .expect_err("hidden worksheet must require opt-in");
        assert!(error.contains("requires explicit opt-in"));

        options.allow_hidden_sheets = true;
        ensure_simple_markdown_visibility(bytes, &options)
            .expect("explicit opt-in permits hidden worksheet export");
    }

    #[test]
    fn analysis_and_rag_exports_use_core_streaming_contracts() {
        let bytes = create_demo().expect("create demo workbook");
        let plan = QueryPlan::new([
            AggregateSpec::count_all("rows"),
            AggregateSpec::column(AggregateOp::Sum, "Amount", "totalAmount"),
        ])
        .with_filter(FilterExpr::compare(
            "Status",
            ComparisonOp::Eq,
            QueryLiteral::String("Ready".to_owned()),
        ))
        .with_group_by(["Category"]);
        let response = analyze(
            &bytes,
            PreviewOptions { has_header: true, ..PreviewOptions::default() },
            &plan,
        )
        .expect("analyze demo workbook");
        let response: Value = serde_json::from_str(&response).expect("parse analysis response");
        assert_eq!(response["stats"]["seenRows"], 6);
        assert_eq!(response["stats"]["matchedRows"], 4);
        assert_eq!(response["rows"].as_array().expect("analysis rows").len(), 4);

        let response = export_rag(
            &bytes,
            PreviewOptions { has_header: true, ..PreviewOptions::default() },
            &RagExportOptions::new().with_chunk_rows(2).with_source_name("demo.xlsx"),
        )
        .expect("export demo RAG chunks");
        let response: Value = serde_json::from_str(&response).expect("parse RAG response");
        assert_eq!(response["manifest"]["emittedRows"], 6);
        assert_eq!(response["manifest"]["emittedChunks"], 3);
        assert_eq!(response["chunks"].as_array().expect("RAG chunks").len(), 3);
        assert_eq!(response["chunksJsonl"].as_str().expect("JSONL").lines().count(), 3);
        let markdown = response["chunksMarkdown"].as_str().expect("Markdown");
        assert_eq!(markdown.matches("miniexcel:chunk-start").count(), 3);
        assert!(markdown.contains("| _row |"));
        assert!(markdown.contains("miniexcel:stream-end"));
    }
}
