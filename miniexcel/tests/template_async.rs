#![cfg(all(feature = "async", not(target_arch = "wasm32")))]

use std::future::Future;
use std::pin::Pin;
use std::task::Context;

use futures_executor::block_on;
use futures_util::task::noop_waker_ref;
use miniexcel::{CancellationToken, CellValue, MiniExcel, TemplateOptions};
use rust_xlsxwriter::Workbook;
use serde_json::json;

#[test]
fn async_template_matches_scalar_and_list_rendering() {
    let directory = tempfile::tempdir().unwrap();
    let template = directory.path().join("template.xlsx");
    let output = directory.path().join("output.xlsx");
    write_template(&template);

    block_on(MiniExcel::save_as_template_async(
        &output,
        &template,
        &json!({
            "title": "Async",
            "items": [{ "name": "Ada" }, { "name": "Linus" }]
        }),
        &TemplateOptions::new(),
    ))
    .unwrap();

    let rows = MiniExcel::query(&output).unwrap().collect::<miniexcel::Result<Vec<_>>>().unwrap();
    assert_eq!(rows[0]["A"], CellValue::String("Async".to_owned()));
    assert_eq!(rows[1]["A"], CellValue::String("Ada".to_owned()));
    assert_eq!(rows[2]["A"], CellValue::String("Linus".to_owned()));
    assert_eq!(rows[3]["A"], CellValue::String("Footer".to_owned()));
}

#[test]
fn async_template_renders_enumerable_conditional_blocks() {
    let directory = tempfile::tempdir().unwrap();
    let template = directory.path().join("conditional-template.xlsx");
    let output = directory.path().join("conditional-output.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_string(0, 0, "{{items.name}}").unwrap();
    sheet
        .write_string(0, 1, "@if(name == Jack)\nPrimary\n@else\n{{items.department}}\n@endif")
        .unwrap();
    workbook.save(&template).unwrap();

    block_on(MiniExcel::save_as_template_async(
        &output,
        &template,
        &json!({
            "items": [
                { "name": "Jack", "department": "HR" },
                { "name": "Linus", "department": "Kernel" }
            ]
        }),
        &TemplateOptions::new(),
    ))
    .unwrap();

    let rows = MiniExcel::query(&output).unwrap().collect::<miniexcel::Result<Vec<_>>>().unwrap();
    assert_eq!(rows[0]["B"], CellValue::String("Primary".to_owned()));
    assert_eq!(rows[1]["B"], CellValue::String("Kernel".to_owned()));
}

#[test]
fn async_template_renders_grouped_enumerables() {
    let directory = tempfile::tempdir().unwrap();
    let template = directory.path().join("group-template.xlsx");
    let output = directory.path().join("group-output.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_string(0, 0, "@group").unwrap();
    sheet.write_string(1, 0, "@header{{items.name}}").unwrap();
    sheet.write_string(2, 0, "{{items.name}}").unwrap();
    sheet.write_string(2, 1, "{{items.department}}").unwrap();
    sheet.write_string(3, 0, "@endgroup").unwrap();
    workbook.save(&template).unwrap();

    block_on(MiniExcel::save_as_template_async(
        &output,
        &template,
        &json!({
            "items": [
                { "name": "Jack", "department": "HR" },
                { "name": "Jack", "department": "IT" },
                { "name": "Neo", "department": "IT" }
            ]
        }),
        &TemplateOptions::new(),
    ))
    .unwrap();

    let rows = MiniExcel::query(&output).unwrap().collect::<miniexcel::Result<Vec<_>>>().unwrap();
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0]["A"], CellValue::String("Jack".to_owned()));
    assert_eq!(rows[1]["B"], CellValue::String("HR".to_owned()));
    assert_eq!(rows[2]["B"], CellValue::String("IT".to_owned()));
    assert_eq!(rows[3]["A"], CellValue::String("Neo".to_owned()));
    assert_eq!(rows[4]["B"], CellValue::String("IT".to_owned()));
}

#[test]
fn async_template_renders_formula_cells_and_ranges() {
    let directory = tempfile::tempdir().unwrap();
    let template = directory.path().join("formula-template.xlsx");
    let output = directory.path().join("formula-output.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_string(1, 0, "{{items.qty}}").unwrap();
    sheet.write_string(1, 1, "$=A{{$rowindex}}*2").unwrap();
    sheet.write_string(3, 2, "$=SUM(A{{$enumrowstart}}:A{{$enumrowend}})").unwrap();
    workbook.save(&template).unwrap();

    block_on(MiniExcel::save_as_template_async(
        &output,
        &template,
        &json!({ "items": [{ "qty": 2 }, { "qty": 3 }] }),
        &TemplateOptions::new(),
    ))
    .unwrap();

    let mut reader = std::fs::File::open(&output).unwrap();
    let mut formulas = std::collections::BTreeMap::new();
    MiniExcel::visit_structured_rows_from_reader(
        &mut reader,
        &miniexcel::ReadOptions::new(),
        |row| {
            for cell in row.cells() {
                if let Some(formula) = cell.formula() {
                    formulas.insert(cell.address(), formula.to_owned());
                }
            }
            Ok(true)
        },
    )
    .unwrap();
    assert_eq!(formulas["B2"], "A2*2");
    assert_eq!(formulas["B3"], "A3*2");
    assert_eq!(formulas["C5"], "SUM(A2:A3)");
}

#[test]
fn pre_cancel_and_failures_preserve_destination() {
    let directory = tempfile::tempdir().unwrap();
    let missing_template = directory.path().join("missing.xlsx");
    let missing_output = directory.path().join("cancelled.xlsx");
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let error = block_on(MiniExcel::save_as_template_async_with_cancellation(
        &missing_output,
        &missing_template,
        &json!({}),
        &TemplateOptions::new(),
        cancellation,
    ))
    .unwrap_err();
    assert!(error.is_cancelled());
    assert!(!missing_output.exists());

    let template = directory.path().join("template.xlsx");
    write_template(&template);
    let output = directory.path().join("output.xlsx");
    std::fs::write(&output, b"existing").unwrap();
    let error = block_on(MiniExcel::save_as_template_async(
        &output,
        &template,
        &json!({ "title": "No overwrite", "items": [] }),
        &TemplateOptions::new(),
    ))
    .unwrap_err();
    assert!(error.to_string().contains("already exists"));
    assert_eq!(std::fs::read(&output).unwrap(), b"existing");

    let error = block_on(MiniExcel::save_as_template_async(
        &output,
        &template,
        &json!({}),
        &TemplateOptions::new().with_overwrite_file(true).with_ignore_missing_variables(false),
    ))
    .unwrap_err();
    assert!(error.to_string().contains("title"));
    assert_eq!(std::fs::read(&output).unwrap(), b"existing");

    block_on(MiniExcel::save_as_template_async(
        &output,
        &template,
        &json!({ "title": "Replacement", "items": [] }),
        &TemplateOptions::new().with_overwrite_file(true),
    ))
    .unwrap();
    let rows = MiniExcel::query(&output).unwrap().collect::<miniexcel::Result<Vec<_>>>().unwrap();
    assert_eq!(rows[0]["A"], CellValue::String("Replacement".to_owned()));
}

#[test]
fn dropping_pending_template_future_preserves_output_and_cleans_temp() {
    let directory = tempfile::tempdir().unwrap();
    let template = directory.path().join("template.xlsx");
    write_large_template(&template);
    let output = directory.path().join("output.xlsx");
    std::fs::write(&output, b"existing").unwrap();
    let value = json!({ "title": "Dropped", "items": [] });
    let options = TemplateOptions::new().with_overwrite_file(true);
    let mut future =
        Box::pin(MiniExcel::save_as_template_async(&output, &template, &value, &options));
    let mut context = Context::from_waker(noop_waker_ref());
    let _ = Future::poll(Pin::as_mut(&mut future), &mut context);
    drop(future);

    for _ in 0..20_000 {
        if !has_temporary_files(directory.path()) {
            break;
        }
        std::thread::yield_now();
    }
    assert_eq!(std::fs::read(&output).unwrap(), b"existing");
    assert!(!has_temporary_files(directory.path()));
}

fn write_template(path: &std::path::Path) {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_string(0, 0, "{{title}}").unwrap();
    sheet.write_string(1, 0, "{{items.name}}").unwrap();
    sheet.write_string(2, 0, "Footer").unwrap();
    workbook.save(path).unwrap();
}

fn write_large_template(path: &std::path::Path) {
    let mut workbook = Workbook::new();
    for sheet_index in 0..20 {
        let sheet = workbook.add_worksheet();
        sheet.set_name(format!("Sheet{sheet_index}")).unwrap();
        for row in 0..200 {
            sheet.write_string(row, 0, "{{title}}").unwrap();
        }
    }
    workbook.save(path).unwrap();
}

fn has_temporary_files(directory: &std::path::Path) -> bool {
    std::fs::read_dir(directory)
        .unwrap()
        .any(|entry| entry.unwrap().file_name().to_string_lossy().starts_with(".miniexcel-"))
}
