mod common;

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};

use miniexcel::{MiniExcel, SheetVisibility};
use zip::ZipArchive;
use zip::write::ZipWriter;

const WORKBOOK_PATH: &str = "xl/workbook.xml";

#[test]
fn renames_by_case_insensitive_source_and_preserves_sheet_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("book.xlsx");
    fs::copy(common::fixture("TestMultiSheetWithHiddenSheet.xlsx"), &path).unwrap();
    let before = MiniExcel::get_sheet_info(&path).unwrap();

    MiniExcel::rename_sheet(&path, "hiddensheet4", "Archive").unwrap();

    let after = MiniExcel::get_sheet_info(&path).unwrap();
    assert_eq!(after.len(), before.len());
    for (index, (before, after)) in before.iter().zip(&after).enumerate() {
        assert_eq!(after.index(), index);
        assert_eq!(after.id(), before.id());
        assert_eq!(after.sheet_type(), before.sheet_type());
        assert_eq!(after.visibility(), before.visibility());
        assert_eq!(after.is_active(), before.is_active());
        if index == 3 {
            assert_eq!(after.name(), "Archive");
            assert_eq!(after.visibility(), SheetVisibility::Hidden);
        } else {
            assert_eq!(after.name(), before.name());
        }
    }

    MiniExcel::rename_sheet(&path, "sheet1", "SHEET1").unwrap();
    assert_eq!(MiniExcel::get_sheet_names(&path).unwrap()[1], "SHEET1");
}

#[test]
fn exact_name_noop_and_rejected_renames_leave_source_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("book.xlsx");
    fs::copy(common::fixture("TestMultiSheet.xlsx"), &path).unwrap();

    let original = fs::read(&path).unwrap();
    MiniExcel::rename_sheet(&path, "sheet1", "Sheet1").unwrap();
    assert_eq!(fs::read(&path).unwrap(), original);

    for (source, target, expected) in [
        ("Missing", "Valid", "was not found"),
        ("Bad:Source", "Valid", "invalid character"),
        ("Sheet1", "sheet2", "already in use"),
        ("Sheet1", "", "cannot be blank"),
        ("Sheet1", "Bad:Name", "invalid character"),
        ("Sheet1", "'Bad", "apostrophe"),
        ("Sheet1", "12345678901234567890123456789012", "31 characters"),
    ] {
        let error = MiniExcel::rename_sheet(&path, source, target).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
        assert_eq!(fs::read(&path).unwrap(), original, "{source} -> {target} changed source");
    }
    assert_no_temporary_files(directory.path());
}

#[test]
fn changes_only_workbook_metadata_and_does_not_rewrite_defined_name_formulas() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("book.xlsx");
    write_fixture_with_defined_name(&path);
    let before = package_entries(&path);

    MiniExcel::rename_sheet(&path, "Sheet1", "Renamed Data").unwrap();

    let after = package_entries(&path);
    assert_eq!(after.keys().collect::<Vec<_>>(), before.keys().collect::<Vec<_>>());
    for (name, payload) in &before {
        if name != WORKBOOK_PATH {
            assert_eq!(&after[name], payload, "package part '{name}' changed");
        }
    }
    let workbook = std::str::from_utf8(&after[WORKBOOK_PATH]).unwrap();
    assert!(workbook.contains("name=\"Renamed Data\""));
    assert!(workbook.contains("'Sheet1'!$A$1"));
    assert!(!workbook.contains("name=\"Sheet1\""));
    assert_no_temporary_files(directory.path());
}

fn write_fixture_with_defined_name(path: &std::path::Path) {
    let source = fs::File::open(common::fixture("TestMultiSheetWithHiddenSheet.xlsx")).unwrap();
    let mut source = ZipArchive::new(source).unwrap();
    let destination = fs::File::create(path).unwrap();
    let mut destination = ZipWriter::new(destination);
    for index in 0..source.len() {
        let mut entry = source.by_index(index).unwrap();
        let name = entry.name().to_owned();
        let options = entry.options();
        destination.start_file(&name, options).unwrap();
        if name == WORKBOOK_PATH {
            let mut workbook = String::new();
            entry.read_to_string(&mut workbook).unwrap();
            let workbook = workbook.replace(
                "<calcPr",
                "<definedNames><definedName name=\"OriginalRef\">'Sheet1'!$A$1</definedName></definedNames><calcPr",
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
