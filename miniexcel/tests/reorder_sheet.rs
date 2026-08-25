mod common;

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};

use miniexcel::MiniExcel;
use zip::ZipArchive;
use zip::write::ZipWriter;

const WORKBOOK_PATH: &str = "xl/workbook.xml";

#[test]
fn reorders_forward_backward_and_clamps_indices() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("book.xlsx");
    fs::copy(common::fixture("TestMultiSheetWithHiddenSheet.xlsx"), &path).unwrap();

    MiniExcel::reorder_sheet(&path, "sheet2", 99).unwrap();
    assert_eq!(
        MiniExcel::get_sheet_names(&path).unwrap(),
        ["Sheet1", "Sheet3", "HiddenSheet4", "Sheet2"]
    );

    MiniExcel::reorder_sheet(&path, "SHEET2", -5).unwrap();
    assert_eq!(
        MiniExcel::get_sheet_names(&path).unwrap(),
        ["Sheet2", "Sheet1", "Sheet3", "HiddenSheet4"]
    );
}

#[test]
fn remaps_active_first_sheet_and_local_defined_name_ownership() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("book.xlsx");
    write_fixture_with_positional_metadata(&path);
    let before = package_entries(&path);

    MiniExcel::reorder_sheet(&path, "Sheet3", 0).unwrap();

    assert_eq!(
        MiniExcel::get_sheet_names(&path).unwrap(),
        ["Sheet3", "Sheet2", "Sheet1", "HiddenSheet4"]
    );
    let info = MiniExcel::get_sheet_info(&path).unwrap();
    assert!(info[0].is_active());
    assert_eq!(info[0].id(), 3);
    assert_eq!(info[2].id(), 1);

    let after = package_entries(&path);
    assert_eq!(after.keys().collect::<Vec<_>>(), before.keys().collect::<Vec<_>>());
    for (name, payload) in &before {
        if name != WORKBOOK_PATH {
            assert_eq!(&after[name], payload, "package part '{name}' changed");
        }
    }
    let workbook = std::str::from_utf8(&after[WORKBOOK_PATH]).unwrap();
    assert!(workbook.contains("activeTab=\"0\""));
    assert!(workbook.contains("firstSheet=\"1\""));
    assert!(workbook.contains("localSheetId=\"2\""));
    assert!(workbook.contains("'Sheet1'!$A$1"));
    assert!(workbook.contains("name=\"GlobalRef\""));
}

#[test]
fn same_effective_index_and_rejected_reorders_leave_source_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("book.xlsx");
    fs::copy(common::fixture("TestMultiSheet.xlsx"), &path).unwrap();
    let original = fs::read(&path).unwrap();

    MiniExcel::reorder_sheet(&path, "Sheet1", -1).unwrap();
    assert_eq!(fs::read(&path).unwrap(), original);

    for (name, expected) in [("Missing", "was not found"), ("Bad:Name", "invalid character")] {
        let error = MiniExcel::reorder_sheet(&path, name, 1).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    let single_path = directory.path().join("single.xlsx");
    fs::copy(common::fixture("TestTypeMapping.xlsx"), &single_path).unwrap();
    let single_original = fs::read(&single_path).unwrap();
    MiniExcel::reorder_sheet(&single_path, "Worksheet", i32::MAX).unwrap();
    assert_eq!(fs::read(&single_path).unwrap(), single_original);
    assert_no_temporary_files(directory.path());
}

fn write_fixture_with_positional_metadata(path: &std::path::Path) {
    let source = fs::File::open(common::fixture("TestMultiSheetWithHiddenSheet.xlsx")).unwrap();
    let mut source = ZipArchive::new(source).unwrap();
    let destination = fs::File::create(path).unwrap();
    let mut destination = ZipWriter::new(destination);
    for index in 0..source.len() {
        let mut entry = source.by_index(index).unwrap();
        let name = entry.name().to_owned();
        destination.start_file(&name, entry.options()).unwrap();
        if name == WORKBOOK_PATH {
            let mut workbook = String::new();
            entry.read_to_string(&mut workbook).unwrap();
            let workbook = workbook
                .replace(" activeTab=\"2\"", " activeTab=\"2\" firstSheet=\"0\"")
                .replace(
                    "<calcPr",
                    "<definedNames><definedName name=\"LocalRef\" localSheetId=\"1\">'Sheet1'!$A$1</definedName><definedName name=\"GlobalRef\">'Sheet3'!$A$1</definedName></definedNames><calcPr",
                );
            destination.write_all(workbook.as_bytes()).unwrap();
        } else {
            std::io::copy(&mut entry, &mut destination).unwrap();
        }
    }
    destination.finish().unwrap();
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
