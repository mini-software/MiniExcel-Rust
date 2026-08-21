use miniexcel::{CellValue, MiniExcel, ReadOptions, TemplateOptions};
use rust_xlsxwriter::{Format, Workbook};
use serde_json::json;

#[test]
fn fills_scalars_and_expands_list_rows_across_worksheets() {
    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let template_path = temp_dir.path().join("template.xlsx");
    let output_path = temp_dir.path().join("output.xlsx");

    let mut workbook = Workbook::new();
    let report = workbook.add_worksheet();
    report.set_name("Report").unwrap();
    report.write_string(0, 0, "{{title}}").unwrap();
    report.write_string(1, 0, "Name").unwrap();
    report.write_string(1, 1, "Score").unwrap();
    report.write_string_with_format(2, 0, "{{items.name}}", &Format::new().set_bold()).unwrap();
    report.write_string(2, 1, "{{items.score}}").unwrap();
    report.write_string(2, 2, "{{active}}").unwrap();
    report.write_string(3, 0, "Footer: {{title}}").unwrap();
    let metadata = workbook.add_worksheet();
    metadata.set_name("Metadata").unwrap();
    metadata.write_string(0, 0, "{{title}}").unwrap();
    workbook.save(&template_path).expect("write template");

    MiniExcel::save_as_template(
        &output_path,
        &template_path,
        &json!({
            "title": "Quarterly <Report>",
            "active": true,
            "items": [
                { "name": "Ada", "score": 10 },
                { "name": "Linus", "score": 20 }
            ]
        }),
        &TemplateOptions::new(),
    )
    .expect("fill template");

    assert_eq!(MiniExcel::get_sheet_names(&output_path).unwrap(), ["Report", "Metadata"]);
    let report_rows =
        MiniExcel::query_with_options(&output_path, &ReadOptions::new().with_sheet_name("Report"))
            .unwrap()
            .collect::<miniexcel::Result<Vec<_>>>()
            .unwrap();
    assert_eq!(report_rows[0]["A"], CellValue::String("Quarterly <Report>".to_owned()));
    assert_eq!(report_rows[2]["A"], CellValue::String("Ada".to_owned()));
    assert_eq!(report_rows[2]["B"], CellValue::Int(10));
    assert_eq!(report_rows[2]["C"], CellValue::Bool(true));
    assert_eq!(report_rows[3]["A"], CellValue::String("Linus".to_owned()));
    assert_eq!(report_rows[3]["B"], CellValue::Int(20));
    assert_eq!(report_rows[4]["A"], CellValue::String("Footer: Quarterly <Report>".to_owned()));

    let metadata_rows = MiniExcel::query_with_options(
        &output_path,
        &ReadOptions::new().with_sheet_name("Metadata"),
    )
    .unwrap()
    .collect::<miniexcel::Result<Vec<_>>>()
    .unwrap();
    assert_eq!(metadata_rows[0]["A"], CellValue::String("Quarterly <Report>".to_owned()));

    let structured = MiniExcel::query_structured_with_options(
        &output_path,
        &ReadOptions::new().with_sheet_name("Report"),
    )
    .unwrap()
    .collect::<miniexcel::Result<Vec<_>>>()
    .unwrap();
    assert_ne!(structured[2].cells()[0].style_id(), 0);
    assert_ne!(structured[3].cells()[0].style_id(), 0);
}

#[test]
fn fills_template_bytes_with_native_types_and_safe_strings() {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_string(0, 0, "{{count}}").unwrap();
    sheet.write_string(0, 1, "{{enabled}}").unwrap();
    sheet.write_string(0, 2, "Value: {{text}}").unwrap();
    sheet.write_string(1, 0, "{{formula_like}}").unwrap();
    sheet.write_string(2, 0, "{{missing}}").unwrap();
    let template = workbook.save_to_buffer().expect("write template bytes");

    let output = MiniExcel::save_as_template_bytes(
        &template,
        &json!({
            "count": 42,
            "enabled": true,
            "text": "A & B < C",
            "formula_like": "=1+1"
        }),
        &TemplateOptions::new(),
    )
    .expect("fill template bytes");
    let rows = MiniExcel::query_bytes(&output, &ReadOptions::new()).expect("read output bytes");

    assert_eq!(rows[0]["A"], CellValue::Int(42));
    assert_eq!(rows[0]["B"], CellValue::Bool(true));
    assert_eq!(rows[0]["C"], CellValue::String("Value: A & B < C".to_owned()));
    assert_eq!(rows[1]["A"], CellValue::String("'=1+1".to_owned()));
    assert!(rows[2]["A"].is_empty());

    let error = MiniExcel::save_as_template_bytes(
        &template,
        &json!({}),
        &TemplateOptions::new().with_ignore_missing_variables(false),
    )
    .expect_err("strict template variables");
    assert!(error.to_string().contains("count"));
}

#[test]
fn keeps_one_blank_template_row_for_an_empty_list_and_requires_explicit_overwrite() {
    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let template_path = temp_dir.path().join("template.xlsx");
    let output_path = temp_dir.path().join("output.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_string(0, 0, "{{items.name}}").unwrap();
    sheet.write_string(1, 0, "Footer").unwrap();
    workbook.save(&template_path).expect("write template");

    MiniExcel::save_as_template(
        &output_path,
        &template_path,
        &json!({ "items": [] }),
        &TemplateOptions::new(),
    )
    .expect("fill empty list");
    MiniExcel::save_as_template(
        &output_path,
        &template_path,
        &json!({ "items": [] }),
        &TemplateOptions::new(),
    )
    .expect_err("reject overwrite by default");
    MiniExcel::save_as_template(
        &output_path,
        &template_path,
        &json!({ "items": [] }),
        &TemplateOptions::new().with_overwrite_file(true),
    )
    .expect("overwrite explicitly");

    let rows =
        MiniExcel::query(&output_path).unwrap().collect::<miniexcel::Result<Vec<_>>>().unwrap();
    assert!(rows[0]["A"].is_empty());
    assert_eq!(rows[1]["A"], CellValue::String("Footer".to_owned()));
}
