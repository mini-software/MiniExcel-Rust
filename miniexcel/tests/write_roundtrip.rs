use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
use miniexcel::{
    CellValue, DynamicRow, HeaderMode, HorizontalAlignment, MiniExcel, ReadOptions,
    SheetVisibility, VerticalAlignment, WriteOptions,
};
use quick_xml::Reader;
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{Cursor, Read};
use zip::ZipArchive;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
struct Release {
    name: String,
    version: u32,
    #[serde(
        serialize_with = "miniexcel::serde_helpers::serialize_date_to_excel",
        deserialize_with = "miniexcel::serde_helpers::deserialize_date"
    )]
    released_on: NaiveDate,
    #[serde(skip)]
    internal: bool,
}

fn dynamic_row(name: &str, value: i64) -> DynamicRow {
    let mut row = DynamicRow::new();
    row.insert("Name".to_owned(), CellValue::String(name.to_owned()));
    row.insert("Value".to_owned(), CellValue::Int(value));
    row
}

fn archive_xml(bytes: &[u8], path: &str) -> String {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("open generated XLSX");
    let mut worksheet = archive.by_name(path).expect("open XLSX XML entry");
    let mut xml = String::new();
    worksheet.read_to_string(&mut xml).expect("read XLSX XML entry");
    xml
}

fn worksheet_xml(bytes: &[u8]) -> String {
    archive_xml(bytes, "xl/worksheets/sheet1.xml")
}

fn cell_style_index(bytes: &[u8], address: &str) -> usize {
    let xml = worksheet_xml(bytes);
    let mut reader = Reader::from_str(&xml);
    loop {
        match reader.read_event().expect("parse worksheet XML") {
            Event::Start(cell) | Event::Empty(cell) if cell.name().as_ref() == b"c" => {
                let mut cell_address = None;
                let mut style = 0;
                for attribute in cell.attributes().flatten() {
                    if attribute.key.as_ref() == b"r" {
                        cell_address = Some(String::from_utf8_lossy(&attribute.value).into_owned());
                    } else if attribute.key.as_ref() == b"s" {
                        style = String::from_utf8_lossy(&attribute.value).parse().unwrap();
                    }
                }
                if cell_address.as_deref() == Some(address) {
                    return style;
                }
            }
            Event::Eof => panic!("cell {address} not found"),
            _ => {}
        }
    }
}

fn wrapped_style_indexes(bytes: &[u8]) -> HashSet<usize> {
    let xml = archive_xml(bytes, "xl/styles.xml");
    let mut reader = Reader::from_str(&xml);
    let mut in_cell_xfs = false;
    let mut next_index = 0;
    let mut current = None;
    let mut wrapped = HashSet::new();
    loop {
        match reader.read_event().expect("parse styles XML") {
            Event::Start(event) if event.name().as_ref() == b"cellXfs" => in_cell_xfs = true,
            Event::End(event) if event.name().as_ref() == b"cellXfs" => in_cell_xfs = false,
            Event::Start(event) if in_cell_xfs && event.name().as_ref() == b"xf" => {
                current = Some(next_index);
                next_index += 1;
            }
            Event::Empty(event) if in_cell_xfs && event.name().as_ref() == b"xf" => {
                next_index += 1;
            }
            Event::Start(event) | Event::Empty(event)
                if current.is_some() && event.name().as_ref() == b"alignment" =>
            {
                if event.attributes().flatten().any(|attribute| {
                    attribute.key.as_ref() == b"wrapText" && attribute.value.as_ref() == b"1"
                }) {
                    wrapped.insert(current.expect("current cell format"));
                }
            }
            Event::End(event) if event.name().as_ref() == b"xf" => current = None,
            Event::Eof => break,
            _ => {}
        }
    }
    wrapped
}

fn style_alignment(bytes: &[u8], style_index: usize) -> (Option<String>, Option<String>) {
    let xml = archive_xml(bytes, "xl/styles.xml");
    let mut reader = Reader::from_str(&xml);
    let mut in_cell_xfs = false;
    let mut next_index = 0;
    let mut current = None;
    loop {
        match reader.read_event().expect("parse styles XML") {
            Event::Start(event) if event.name().as_ref() == b"cellXfs" => in_cell_xfs = true,
            Event::End(event) if event.name().as_ref() == b"cellXfs" => in_cell_xfs = false,
            Event::Start(event) if in_cell_xfs && event.name().as_ref() == b"xf" => {
                current = Some(next_index);
                next_index += 1;
            }
            Event::Empty(event) if in_cell_xfs && event.name().as_ref() == b"xf" => {
                next_index += 1;
            }
            Event::Start(event) | Event::Empty(event)
                if current == Some(style_index) && event.name().as_ref() == b"alignment" =>
            {
                let mut horizontal = None;
                let mut vertical = None;
                for attribute in event.attributes().flatten() {
                    if attribute.key.as_ref() == b"horizontal" {
                        horizontal = Some(String::from_utf8_lossy(&attribute.value).into_owned());
                    } else if attribute.key.as_ref() == b"vertical" {
                        vertical = Some(String::from_utf8_lossy(&attribute.value).into_owned());
                    }
                }
                return (horizontal, vertical);
            }
            Event::End(event) if event.name().as_ref() == b"xf" => current = None,
            Event::Eof => return (None, None),
            _ => {}
        }
    }
}

#[test]
fn writes_default_custom_and_disabled_freeze_panes() {
    let rows = [dynamic_row("Ada", 1)];

    let default_bytes =
        MiniExcel::save_as_bytes(&rows, &WriteOptions::new()).expect("write default panes");
    let default_xml = worksheet_xml(&default_bytes);
    assert!(default_xml.contains("ySplit=\"1\""));
    assert!(default_xml.contains("topLeftCell=\"A2\""));

    let custom_bytes = MiniExcel::save_as_bytes(
        &rows,
        &WriteOptions::new().with_freeze_row_count(1).with_freeze_column_count(2),
    )
    .expect("write custom panes");
    let custom_xml = worksheet_xml(&custom_bytes);
    assert!(custom_xml.contains("xSplit=\"2\""));
    assert!(custom_xml.contains("ySplit=\"1\""));
    assert!(custom_xml.contains("topLeftCell=\"C2\""));

    let disabled_bytes = MiniExcel::save_as_bytes(
        &rows,
        &WriteOptions::new().with_freeze_row_count(0).with_freeze_column_count(0),
    )
    .expect("write without panes");
    assert!(!worksheet_xml(&disabled_bytes).contains("<pane"));

    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let path = temp_dir.path().join("typed-freeze.xlsx");
    let releases = [Release {
        name: "MiniExcel".to_owned(),
        version: 1,
        released_on: NaiveDate::from_ymd_opt(2026, 8, 23).unwrap(),
        internal: false,
    }];
    MiniExcel::save_as_serialized_with_options(
        &path,
        &releases,
        &WriteOptions::new().with_freeze_row_count(2).with_freeze_column_count(1),
    )
    .expect("write typed panes");
    let typed_xml = worksheet_xml(&std::fs::read(path).unwrap());
    assert!(typed_xml.contains("xSplit=\"1\""));
    assert!(typed_xml.contains("ySplit=\"2\""));
    assert!(typed_xml.contains("topLeftCell=\"B3\""));
}

#[test]
fn writes_auto_filter_ranges() {
    let rows = [dynamic_row("Ada", 1)];

    let default_bytes =
        MiniExcel::save_as_bytes(&rows, &WriteOptions::new()).expect("write default filter");
    let default_xml = worksheet_xml(&default_bytes);
    assert!(default_xml.contains("<autoFilter ref=\"A1:B2\""));
    let workbook_xml = archive_xml(&default_bytes, "xl/workbook.xml");
    assert!(workbook_xml.contains("_xlnm._FilterDatabase"));
    assert!(workbook_xml.contains("$A$1:$B$2"));

    let no_header_xml = worksheet_xml(
        &MiniExcel::save_as_bytes(&rows, &WriteOptions::new().with_print_header(false))
            .expect("write headerless filter"),
    );
    assert!(no_header_xml.contains("<autoFilter ref=\"A1:B1\""));

    let schema = vec!["Name".to_owned(), "Value".to_owned()];
    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let schema_path = temp_dir.path().join("schema.xlsx");
    MiniExcel::save_as_with_schema(&schema_path, &schema, &[], &WriteOptions::new())
        .expect("write schema filter");
    let schema_xml = worksheet_xml(&std::fs::read(schema_path).unwrap());
    assert!(schema_xml.contains("<autoFilter ref=\"A1:B1\""));

    let release = [Release {
        name: "MiniExcel".to_owned(),
        version: 1,
        released_on: NaiveDate::from_ymd_opt(2026, 8, 23).unwrap(),
        internal: false,
    }];
    let typed_path = temp_dir.path().join("typed.xlsx");
    MiniExcel::save_as_serialized_with_options(&typed_path, &release, &WriteOptions::new())
        .expect("write typed filter");
    let typed_xml = worksheet_xml(&std::fs::read(typed_path).unwrap());
    assert!(typed_xml.contains("<autoFilter ref=\"A1:C2\""));

    let disabled_xml = worksheet_xml(
        &MiniExcel::save_as_bytes(&rows, &WriteOptions::new().with_auto_filter(false))
            .expect("write disabled filter"),
    );
    assert!(!disabled_xml.contains("<autoFilter"));

    let empty_xml = worksheet_xml(
        &MiniExcel::save_as_bytes(&[], &WriteOptions::new().with_print_header(false))
            .expect("write zero-column workbook"),
    );
    assert!(!empty_xml.contains("<autoFilter"));
}

#[test]
fn writes_right_to_left_worksheet_view() {
    let rows = [dynamic_row("Ada", 1)];
    let default_xml = worksheet_xml(
        &MiniExcel::save_as_bytes(&rows, &WriteOptions::new()).expect("write default view"),
    );
    assert!(!default_xml.contains("rightToLeft=\"1\""));

    let rtl_xml = worksheet_xml(
        &MiniExcel::save_as_bytes(&rows, &WriteOptions::new().with_right_to_left(true))
            .expect("write right-to-left view"),
    );
    assert!(rtl_xml.contains("rightToLeft=\"1\""));
    assert!(rtl_xml.contains("ySplit=\"1\""));
    assert!(rtl_xml.contains("topLeftCell=\"A2\""));
}

#[test]
fn writes_v1_compatible_auto_widths() {
    let mut first = DynamicRow::new();
    first.insert("Column1".to_owned(), CellValue::String("1".repeat(32)));
    first.insert("Column2".to_owned(), CellValue::String("2".repeat(8)));
    first.insert("Column3".to_owned(), CellValue::String("3".to_owned()));
    first.insert("Column4".to_owned(), CellValue::String("4".repeat(100)));
    let mut second = DynamicRow::new();
    second.insert("Column1".to_owned(), CellValue::String("1".repeat(16)));
    second.insert("Column2".to_owned(), CellValue::String("2".repeat(16)));
    second.insert("Column3".to_owned(), CellValue::String("33".to_owned()));
    second.insert("Column4".to_owned(), CellValue::String("4".repeat(50)));

    let default_xml = worksheet_xml(
        &MiniExcel::save_as_bytes(&[first.clone(), second.clone()], &WriteOptions::new())
            .expect("write without auto width"),
    );
    assert!(!default_xml.contains("<cols>"));

    let auto_xml = worksheet_xml(
        &MiniExcel::save_as_bytes(
            &[first, second],
            &WriteOptions::new().with_auto_width(true).with_max_width(50.0),
        )
        .expect("write auto widths"),
    );
    assert!(auto_xml.contains("min=\"1\" max=\"1\" width=\"32.7109375\""));
    assert!(auto_xml.contains("min=\"2\" max=\"2\" width=\"16.7109375\""));
    assert!(auto_xml.contains("min=\"3\" max=\"3\" width=\"9.140625\""));
    assert!(auto_xml.contains("min=\"4\" max=\"4\" width=\"50.7109375\""));
    assert!(!auto_xml.contains("bestFit="));

    let schema = vec!["This header is deliberately long".to_owned()];
    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let path = temp_dir.path().join("header-only.xlsx");
    MiniExcel::save_as_with_schema(
        &path,
        &schema,
        &[],
        &WriteOptions::new().with_auto_width(true).with_min_width(1.0),
    )
    .expect("write minimum auto width");
    let header_xml = worksheet_xml(&std::fs::read(path).unwrap());
    assert!(header_xml.contains("width=\"1.7109375\""));

    let typed_path = temp_dir.path().join("typed-auto-width.xlsx");
    let releases = [Release {
        name: "N".repeat(20),
        version: 1234,
        released_on: NaiveDate::from_ymd_opt(2026, 8, 23).unwrap(),
        internal: true,
    }];
    MiniExcel::save_as_serialized_with_options(
        &typed_path,
        &releases,
        &WriteOptions::new().with_auto_width(true).with_min_width(1.0),
    )
    .expect("write typed auto widths");
    let typed_xml = worksheet_xml(&std::fs::read(typed_path).unwrap());
    assert!(typed_xml.contains("min=\"1\" max=\"1\" width=\"20.7109375\""));
    assert!(typed_xml.contains("min=\"2\" max=\"2\" width=\"4.7109375\""));
    assert!(!typed_xml.contains("min=\"4\""));

    for options in [
        WriteOptions::new().with_auto_width(true).with_min_width(f64::NAN),
        WriteOptions::new().with_auto_width(true).with_max_width(f64::INFINITY),
        WriteOptions::new().with_auto_width(true).with_min_width(-1.0),
        WriteOptions::new().with_auto_width(true).with_min_width(10.0).with_max_width(5.0),
    ] {
        assert!(MiniExcel::save_as_bytes(&[dynamic_row("Ada", 1)], &options).is_err());
    }
}

#[test]
fn wraps_ordinary_body_cells_without_wrapping_headers_or_formatted_values() {
    let date = NaiveDate::from_ymd_opt(2026, 8, 24).unwrap();
    let mut row = DynamicRow::new();
    row.insert("Text".to_owned(), CellValue::String("line 1\nline 2".to_owned()));
    row.insert("Count".to_owned(), CellValue::Int(42));
    row.insert("Active".to_owned(), CellValue::Bool(true));
    row.insert("Date".to_owned(), CellValue::Date(date));

    let default_bytes = MiniExcel::save_as_bytes(&[row.clone()], &WriteOptions::new())
        .expect("write default cells");
    let default_wrapped = wrapped_style_indexes(&default_bytes);
    assert!(!default_wrapped.contains(&cell_style_index(&default_bytes, "A2")));

    let wrapped_bytes =
        MiniExcel::save_as_bytes(&[row], &WriteOptions::new().with_wrap_cell_contents(true))
            .expect("write wrapped cells");
    let wrapped = wrapped_style_indexes(&wrapped_bytes);
    for address in ["A2", "B2", "C2"] {
        assert!(wrapped.contains(&cell_style_index(&wrapped_bytes, address)));
    }
    assert!(!wrapped.contains(&cell_style_index(&wrapped_bytes, "A1")));
    assert!(!wrapped.contains(&cell_style_index(&wrapped_bytes, "D2")));

    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let path = temp_dir.path().join("typed-wrap.xlsx");
    let releases = [Release {
        name: "line 1\nline 2".to_owned(),
        version: 2,
        released_on: date,
        internal: false,
    }];
    MiniExcel::save_as_serialized_with_options(
        &path,
        &releases,
        &WriteOptions::new()
            .with_wrap_cell_contents(true)
            .with_column_format("ReleasedOn", "yyyy-mm-dd"),
    )
    .expect("write typed wrapped cells");
    let typed_bytes = std::fs::read(path).unwrap();
    let typed_wrapped = wrapped_style_indexes(&typed_bytes);
    assert!(typed_wrapped.contains(&cell_style_index(&typed_bytes, "A2")));
    assert!(typed_wrapped.contains(&cell_style_index(&typed_bytes, "B2")));
    assert!(!typed_wrapped.contains(&cell_style_index(&typed_bytes, "A1")));
    assert!(!typed_wrapped.contains(&cell_style_index(&typed_bytes, "C2")));
}

#[test]
fn aligns_dynamic_and_typed_body_cells_without_aligning_headers() {
    let date = NaiveDate::from_ymd_opt(2026, 8, 24).unwrap();
    let time = NaiveTime::from_hms_opt(10, 30, 0).unwrap();
    let mut row = DynamicRow::new();
    row.insert("Text".to_owned(), CellValue::String("centered".to_owned()));
    row.insert("Count".to_owned(), CellValue::Int(42));
    row.insert("Date".to_owned(), CellValue::Date(date));
    row.insert("Time".to_owned(), CellValue::Time(time));

    let bytes = MiniExcel::save_as_bytes(
        &[row],
        &WriteOptions::new()
            .with_wrap_cell_contents(true)
            .with_horizontal_alignment(HorizontalAlignment::Center)
            .with_vertical_alignment(VerticalAlignment::Top),
    )
    .expect("write aligned dynamic cells");
    for address in ["A2", "B2", "C2", "D2"] {
        assert_eq!(
            style_alignment(&bytes, cell_style_index(&bytes, address)),
            (Some("center".to_owned()), Some("top".to_owned()))
        );
    }
    assert_eq!(style_alignment(&bytes, cell_style_index(&bytes, "A1")), (None, None));
    let wrapped = wrapped_style_indexes(&bytes);
    assert!(wrapped.contains(&cell_style_index(&bytes, "A2")));
    assert!(!wrapped.contains(&cell_style_index(&bytes, "C2")));

    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let path = temp_dir.path().join("typed-alignment.xlsx");
    let releases =
        [Release { name: "right".to_owned(), version: 123, released_on: date, internal: false }];
    MiniExcel::save_as_serialized_with_options(
        &path,
        &releases,
        &WriteOptions::new()
            .with_horizontal_alignment(HorizontalAlignment::Right)
            .with_vertical_alignment(VerticalAlignment::Center)
            .with_column_format("ReleasedOn", "yyyy-mm-dd"),
    )
    .expect("write aligned typed cells");
    let typed = std::fs::read(path).unwrap();
    for address in ["A2", "B2", "C2"] {
        assert_eq!(
            style_alignment(&typed, cell_style_index(&typed, address)),
            (Some("right".to_owned()), Some("center".to_owned()))
        );
    }
    assert_eq!(style_alignment(&typed, cell_style_index(&typed, "A1")), (None, None));

    let default = MiniExcel::save_as_bytes(
        &[dynamic_row("default", 1)],
        &WriteOptions::new()
            .with_horizontal_alignment(HorizontalAlignment::Left)
            .with_vertical_alignment(VerticalAlignment::Bottom),
    )
    .expect("write default alignment");
    let (horizontal, _) = style_alignment(&default, cell_style_index(&default, "B2"));
    assert_ne!(horizontal.as_deref(), Some("left"));
}

#[test]
fn writes_multiple_dynamic_sheets_and_enforces_overwrite_policy() {
    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let path = temp_dir.path().join("output.xlsx");
    let users = [dynamic_row("Ada", 1), dynamic_row("Linus", 2)];
    let departments = [dynamic_row("Engineering", 3)];

    let row_counts = MiniExcel::save_as_sheets(
        &path,
        [("Users", users.as_slice()), ("Departments", departments.as_slice())],
        &WriteOptions::new(),
    )
    .expect("write multiple worksheets");

    assert_eq!(row_counts, [2, 1]);
    assert_eq!(
        MiniExcel::get_sheet_names(&path).expect("read sheet names"),
        ["Users", "Departments"]
    );
    MiniExcel::save_as(&path, &users).expect_err("reject existing output by default");
    assert_eq!(
        MiniExcel::get_sheet_names(&path).expect("original workbook remains readable"),
        ["Users", "Departments"]
    );

    MiniExcel::save_as_with_options(
        &path,
        &departments,
        &WriteOptions::new().with_sheet_name("Replacement").with_overwrite_file(true),
    )
    .expect("overwrite output explicitly");
    assert_eq!(MiniExcel::get_sheet_names(&path).unwrap(), ["Replacement"]);
}

#[test]
fn writes_per_sheet_visibility_and_activates_the_first_visible_sheet() {
    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let path = temp_dir.path().join("visibility.xlsx");
    let archived = [dynamic_row("Archived", 1)];
    let current = [dynamic_row("Current", 2)];
    let system = [dynamic_row("System", 3)];
    let options = WriteOptions::new()
        .with_sheet_visibility("ARCHIVED", SheetVisibility::Hidden)
        .with_sheet_visibility("System", SheetVisibility::VeryHidden);

    MiniExcel::save_as_sheets(
        &path,
        [
            ("Archived", archived.as_slice()),
            ("Current", current.as_slice()),
            ("System", system.as_slice()),
        ],
        &options,
    )
    .expect("write sheet visibility");

    let info = MiniExcel::get_sheet_info(&path).expect("read sheet visibility");
    assert_eq!(info.len(), 3);
    assert_eq!(info[0].visibility(), SheetVisibility::Hidden);
    assert!(!info[0].is_active());
    assert_eq!(info[1].visibility(), SheetVisibility::Visible);
    assert!(info[1].is_active());
    assert_eq!(info[2].visibility(), SheetVisibility::VeryHidden);
    assert!(!info[2].is_active());

    let workbook_xml = archive_xml(&std::fs::read(&path).unwrap(), "xl/workbook.xml");
    assert!(workbook_xml.contains("name=\"Archived\" sheetId=\"1\" state=\"hidden\""));
    assert!(workbook_xml.contains("name=\"System\" sheetId=\"3\" state=\"veryHidden\""));
    assert!(workbook_xml.contains("activeTab=\"1\""));

    let hidden_row = MiniExcel::query_with_options(
        &path,
        &ReadOptions::new().with_sheet_name("Archived").with_header_mode(HeaderMode::FirstRow),
    )
    .unwrap()
    .next()
    .unwrap()
    .unwrap();
    assert_eq!(hidden_row["Name"], CellValue::String("Archived".to_owned()));
}

#[test]
fn rejects_invalid_sheet_visibility_configurations_before_creating_output() {
    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let path = temp_dir.path().join("all-hidden.xlsx");
    let rows = [dynamic_row("Hidden", 1)];
    let all_hidden = WriteOptions::new()
        .with_sheet_visibility("One", SheetVisibility::Hidden)
        .with_sheet_visibility("Two", SheetVisibility::VeryHidden);
    assert!(
        MiniExcel::save_as_sheets(
            &path,
            [("One", rows.as_slice()), ("Two", rows.as_slice())],
            &all_hidden,
        )
        .is_err()
    );
    assert!(!path.exists());

    let typo_path = temp_dir.path().join("typo.xlsx");
    let typo = WriteOptions::new().with_sheet_visibility("Missing", SheetVisibility::Hidden);
    assert!(MiniExcel::save_as_sheets(&typo_path, [("Actual", rows.as_slice())], &typo).is_err());
    assert!(!typo_path.exists());
}

#[test]
fn writes_dynamic_rows_and_reads_them_back() {
    let date = NaiveDate::from_ymd_opt(2025, 8, 13).unwrap();
    let time = NaiveTime::from_hms_opt(14, 30, 15).unwrap();
    let datetime = NaiveDateTime::new(date, time);

    let mut first = DynamicRow::new();
    first.insert("Name".to_owned(), CellValue::String("MiniExcel".to_owned()));
    first.insert("Count".to_owned(), CellValue::Int(2));
    first.insert("Ratio".to_owned(), CellValue::Float(1.25));
    first.insert("Active".to_owned(), CellValue::Bool(true));
    first.insert("Date".to_owned(), CellValue::Date(date));
    first.insert("Time".to_owned(), CellValue::Time(time));
    first.insert("Created".to_owned(), CellValue::DateTime(datetime));
    first.insert("Elapsed".to_owned(), CellValue::Duration(Duration::hours(27)));
    first.insert("Missing".to_owned(), CellValue::Empty);

    let mut second = DynamicRow::new();
    second.insert("Name".to_owned(), CellValue::String("Rust".to_owned()));
    second.insert("Later".to_owned(), CellValue::String("union column".to_owned()));

    let temp_file = tempfile::NamedTempFile::new().expect("create temporary XLSX path");
    let options = ReadOptions::new().with_sheet_name("Data").with_header_mode(HeaderMode::FirstRow);
    MiniExcel::save_as_with_options(
        temp_file.path(),
        &[first, second],
        &WriteOptions::new().with_sheet_name("Data").with_overwrite_file(true),
    )
    .expect("write workbook");
    assert_eq!(MiniExcel::get_sheet_names(temp_file.path()).expect("read sheet names"), ["Data"]);
    let rows = MiniExcel::query_with_options(temp_file.path(), &options)
        .expect("create generated query")
        .collect::<miniexcel::Result<Vec<_>>>()
        .expect("read generated rows");

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["Name"], CellValue::String("MiniExcel".to_owned()));
    assert_eq!(rows[0]["Count"], CellValue::Int(2));
    assert_eq!(rows[0]["Ratio"], CellValue::Float(1.25));
    assert_eq!(rows[0]["Active"], CellValue::Bool(true));
    assert_eq!(rows[0]["Elapsed"], CellValue::Duration(Duration::hours(27)));
    assert!(rows[0]["Missing"].is_empty());
    assert!(rows[1]["Count"].is_empty());
    assert_eq!(rows[1]["Later"], CellValue::String("union column".to_owned()));
    assert_eq!(rows[0].keys().last().map(String::as_str), Some("Later"));

    assert_eq!(rows[0]["Date"], CellValue::DateTime(date.and_hms_opt(0, 0, 0).unwrap()));
    assert_eq!(rows[0]["Created"], CellValue::DateTime(datetime));
}

#[test]
fn writes_without_headers() {
    let mut row = DynamicRow::new();
    row.insert("Name".to_owned(), CellValue::String("MiniExcel".to_owned()));
    row.insert("Count".to_owned(), CellValue::Int(1));

    let temp_file = tempfile::NamedTempFile::new().expect("create temporary XLSX path");
    MiniExcel::save_as_with_options(
        temp_file.path(),
        &[row],
        &WriteOptions::new().with_print_header(false).with_overwrite_file(true),
    )
    .expect("write workbook");
    let rows = MiniExcel::query(temp_file.path())
        .expect("create generated query")
        .collect::<miniexcel::Result<Vec<_>>>()
        .expect("read generated rows");
    assert_eq!(rows[0]["A"], CellValue::String("MiniExcel".to_owned()));
    assert_eq!(rows[0]["B"], CellValue::Int(1));
}

#[test]
fn rejects_invalid_sheet_names() {
    let mut row = DynamicRow::new();
    row.insert("Value".to_owned(), CellValue::Int(1));

    let temp_file = tempfile::NamedTempFile::new().expect("create temporary XLSX path");
    assert!(
        MiniExcel::save_as_with_options(
            temp_file.path(),
            &[row],
            &WriteOptions::new().with_sheet_name("invalid/name"),
        )
        .is_err()
    );
}

#[test]
fn writes_serde_structs_with_dates() {
    let releases = [
        Release {
            name: "MiniExcel".to_owned(),
            version: 2,
            released_on: NaiveDate::from_ymd_opt(2025, 1, 2).unwrap(),
            internal: true,
        },
        Release {
            name: "MiniExcel Rust".to_owned(),
            version: 1,
            released_on: NaiveDate::from_ymd_opt(2026, 8, 13).unwrap(),
            internal: false,
        },
    ];

    let options = WriteOptions::new()
        .with_sheet_name("Releases")
        .with_overwrite_file(true)
        .with_column_format("ReleasedOn", "yyyy-mm-dd");
    let temp_file = tempfile::NamedTempFile::new().expect("create temporary XLSX path");
    MiniExcel::save_as_serialized_with_options(temp_file.path(), &releases, &options)
        .expect("serialize rows");
    let rows = MiniExcel::query_as_with_options::<Release>(
        temp_file.path(),
        &ReadOptions::new().with_sheet_name("Releases"),
    )
    .expect("create typed query")
    .collect::<miniexcel::Result<Vec<_>>>()
    .expect("deserialize generated rows");

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].name, "MiniExcel");
    assert_eq!(rows[0].version, 2);
    assert_eq!(rows[0].released_on, NaiveDate::from_ymd_opt(2025, 1, 2).unwrap());
    assert!(!rows[0].internal);
    assert_eq!(rows[1].released_on, NaiveDate::from_ymd_opt(2026, 8, 13).unwrap());
}

#[test]
fn writes_explicit_empty_schema_and_overwrites_paths_when_enabled() {
    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let path = temp_dir.path().join("output.xlsx");
    let schema = vec!["Value".to_owned()];

    MiniExcel::save_as_with_schema(&path, &schema, &[], &WriteOptions::default())
        .expect("save header-only workbook");
    let options = ReadOptions::new().with_header_mode(HeaderMode::FirstRow);
    let first_rows = MiniExcel::query_with_options(&path, &options)
        .expect("create header-only query")
        .collect::<miniexcel::Result<Vec<_>>>()
        .expect("read header-only sheet");
    assert!(first_rows.is_empty());

    let mut row = DynamicRow::new();
    row.insert("Value".to_owned(), CellValue::String("replacement".to_owned()));
    MiniExcel::save_as_with_options(&path, &[row], &WriteOptions::new().with_overwrite_file(true))
        .expect("overwrite workbook");
    let rows = MiniExcel::query_with_options(path, &options)
        .expect("create replacement query")
        .collect::<miniexcel::Result<Vec<_>>>()
        .expect("read replacement row");
    assert_eq!(rows[0]["Value"], CellValue::String("replacement".to_owned()));
}

#[test]
fn writes_multiple_serialized_sheets() {
    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let path = temp_dir.path().join("serialized-sheets.xlsx");
    let stable = [Release {
        name: "MiniExcel".to_owned(),
        version: 2,
        released_on: NaiveDate::from_ymd_opt(2025, 1, 2).unwrap(),
        internal: false,
    }];
    let preview = [Release {
        name: "MiniExcel Rust".to_owned(),
        version: 1,
        released_on: NaiveDate::from_ymd_opt(2026, 8, 13).unwrap(),
        internal: true,
    }];
    let options = WriteOptions::new()
        .with_column_format("ReleasedOn", "yyyy-mm-dd")
        .with_sheet_visibility("Preview", SheetVisibility::VeryHidden);

    let row_counts = MiniExcel::save_as_serialized_sheets(
        &path,
        [("Stable", stable.as_slice()), ("Preview", preview.as_slice())],
        &options,
    )
    .expect("write serialized worksheets");

    assert_eq!(row_counts, [1, 1]);
    assert_eq!(MiniExcel::get_sheet_names(&path).unwrap(), ["Stable", "Preview"]);
    assert_eq!(
        MiniExcel::get_sheet_info(&path).unwrap()[1].visibility(),
        SheetVisibility::VeryHidden
    );
    let rows = MiniExcel::query_as_with_options::<Release>(
        &path,
        &ReadOptions::new().with_sheet_name("Preview"),
    )
    .expect("query serialized worksheet")
    .collect::<miniexcel::Result<Vec<_>>>()
    .expect("read serialized worksheet");
    assert_eq!(rows[0].name, "MiniExcel Rust");
    assert_eq!(rows[0].released_on, NaiveDate::from_ymd_opt(2026, 8, 13).unwrap());
}

#[test]
fn requires_schema_for_empty_default_exports() {
    let temp_file = tempfile::NamedTempFile::new().expect("create temporary XLSX path");
    assert!(MiniExcel::save_as(temp_file.path(), &[]).is_err());
    assert!(MiniExcel::save_as_serialized::<Release>(temp_file.path(), &[]).is_err());
}

#[test]
fn writes_and_reads_an_in_memory_workbook() {
    let mut row = DynamicRow::new();
    row.insert("Name".to_owned(), CellValue::String("Browser".to_owned()));
    row.insert("Count".to_owned(), CellValue::Int(1));
    let write_options = WriteOptions::new().with_sheet_name("Memory");

    let bytes = MiniExcel::save_as_bytes(&[row], &write_options).expect("write workbook bytes");
    let read_options =
        ReadOptions::new().with_sheet_name("Memory").with_header_mode(HeaderMode::FirstRow);
    let rows = MiniExcel::query_bytes(&bytes, &read_options).expect("read workbook bytes");

    assert_eq!(MiniExcel::get_sheet_names_from_bytes(&bytes).unwrap(), ["Memory"]);
    assert_eq!(rows[0]["Name"], CellValue::String("Browser".to_owned()));
    assert_eq!(rows[0]["Count"], CellValue::Int(1));
}
