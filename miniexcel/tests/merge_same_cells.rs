use std::io::{Cursor, Read};

use miniexcel::{CellValue, HeaderMode, MergeSameCellsOptions, MiniExcel, ReadOptions};
use quick_xml::Reader;
use quick_xml::events::Event;
use rust_xlsxwriter::{Format, Workbook};
use zip::ZipArchive;

mod common;

#[test]
fn merges_tagged_cells_with_dotnet_fixture_parity() {
    let output = MiniExcel::merge_same_cells_bytes(&fixture("TestMergeWithTag.xlsx")).unwrap();

    assert_eq!(merge_refs(&output), ["A2:A4", "C3:C4", "A7:A8"]);
    assert_markers_removed(&output);
}

#[test]
fn limits_merges_by_the_mergelimit_column() {
    let output = MiniExcel::merge_same_cells_bytes(&fixture("TestMergeWithLimitTag.xlsx")).unwrap();

    assert_eq!(merge_refs(&output), ["A3:A4", "C3:C6", "A5:A6"]);
    assert_markers_removed(&output);
}

#[test]
fn atomically_writes_a_separate_destination_with_explicit_overwrite() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.xlsx");
    let destination = directory.path().join("merged.xlsx");
    let source_bytes = fixture("TestMergeWithTag.xlsx");
    std::fs::write(&source, &source_bytes).unwrap();

    MiniExcel::merge_same_cells(&source, &destination, &MergeSameCellsOptions::new()).unwrap();
    assert_eq!(std::fs::read(&source).unwrap(), source_bytes);
    assert_eq!(merge_refs(&std::fs::read(&destination).unwrap()), ["A2:A4", "C3:C4", "A7:A8"]);

    std::fs::write(&destination, b"existing").unwrap();
    let error = MiniExcel::merge_same_cells(&source, &destination, &MergeSameCellsOptions::new())
        .unwrap_err();
    assert!(error.to_string().contains("already exists"));
    assert_eq!(std::fs::read(&destination).unwrap(), b"existing");

    MiniExcel::merge_same_cells(
        &source,
        &destination,
        &MergeSameCellsOptions::new().with_overwrite_file(true),
    )
    .unwrap();
    assert_eq!(merge_refs(&std::fs::read(&destination).unwrap()), ["A2:A4", "C3:C4", "A7:A8"]);

    let source_before = std::fs::read(&source).unwrap();
    let error = MiniExcel::merge_same_cells(
        &source,
        &source,
        &MergeSameCellsOptions::new().with_overwrite_file(true),
    )
    .unwrap_err();
    assert!(error.to_string().contains("source and destination must differ"));
    assert_eq!(std::fs::read(source).unwrap(), source_before);
    assert_no_temporary_files(directory.path());
}

#[test]
fn processes_all_worksheets_and_shifts_existing_merges() {
    let mut workbook = Workbook::new();
    let first = workbook.add_worksheet();
    first.set_name("First").unwrap();
    first.write_string(1, 0, "@merge").unwrap();
    first.write_string(1, 2, "@merge").unwrap();
    first.write_string(2, 0, "Same").unwrap();
    first.write_string(2, 2, "Same").unwrap();
    first.write_string(3, 0, "Same").unwrap();
    first.write_string(3, 2, "Same").unwrap();
    first.write_string(4, 0, "@endmerge").unwrap();
    first.write_string(4, 2, "@endmerge").unwrap();
    first.merge_range(2, 3, 2, 4, "Existing", &Format::new()).unwrap();

    let second = workbook.add_worksheet();
    second.set_name("Second").unwrap();
    second.write_string(0, 0, "@merge").unwrap();
    second.write_string(1, 0, "Other").unwrap();
    second.write_string(2, 0, "Other").unwrap();
    second.write_string(3, 0, "@endmerge").unwrap();

    let output = MiniExcel::merge_same_cells_bytes(&workbook.save_to_buffer().unwrap()).unwrap();
    let first_refs = merge_refs_from_sheet(&output, "xl/worksheets/sheet1.xml");
    assert!(first_refs.contains(&"D2:E2".to_owned()));
    assert!(first_refs.contains(&"A2:A3".to_owned()));
    assert!(first_refs.contains(&"C2:C3".to_owned()));
    assert_eq!(merge_refs_from_sheet(&output, "xl/worksheets/sheet2.xml"), ["A1:A2"]);
}

fn merge_refs(bytes: &[u8]) -> Vec<String> {
    merge_refs_from_sheet(bytes, "xl/worksheets/sheet1.xml")
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(common::fixture(name)).unwrap()
}

fn merge_refs_from_sheet(bytes: &[u8], path: &str) -> Vec<String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut worksheet = String::new();
    archive.by_name(path).unwrap().read_to_string(&mut worksheet).unwrap();
    let mut reader = Reader::from_str(&worksheet);
    let mut refs = Vec::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Start(start) | Event::Empty(start)
                if start.local_name().as_ref() == b"mergeCell" =>
            {
                refs.push(
                    start
                        .try_get_attribute("ref")
                        .unwrap()
                        .unwrap()
                        .decode_and_unescape_value(reader.decoder())
                        .unwrap()
                        .into_owned(),
                );
            }
            Event::Eof => break,
            _ => {}
        }
    }
    refs
}

fn assert_markers_removed(bytes: &[u8]) {
    let rows =
        MiniExcel::query_bytes(bytes, &ReadOptions::new().with_header_mode(HeaderMode::None))
            .unwrap();
    assert!(rows.iter().flat_map(|row| row.values()).all(|value| {
        !matches!(value, CellValue::String(text) if text == "@merge" || text == "@endmerge")
    }));
}

fn assert_no_temporary_files(directory: &std::path::Path) {
    assert!(
        !std::fs::read_dir(directory).unwrap().any(|entry| {
            entry.unwrap().file_name().to_string_lossy().starts_with(".miniexcel-")
        })
    );
}
