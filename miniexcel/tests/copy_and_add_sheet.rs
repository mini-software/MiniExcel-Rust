mod common;

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::rc::Rc;

use miniexcel::{
    CellValue, DynamicRow, ExistingSheetPolicy, HeaderMode, InsertOptions, MiniExcel, ReadOptions,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct Record {
    name: String,
    value: i64,
}

#[test]
fn copies_source_and_appends_dynamic_sheet_without_modifying_source() {
    let directory = tempfile::tempdir().unwrap();
    let source = common::fixture("TestMultiSheetWithHiddenSheet.xlsx");
    let destination = directory.path().join("copied.xlsx");
    let source_hash = file_hash(&source);
    let source_entries = package_entries(&source);
    let source_info = MiniExcel::get_sheet_info(&source).unwrap();
    let rows = [row("First", 1), row("Second", 2)];

    let count = MiniExcel::copy_and_add_sheet(
        &source,
        &destination,
        &rows,
        &InsertOptions::new().with_sheet_name("Added"),
    )
    .unwrap();

    assert_eq!(count, 2);
    assert_eq!(file_hash(&source), source_hash);
    let destination_entries = package_entries(&destination);
    for (name, payload) in source_entries {
        if ![
            "[Content_Types].xml",
            "xl/workbook.xml",
            "xl/_rels/workbook.xml.rels",
            "xl/styles.xml",
        ]
        .contains(&name.as_str())
        {
            assert_eq!(
                destination_entries.get(&name),
                Some(&payload),
                "unrelated source part '{name}' changed"
            );
        }
    }
    let destination_info = MiniExcel::get_sheet_info(&destination).unwrap();
    assert_eq!(&destination_info[..source_info.len()], source_info);
    assert_eq!(destination_info.last().unwrap().name(), "Added");
    let added = MiniExcel::query_with_options(
        &destination,
        &ReadOptions::new().with_sheet_name("Added").with_header_mode(HeaderMode::FirstRow),
    )
    .unwrap()
    .collect::<miniexcel::Result<Vec<_>>>()
    .unwrap();
    assert_eq!(added.len(), 2);
    assert_eq!(added[1]["Name"], CellValue::String("Second".to_owned()));
    assert_no_temporary_files(directory.path());
}

#[test]
fn supports_schema_serde_and_strict_sheet_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let source = common::fixture("TestMultiSheet.xlsx");
    let schema_destination = directory.path().join("schema.xlsx");
    let pulls = Rc::new(Cell::new(0_usize));
    let observed = Rc::clone(&pulls);
    let rows = [row("Schema", 3)].into_iter().map(move |row| {
        observed.set(observed.get() + 1);
        Ok(row)
    });
    MiniExcel::copy_and_add_sheet_with_schema(
        &source,
        &schema_destination,
        &["Name".to_owned(), "Value".to_owned()],
        rows,
        &InsertOptions::new().with_sheet_name("Schema"),
    )
    .unwrap();
    assert_eq!(pulls.get(), 1);

    let serde_destination = directory.path().join("serde.xlsx");
    MiniExcel::copy_and_add_sheet_serialized(
        &schema_destination,
        &serde_destination,
        &[Record { name: "Typed".to_owned(), value: 4 }],
        &InsertOptions::new().with_sheet_name("Typed"),
    )
    .unwrap();
    assert_eq!(
        MiniExcel::get_sheet_names(&serde_destination).unwrap(),
        ["Sheet1", "Sheet2", "Sheet3", "Schema", "Typed"]
    );

    let replacement = directory.path().join("replacement.xlsx");
    let before = MiniExcel::get_sheet_info(&source).unwrap();
    MiniExcel::copy_and_add_sheet(
        &source,
        &replacement,
        &[row("Replacement", 5)],
        &InsertOptions::new()
            .with_sheet_name("Sheet2")
            .with_existing_sheet_policy(ExistingSheetPolicy::Replace),
    )
    .unwrap();
    let after = MiniExcel::get_sheet_info(&replacement).unwrap();
    assert_eq!(after, before);
    let rows = MiniExcel::query_with_options(
        replacement,
        &ReadOptions::new().with_sheet_name("Sheet2").with_header_mode(HeaderMode::FirstRow),
    )
    .unwrap()
    .collect::<miniexcel::Result<Vec<_>>>()
    .unwrap();
    assert_eq!(rows[0]["Name"], CellValue::String("Replacement".to_owned()));
}

#[test]
fn enforces_destination_policy_and_rejects_source_aliases_before_consuming_rows() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.xlsx");
    fs::copy(common::fixture("TestMultiSheet.xlsx"), &source).unwrap();
    let destination = directory.path().join("destination.xlsx");
    fs::write(&destination, b"existing destination").unwrap();
    let original_destination = fs::read(&destination).unwrap();
    let pulls = Rc::new(Cell::new(0_usize));
    let observed = Rc::clone(&pulls);
    let rows = std::iter::from_fn(move || {
        observed.set(observed.get() + 1);
        Some(Ok(row("Unexpected", 1)))
    });
    let error = MiniExcel::copy_and_add_sheet_with_schema(
        &source,
        &destination,
        &["Name".to_owned(), "Value".to_owned()],
        rows,
        &InsertOptions::new().with_sheet_name("Added"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("already exists"));
    assert_eq!(pulls.get(), 0);
    assert_eq!(fs::read(&destination).unwrap(), original_destination);

    MiniExcel::copy_and_add_sheet(
        &source,
        &destination,
        &[row("Overwritten", 6)],
        &InsertOptions::new().with_sheet_name("Added").with_overwrite_file(true),
    )
    .unwrap();
    assert!(MiniExcel::get_sheet_names(&destination).unwrap().contains(&"Added".to_owned()));

    let source_hash = file_hash(&source);
    let error = MiniExcel::copy_and_add_sheet(
        &source,
        &source,
        &[row("Alias", 7)],
        &InsertOptions::new().with_sheet_name("Added").with_overwrite_file(true),
    )
    .unwrap_err();
    assert!(error.to_string().contains("must differ"));
    assert_eq!(file_hash(&source), source_hash);

    let hard_link = directory.path().join("source-alias.xlsx");
    fs::hard_link(&source, &hard_link).unwrap();
    let error = MiniExcel::copy_and_add_sheet(
        &source,
        &hard_link,
        &[row("Alias", 8)],
        &InsertOptions::new().with_sheet_name("Added").with_overwrite_file(true),
    )
    .unwrap_err();
    assert!(error.to_string().contains("must differ"));
    assert_eq!(file_hash(&source), source_hash);
    assert_no_temporary_files(directory.path());
}

fn row(name: &str, value: i64) -> DynamicRow {
    let mut row = DynamicRow::new();
    row.insert("Name".to_owned(), CellValue::String(name.to_owned()));
    row.insert("Value".to_owned(), CellValue::Int(value));
    row
}

fn file_hash(path: &std::path::Path) -> Vec<u8> {
    Sha256::digest(fs::read(path).unwrap()).to_vec()
}

fn package_entries(path: &std::path::Path) -> BTreeMap<String, Vec<u8>> {
    let mut archive = ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut entries = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let mut payload = Vec::new();
        entry.read_to_end(&mut payload).unwrap();
        entries.insert(entry.name().to_owned(), payload);
    }
    entries
}

fn assert_no_temporary_files(directory: &std::path::Path) {
    let temporary = fs::read_dir(directory)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".miniexcel-"))
        .collect::<Vec<_>>();
    assert!(temporary.is_empty(), "temporary files remain: {temporary:?}");
}
