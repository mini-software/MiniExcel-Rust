mod common;

use std::io::{Cursor, Write};

use miniexcel::{
    CellValue, DynamicRow, HeaderMode, MiniExcel, QuerySummary, ReadOptions, WriteOptions,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
struct Record {
    name: String,
    value: i64,
}

fn row(name: &str, value: i64) -> DynamicRow {
    let mut row = DynamicRow::new();
    row.insert("Name".to_owned(), CellValue::String(name.to_owned()));
    row.insert("Value".to_owned(), CellValue::Int(value));
    row
}

#[test]
fn writes_to_a_borrowed_writer_and_reads_repeatedly_from_a_borrowed_reader() {
    let rows = [row("Ada", 1), row("Linus", 2)];
    let mut output = Vec::new();
    MiniExcel::save_as_to_writer(&mut output, &rows, &WriteOptions::new())
        .expect("write borrowed output");
    output.write_all(&[]).expect("writer remains usable");

    let mut reader = Cursor::new(output);
    assert_eq!(MiniExcel::get_sheet_names_from_reader(&mut reader).unwrap(), ["Sheet1"]);
    assert_eq!(MiniExcel::get_sheet_dimensions_from_reader(&mut reader).unwrap().len(), 1);
    assert_eq!(MiniExcel::get_sheet_info_from_reader(&mut reader).unwrap()[0].name(), "Sheet1");

    let options = ReadOptions::new().with_header_mode(HeaderMode::FirstRow);
    let mut visited = Vec::new();
    let summary: QuerySummary =
        MiniExcel::visit_rows_from_reader(&mut reader, &options, |excel_row, row| {
            visited.push((excel_row, row["Name"].clone()));
            Ok(false)
        })
        .expect("visit borrowed reader");
    assert_eq!(summary.sheet_name(), "Sheet1");
    assert_eq!(summary.columns(), ["Name", "Value"]);
    assert_eq!(summary.visited_rows(), 1);
    assert_eq!(visited, [(2, CellValue::String("Ada".to_owned()))]);
    assert_eq!(
        MiniExcel::get_columns_from_reader(&mut reader, &options).unwrap(),
        ["Name", "Value"]
    );

    let mut structured_addresses = Vec::new();
    MiniExcel::visit_structured_rows_from_reader(&mut reader, &options, |row| {
        structured_addresses.extend(row.cells().iter().map(|cell| cell.address()));
        Ok(false)
    })
    .expect("visit structured borrowed reader");
    assert_eq!(structured_addresses, ["A1", "B1"]);

    let cache_dir = tempfile::tempdir().expect("create shared-string cache directory");
    MiniExcel::visit_rows_from_reader(
        &mut reader,
        &options
            .clone()
            .with_shared_string_cache_size(1)
            .with_shared_string_cache_path(cache_dir.path()),
        |_, _| Ok(false),
    )
    .expect("visit disk-cached borrowed reader");
    assert!(cache_dir.path().read_dir().unwrap().next().is_none());

    let error = MiniExcel::visit_rows_from_reader(&mut reader, &options, |_, _| {
        Err(miniexcel::Error::from(std::io::Error::other("visitor stopped")))
    })
    .expect_err("propagate visitor errors");
    assert!(error.to_string().contains("visitor stopped"));
}

#[test]
fn visits_typed_rows_and_writes_typed_and_multi_sheet_outputs() {
    let source = std::fs::read(common::fixture("TestTypeMapping.xlsx")).unwrap();
    let mut reader = Cursor::new(source);
    let mut names = Vec::new();
    let summary = MiniExcel::visit_rows_as_from_reader::<Record, _, _>(
        &mut reader,
        &ReadOptions::new(),
        |_, record| {
            names.push(record.name.clone());
            Ok(false)
        },
    );
    assert!(summary.is_err(), "fixture does not match Record and should preserve typed errors");

    let stable = [Record { name: "Stable".to_owned(), value: 1 }];
    let preview = [Record { name: "Preview".to_owned(), value: 2 }];
    let mut typed_output = Vec::new();
    MiniExcel::save_as_serialized_to_writer(&mut typed_output, &stable, &WriteOptions::new())
        .expect("write typed borrowed output");
    let mut typed_reader = Cursor::new(typed_output);
    let mut typed_values = Vec::new();
    MiniExcel::visit_rows_as_from_reader::<Record, _, _>(
        &mut typed_reader,
        &ReadOptions::new(),
        |excel_row, record| {
            typed_values.push((excel_row, record.value));
            Ok(true)
        },
    )
    .expect("read typed borrowed input");
    assert_eq!(typed_values, [(2, 1)]);

    let mut schema_output = Vec::new();
    MiniExcel::save_as_with_schema_to_writer(
        &mut schema_output,
        &["Name".to_owned(), "Value".to_owned()],
        &[],
        &WriteOptions::new(),
    )
    .expect("write borrowed schema output");
    let mut schema_reader = Cursor::new(schema_output);
    assert_eq!(
        MiniExcel::get_columns_from_reader(
            &mut schema_reader,
            &ReadOptions::new().with_header_mode(HeaderMode::FirstRow),
        )
        .unwrap(),
        ["Name", "Value"]
    );

    let mut multi_output = Vec::new();
    let counts = MiniExcel::save_as_sheets_to_writer(
        &mut multi_output,
        [("Stable", [row("Stable", 1)].as_slice()), ("Preview", [row("Preview", 2)].as_slice())],
        &WriteOptions::new(),
    )
    .expect("write borrowed multi-sheet output");
    assert_eq!(counts, [1, 1]);
    let mut multi_reader = Cursor::new(multi_output);
    assert_eq!(
        MiniExcel::get_sheet_names_from_reader(&mut multi_reader).unwrap(),
        ["Stable", "Preview"]
    );

    let mut typed_multi_output = Vec::new();
    let typed_counts = MiniExcel::save_as_serialized_sheets_to_writer(
        &mut typed_multi_output,
        [("Stable", stable.as_slice()), ("Preview", preview.as_slice())],
        &WriteOptions::new(),
    )
    .expect("write borrowed typed multi-sheet output");
    assert_eq!(typed_counts, [1, 1]);
}
