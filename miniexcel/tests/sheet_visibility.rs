mod common;

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;

use miniexcel::{MiniExcel, SheetVisibility};
use zip::ZipArchive;

const WORKBOOK_PATH: &str = "xl/workbook.xml";

#[test]
fn changes_visibility_case_insensitively_and_preserves_active_sheet_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("book.xlsx");
    fs::copy(common::fixture("TestMultiSheetWithHiddenSheet.xlsx"), &path).unwrap();
    let before = MiniExcel::get_sheet_info(&path).unwrap();
    assert!(before[2].is_active());

    MiniExcel::set_sheet_visibility(&path, "sheet3", SheetVisibility::Hidden).unwrap();
    let hidden = MiniExcel::get_sheet_info(&path).unwrap();
    assert_eq!(hidden[2].visibility(), SheetVisibility::Hidden);
    assert!(hidden[2].is_active());

    MiniExcel::set_sheet_visibility(&path, "hiddensheet4", SheetVisibility::VeryHidden).unwrap();
    let very_hidden = MiniExcel::get_sheet_info(&path).unwrap();
    assert_eq!(very_hidden[3].visibility(), SheetVisibility::VeryHidden);

    MiniExcel::set_sheet_visibility(&path, "HIDDENSHEET4", SheetVisibility::Visible).unwrap();
    let visible = MiniExcel::get_sheet_info(&path).unwrap();
    assert_eq!(visible[3].visibility(), SheetVisibility::Visible);
    for (before, after) in before.iter().zip(&visible) {
        assert_eq!(after.id(), before.id());
        assert_eq!(after.index(), before.index());
        assert_eq!(after.name(), before.name());
        assert_eq!(after.sheet_type(), before.sheet_type());
        assert_eq!(after.is_active(), before.is_active());
    }
}

#[test]
fn same_state_and_rejected_visibility_changes_leave_source_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("book.xlsx");
    fs::copy(common::fixture("TestMultiSheetWithHiddenSheet.xlsx"), &path).unwrap();
    let original = fs::read(&path).unwrap();

    MiniExcel::set_sheet_visibility(&path, "HiddenSheet4", SheetVisibility::Hidden).unwrap();
    assert_eq!(fs::read(&path).unwrap(), original);

    for (name, expected) in [("Missing", "was not found"), ("Bad:Name", "invalid character")] {
        let error =
            MiniExcel::set_sheet_visibility(&path, name, SheetVisibility::VeryHidden).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    let single_path = directory.path().join("single.xlsx");
    fs::copy(common::fixture("TestTypeMapping.xlsx"), &single_path).unwrap();
    let single_original = fs::read(&single_path).unwrap();
    let error =
        MiniExcel::set_sheet_visibility(&single_path, "Worksheet", SheetVisibility::VeryHidden)
            .unwrap_err();
    assert!(error.to_string().contains("at least one visible worksheet"));
    assert_eq!(fs::read(&single_path).unwrap(), single_original);
    assert_no_temporary_files(directory.path());
}

#[test]
fn visibility_changes_only_workbook_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("book.xlsx");
    fs::copy(common::fixture("TestMultiSheetWithHiddenSheet.xlsx"), &path).unwrap();
    let before = package_entries(&path);

    MiniExcel::set_sheet_visibility(&path, "Sheet1", SheetVisibility::VeryHidden).unwrap();

    let after = package_entries(&path);
    assert_eq!(after.keys().collect::<Vec<_>>(), before.keys().collect::<Vec<_>>());
    for (name, payload) in &before {
        if name != WORKBOOK_PATH {
            assert_eq!(&after[name], payload, "package part '{name}' changed");
        }
    }
    assert_eq!(
        MiniExcel::get_sheet_info(&path).unwrap()[1].visibility(),
        SheetVisibility::VeryHidden
    );
    assert_no_temporary_files(directory.path());
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
