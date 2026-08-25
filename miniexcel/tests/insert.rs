use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::rc::Rc;

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
#[cfg(feature = "async")]
use futures_util::StreamExt;
use miniexcel::{
    CellValue, DynamicRow, ExistingSheetPolicy, HeaderMode, HeaderStyle, HorizontalAlignment,
    InsertOptions, MiniExcel, ReadOptions, RgbColor, SheetVisibility, TableStyle,
    TargetRelationshipPolicy, VerticalAlignment, WriteOptions,
};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use serde::Serialize;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

const ENTRY_NAMES: &[&str] = &[
    "[Content_Types].xml",
    "_rels/.rels",
    "customXml/_rels/item1.xml.rels",
    "customXml/item1.xml",
    "customXml/itemProps1.xml",
    "docProps/app.xml",
    "docProps/core.xml",
    "xl/_rels/workbook.xml.rels",
    "xl/comments1.xml",
    "xl/drawings/_rels/drawing1.xml.rels",
    "xl/drawings/drawing1.xml",
    "xl/drawings/vmlDrawing1.vml",
    "xl/media/image1.png",
    "xl/sharedStrings.xml",
    "xl/styles.xml",
    "xl/tables/table1.xml",
    "xl/workbook.xml",
    "xl/worksheets/_rels/data.xml.rels",
    "xl/worksheets/archive.xml",
    "xl/worksheets/data.xml",
    "xl/worksheets/sheet7.xml",
];

const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 4, 0,
    0, 0, 181, 28, 12, 2, 0, 0, 0, 11, 73, 68, 65, 84, 120, 218, 99, 100, 248, 15, 0, 1, 5, 1, 1,
    39, 24, 227, 102, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ContractOutcome {
    Success,
    RejectUnchanged,
}

#[derive(Clone, Copy, Debug)]
struct InsertContractCase {
    name: &'static str,
    target_sheet: &'static str,
    print_header: bool,
    input_rows: usize,
    expected_count: Option<usize>,
    expected_order: &'static [&'static str],
    replace: bool,
    preserve_identity: bool,
    outcome: ContractOutcome,
}

const INSERT_CONTRACT_CASES: &[InsertContractCase] = &[
    InsertContractCase {
        name: "missing_path_create",
        target_sheet: "Sheet1",
        print_header: true,
        input_rows: 2,
        expected_count: Some(2),
        expected_order: &["Sheet1"],
        replace: false,
        preserve_identity: false,
        outcome: ContractOutcome::Success,
    },
    InsertContractCase {
        name: "append",
        target_sheet: "Sheet2",
        print_header: true,
        input_rows: 2,
        expected_count: Some(2),
        expected_order: &["Sheet1", "Sheet2"],
        replace: false,
        preserve_identity: false,
        outcome: ContractOutcome::Success,
    },
    InsertContractCase {
        name: "duplicate_exact",
        target_sheet: "Data",
        print_header: true,
        input_rows: 1,
        expected_count: None,
        expected_order: &["Data", "HiddenCalc", "Archive"],
        replace: false,
        preserve_identity: true,
        outcome: ContractOutcome::RejectUnchanged,
    },
    InsertContractCase {
        name: "duplicate_case_variant",
        target_sheet: "data",
        print_header: true,
        input_rows: 1,
        expected_count: None,
        expected_order: &["Data", "HiddenCalc", "Archive"],
        replace: false,
        preserve_identity: true,
        outcome: ContractOutcome::RejectUnchanged,
    },
    InsertContractCase {
        name: "replace_active",
        target_sheet: "Data",
        print_header: false,
        input_rows: 1,
        expected_count: Some(1),
        expected_order: &["Data", "HiddenCalc", "Archive"],
        replace: true,
        preserve_identity: true,
        outcome: ContractOutcome::Success,
    },
    InsertContractCase {
        name: "replace_hidden",
        target_sheet: "HiddenCalc",
        print_header: true,
        input_rows: 1,
        expected_count: Some(1),
        expected_order: &["Data", "HiddenCalc", "Archive"],
        replace: true,
        preserve_identity: true,
        outcome: ContractOutcome::Success,
    },
    InsertContractCase {
        name: "no_header",
        target_sheet: "Sheet3",
        print_header: false,
        input_rows: 1,
        expected_count: Some(1),
        expected_order: &["Sheet1", "Sheet2", "Sheet3"],
        replace: false,
        preserve_identity: false,
        outcome: ContractOutcome::Success,
    },
    InsertContractCase {
        name: "long_name",
        target_sheet: "12345678901234567890123456789012",
        print_header: true,
        input_rows: 1,
        expected_count: None,
        expected_order: &[],
        replace: false,
        preserve_identity: false,
        outcome: ContractOutcome::RejectUnchanged,
    },
];

#[derive(Debug, Eq, PartialEq)]
struct EntryInventory {
    crc32: u32,
    uncompressed_size: u64,
}

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RelationshipIdentity {
    source: String,
    id: String,
    relationship_type: String,
    target: String,
    target_mode: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
struct SheetIdentity {
    index: usize,
    name: String,
    sheet_id: u32,
    relationship_id: String,
    target: String,
    state: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
struct DefinedName {
    name: String,
    local_sheet_id: Option<usize>,
    hidden: bool,
    formula: String,
}

#[derive(Debug, Default, Eq, PartialEq)]
struct StyleCounts {
    num_fmts: usize,
    fonts: usize,
    fills: usize,
    borders: usize,
    cell_style_xfs: usize,
    cell_xfs: usize,
    cell_styles: usize,
    dxfs: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct CellXf {
    num_fmt_id: usize,
    font_id: usize,
    fill_id: usize,
    border_id: usize,
    horizontal: Option<String>,
    vertical: Option<String>,
    wrap_text: bool,
}

#[derive(Debug, Eq, PartialEq)]
struct PackageInventory {
    entries: BTreeMap<String, EntryInventory>,
    relationships: Vec<RelationshipIdentity>,
    sheets: Vec<SheetIdentity>,
    active_tab: usize,
    defined_names: Vec<DefinedName>,
    styles: StyleCounts,
}

struct FailOnceWriter {
    inner: Cursor<Vec<u8>>,
    fail_next_write: bool,
}

struct FailOnceReader {
    inner: Cursor<Vec<u8>>,
    fail_next_read: bool,
}

impl FailOnceReader {
    fn new(bytes: Vec<u8>) -> Self {
        Self { inner: Cursor::new(bytes), fail_next_read: true }
    }
}

impl Read for FailOnceReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.fail_next_read {
            self.fail_next_read = false;
            return Err(std::io::Error::other("source failed"));
        }
        self.inner.read(buffer)
    }
}

impl Seek for FailOnceReader {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(position)
    }
}

impl FailOnceWriter {
    fn new() -> Self {
        Self { inner: Cursor::new(Vec::new()), fail_next_write: true }
    }
}

impl Write for FailOnceWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        if self.fail_next_write {
            self.fail_next_write = false;
            return Err(std::io::Error::other("destination failed"));
        }
        self.inner.write(buffer)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl Seek for FailOnceWriter {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(position)
    }
}

#[test]
fn characterization_complex_fixture_is_deterministic() {
    let first = complex_fixture();
    let second = complex_fixture();
    assert_eq!(first, second);
    assert_eq!(package_inventory(&first).entries.len(), ENTRY_NAMES.len());
}

#[test]
fn characterization_complex_fixture_inventory_matches_contract() {
    let bytes = complex_fixture();
    let inventory = package_inventory(&bytes);
    assert_eq!(inventory.entries.keys().map(String::as_str).collect::<Vec<_>>(), ENTRY_NAMES);
    for (name, entry) in &inventory.entries {
        let payload = read_entry(&bytes, name);
        assert_eq!(entry.crc32, crc32(&payload), "CRC mismatch for {name}");
        assert_eq!(entry.uncompressed_size, payload.len() as u64);
    }
    assert_eq!(
        inventory.sheets,
        [
            SheetIdentity {
                index: 0,
                name: "Data".to_owned(),
                sheet_id: 7,
                relationship_id: "rId9".to_owned(),
                target: "worksheets/data.xml".to_owned(),
                state: None,
            },
            SheetIdentity {
                index: 1,
                name: "HiddenCalc".to_owned(),
                sheet_id: 42,
                relationship_id: "rId2".to_owned(),
                target: "worksheets/sheet7.xml".to_owned(),
                state: Some("hidden".to_owned()),
            },
            SheetIdentity {
                index: 2,
                name: "Archive".to_owned(),
                sheet_id: 3,
                relationship_id: "rId14".to_owned(),
                target: "worksheets/archive.xml".to_owned(),
                state: Some("veryHidden".to_owned()),
            },
        ]
    );
    assert_eq!(inventory.active_tab, 0);
    assert_eq!(
        inventory.defined_names,
        [
            DefinedName {
                name: "GlobalTotal".to_owned(),
                local_sheet_id: None,
                hidden: false,
                formula: "'Data'!$C$2".to_owned(),
            },
            DefinedName {
                name: "_xlnm.Print_Area".to_owned(),
                local_sheet_id: Some(0),
                hidden: false,
                formula: "'Data'!$A$1:$C$2".to_owned(),
            },
            DefinedName {
                name: "HiddenFormula".to_owned(),
                local_sheet_id: Some(1),
                hidden: true,
                formula: "'HiddenCalc'!$A$2".to_owned(),
            },
        ]
    );
    assert_eq!(
        inventory.styles,
        StyleCounts {
            num_fmts: 1,
            fonts: 2,
            fills: 3,
            borders: 2,
            cell_style_xfs: 1,
            cell_xfs: 3,
            cell_styles: 1,
            dxfs: 0,
        }
    );
}

#[test]
fn characterization_reader_preserves_sheet_document_order_and_metadata() {
    let bytes = complex_fixture();
    let info = MiniExcel::get_sheet_info_from_bytes(&bytes).expect("read fixture metadata");
    assert_eq!(
        info.iter().map(|sheet| sheet.name()).collect::<Vec<_>>(),
        ["Data", "HiddenCalc", "Archive",]
    );
    assert_eq!(info.iter().map(|sheet| sheet.id()).collect::<Vec<_>>(), [7, 42, 3]);
    assert_eq!(
        info.iter().map(|sheet| sheet.visibility()).collect::<Vec<_>>(),
        [SheetVisibility::Visible, SheetVisibility::Hidden, SheetVisibility::VeryHidden]
    );
    assert_eq!(
        info.iter().map(|sheet| sheet.is_active()).collect::<Vec<_>>(),
        [true, false, false,]
    );

    let rows = MiniExcel::query_structured_with_options(
        write_fixture(&bytes).path(),
        &ReadOptions::new().with_sheet_name("Data"),
    )
    .expect("query structured fixture")
    .collect::<miniexcel::Result<Vec<_>>>()
    .expect("read structured fixture");
    assert_eq!(rows[1].cells()[2].formula(), Some("B2*2"));
    assert_eq!(rows[1].cells()[2].value(), &CellValue::Int(83));
}

#[test]
fn characterization_fixture_covers_preservation_parts() {
    let bytes = complex_fixture();
    let relationships = package_inventory(&bytes).relationships;
    for expected in [
        ("xl/workbook.xml", "rId21", "../customXml/item1.xml"),
        ("xl/worksheets/data.xml", "rId4", "../tables/table1.xml"),
        ("xl/worksheets/data.xml", "rId11", "../drawings/drawing1.xml"),
        ("xl/worksheets/data.xml", "rId3", "../comments1.xml"),
        ("xl/drawings/drawing1.xml", "rId8", "../media/image1.png"),
    ] {
        assert!(relationships.iter().any(|relationship| {
            relationship.source == expected.0
                && relationship.id == expected.1
                && relationship.target == expected.2
        }));
    }
    assert_eq!(read_entry(&bytes, "xl/media/image1.png"), PNG);
    assert!(entry_text(&bytes, "xl/tables/table1.xml").contains("displayName=\"DataTable\""));
    assert!(entry_text(&bytes, "xl/comments1.xml").contains("preserve me"));
    assert!(entry_text(&bytes, "customXml/item1.xml").contains("urn:miniexcel:insert:test"));
    assert!(entry_text(&bytes, "xl/worksheets/data.xml").contains("<f>B2*2</f>"));
    assert!(entry_text(&bytes, "xl/sharedStrings.xml").contains("Hidden label"));
}

#[test]
fn characterization_v1_contract_cases_are_complete() {
    let names = INSERT_CONTRACT_CASES.iter().map(|case| case.name).collect::<BTreeSet<_>>();
    assert_eq!(names.len(), INSERT_CONTRACT_CASES.len());
    assert_eq!(
        names,
        BTreeSet::from([
            "append",
            "duplicate_case_variant",
            "duplicate_exact",
            "long_name",
            "missing_path_create",
            "no_header",
            "replace_active",
            "replace_hidden",
        ])
    );
    for case in INSERT_CONTRACT_CASES {
        match case.outcome {
            ContractOutcome::Success => assert_eq!(case.expected_count, Some(case.input_rows)),
            ContractOutcome::RejectUnchanged => assert!(case.expected_count.is_none()),
        }
        if case.replace {
            assert!(case.preserve_identity);
        }
        if case.name == "no_header" {
            assert!(!case.print_header);
        }
        if case.name == "append" {
            assert_eq!(case.target_sheet, "Sheet2");
            assert_eq!(case.expected_order, ["Sheet1", "Sheet2"]);
        }
    }
}

#[test]
fn characterization_sheet_name_limit_matches_writer_contract() {
    let mut row = DynamicRow::new();
    row.insert("Value".to_owned(), CellValue::Int(1));
    assert!(
        MiniExcel::save_as_bytes(
            &[row.clone()],
            &WriteOptions::new().with_sheet_name("1234567890123456789012345678901"),
        )
        .is_ok()
    );
    assert!(
        MiniExcel::save_as_bytes(
            &[row],
            &WriteOptions::new().with_sheet_name("12345678901234567890123456789012"),
        )
        .is_err()
    );
}

#[test]
fn public_append_creates_missing_path_and_appends_dynamic_rows_atomically() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("dynamic.xlsx");
    let rows = [dynamic_insert_row("MiniExcel", 2), dynamic_insert_row("Rust", 1)];

    let created =
        MiniExcel::insert(&path, &rows, &InsertOptions::new().with_sheet_name("Current")).unwrap();
    assert_eq!(created, 2);
    assert_eq!(MiniExcel::get_sheet_names(&path).unwrap(), ["Current"]);

    let appended = MiniExcel::insert(
        &path,
        &[dynamic_insert_row("Archive", 3)],
        &InsertOptions::new().with_write_options(
            WriteOptions::new()
                .with_sheet_name("Archive")
                .with_auto_filter(false)
                .with_freeze_row_count(0),
        ),
    )
    .unwrap();
    assert_eq!(appended, 1);
    assert_eq!(MiniExcel::get_sheet_names(&path).unwrap(), ["Current", "Archive"]);
    let rows = MiniExcel::query_with_options(
        &path,
        &ReadOptions::new().with_sheet_name("Archive").with_header_mode(HeaderMode::FirstRow),
    )
    .unwrap()
    .collect::<miniexcel::Result<Vec<_>>>()
    .unwrap();
    assert_eq!(rows[0]["Name"], CellValue::String("Archive".to_owned()));
    assert_eq!(rows[0]["Version"], CellValue::Int(3));
}

#[test]
fn write_options_matrix_appends_hidden_sheet() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("hidden.xlsx");
    let missing_hidden = directory.path().join("missing-hidden.xlsx");
    assert!(
        MiniExcel::insert(
            &missing_hidden,
            &[dynamic_insert_row("Hidden", 0)],
            &InsertOptions::new().with_write_options(
                WriteOptions::new()
                    .with_sheet_name("Hidden")
                    .with_sheet_visibility("Hidden", SheetVisibility::Hidden),
            ),
        )
        .is_err()
    );
    assert!(!missing_hidden.exists());

    MiniExcel::insert(
        &path,
        &[dynamic_insert_row("Current", 1)],
        &InsertOptions::new().with_sheet_name("Current"),
    )
    .unwrap();

    MiniExcel::insert(
        &path,
        &[dynamic_insert_row("Archive", 2)],
        &InsertOptions::new().with_write_options(
            WriteOptions::new()
                .with_sheet_name("Archive")
                .with_sheet_visibility("archive", SheetVisibility::Hidden),
        ),
    )
    .unwrap();

    let bytes = std::fs::read(&path).unwrap();
    let inventory = package_inventory(&bytes);
    assert_eq!(inventory.sheets[0].state, None);
    assert_eq!(inventory.sheets[1].state.as_deref(), Some("hidden"));
}

#[test]
fn write_options_matrix_preserves_layout_styles_and_formats() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("matrix.xlsx");
    MiniExcel::insert(
        &path,
        &[dynamic_insert_row("Current", 1)],
        &InsertOptions::new().with_sheet_name("Current"),
    )
    .unwrap();

    let mut row = DynamicRow::new();
    row.insert("Name".to_owned(), CellValue::String("Matrix".to_owned()));
    row.insert("Version".to_owned(), CellValue::Int(3));
    row.insert(
        "ReleasedOn".to_owned(),
        CellValue::Date(NaiveDate::from_ymd_opt(2026, 8, 25).unwrap()),
    );
    row.insert("At".to_owned(), CellValue::Time(NaiveTime::from_hms_opt(10, 30, 0).unwrap()));
    row.insert(
        "RecordedAt".to_owned(),
        CellValue::DateTime(
            NaiveDateTime::parse_from_str("2026-08-25 10:30:00", "%Y-%m-%d %H:%M:%S").unwrap(),
        ),
    );
    row.insert("Elapsed".to_owned(), CellValue::Duration(Duration::minutes(90)));
    let header_style = HeaderStyle::new()
        .with_wrap_text(true)
        .with_background_color(RgbColor::new(0x12, 0x34, 0x56))
        .with_horizontal_alignment(HorizontalAlignment::Center)
        .with_vertical_alignment(VerticalAlignment::Top);
    let options = WriteOptions::new()
        .with_sheet_name("Matrix")
        .with_auto_filter(true)
        .with_freeze_row_count(2)
        .with_freeze_column_count(1)
        .with_right_to_left(true)
        .with_auto_width(true)
        .with_min_width(12.0)
        .with_max_width(24.0)
        .with_column_width("Name", 18.0)
        .with_column_hidden("Version", true)
        .with_wrap_cell_contents(true)
        .with_horizontal_alignment(HorizontalAlignment::Center)
        .with_vertical_alignment(VerticalAlignment::Top)
        .with_header_style(header_style)
        .with_table_style(TableStyle::Default)
        .with_date_format("dd.mm.yyyy")
        .with_time_format("hh:mm")
        .with_datetime_format("dd.mm.yyyy hh:mm")
        .with_duration_format("[h]:mm")
        .with_sheet_visibility("matrix", SheetVisibility::VeryHidden);
    MiniExcel::insert(&path, &[row], &InsertOptions::new().with_write_options(options)).unwrap();

    let bytes = std::fs::read(&path).unwrap();
    let inventory = package_inventory(&bytes);
    let matrix = inventory.sheets.iter().find(|sheet| sheet.name == "Matrix").unwrap();
    assert_eq!(matrix.state.as_deref(), Some("veryHidden"));
    let worksheet = entry_text(&bytes, &format!("xl/{}", matrix.target));
    assert!(worksheet.contains("rightToLeft=\"1\""));
    assert!(worksheet.contains("xSplit=\"1\""));
    assert!(worksheet.contains("ySplit=\"2\""));
    assert!(worksheet.contains("topLeftCell=\"B3\""));
    assert!(worksheet.contains("<autoFilter ref=\"A1:F2\""));
    assert!(worksheet.contains("min=\"1\" max=\"1\" width=\"18.7109375\""));
    assert!(worksheet.contains("min=\"2\" max=\"2\""));
    assert!(worksheet.contains("hidden=\"1\""));

    let styles = read_entry(&bytes, "xl/styles.xml");
    assert!(String::from_utf8_lossy(&styles).contains("rgb=\"FF123456\""));
    let header = cell_xf(&styles, cell_style_index(&worksheet, "A1"));
    assert!(header.wrap_text);
    assert_eq!(header.horizontal.as_deref(), Some("center"));
    assert_eq!(header.vertical.as_deref(), Some("top"));
    assert_ne!(header.fill_id, 0);
    assert_ne!(header.border_id, 0);
    let body = cell_xf(&styles, cell_style_index(&worksheet, "A2"));
    assert!(body.wrap_text);
    assert_eq!(body.horizontal.as_deref(), Some("center"));
    assert_eq!(body.vertical.as_deref(), Some("top"));
    assert_ne!(body.border_id, 0);
    for address in ["C2", "D2", "E2", "F2"] {
        assert_ne!(cell_xf(&styles, cell_style_index(&worksheet, address)).num_fmt_id, 0);
    }
    let styles_text = String::from_utf8_lossy(&styles);
    for format in ["dd.mm.yyyy", "hh:mm", "dd.mm.yyyy hh:mm", "[h]:mm"] {
        assert!(styles_text.contains(&format!("formatCode=\"{format}\"")));
    }

    MiniExcel::insert(
        &path,
        &[{
            let mut row = DynamicRow::new();
            row.insert("Name".to_owned(), CellValue::String("Minimal".to_owned()));
            row.insert(
                "ReleasedOn".to_owned(),
                CellValue::Date(NaiveDate::from_ymd_opt(2026, 8, 25).unwrap()),
            );
            row
        }],
        &InsertOptions::new().with_write_options(
            WriteOptions::new()
                .with_sheet_name("Minimal")
                .with_table_style(TableStyle::None)
                .with_date_format("0.000"),
        ),
    )
    .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let inventory = package_inventory(&bytes);
    let minimal = inventory.sheets.iter().find(|sheet| sheet.name == "Minimal").unwrap();
    let worksheet = entry_text(&bytes, &format!("xl/{}", minimal.target));
    let styles = read_entry(&bytes, "xl/styles.xml");
    for address in ["A1", "A2"] {
        let style = cell_xf(&styles, cell_style_index(&worksheet, address));
        assert_eq!((style.font_id, style.fill_id, style.border_id), (0, 0, 0));
        assert!(!style.wrap_text);
        assert_eq!((style.horizontal, style.vertical), (None, None));
    }
    let formatted = cell_xf(&styles, cell_style_index(&worksheet, "B2"));
    assert_ne!(formatted.num_fmt_id, 0);
    assert_eq!(formatted.border_id, 0);
    assert!(String::from_utf8_lossy(&styles).contains("formatCode=\"0.000\""));
}

#[test]
fn write_options_matrix_supports_all_insert_input_shapes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("shapes.xlsx");
    MiniExcel::insert(
        &path,
        &[dynamic_insert_row("Current", 1)],
        &InsertOptions::new().with_sheet_name("Current"),
    )
    .unwrap();

    assert_eq!(
        MiniExcel::insert(
            &path,
            &[dynamic_insert_row("Headerless", 2)],
            &InsertOptions::new().with_write_options(
                WriteOptions::new().with_sheet_name("Headerless").with_print_header(false),
            ),
        )
        .unwrap(),
        1
    );
    assert_eq!(
        MiniExcel::insert_with_schema(
            &path,
            &["Name".to_owned(), "Version".to_owned()],
            std::iter::empty::<miniexcel::Result<DynamicRow>>(),
            &InsertOptions::new().with_sheet_name("Header Only"),
        )
        .unwrap(),
        0
    );
    assert_eq!(
        MiniExcel::insert(
            &path,
            &[],
            &InsertOptions::new().with_write_options(
                WriteOptions::new().with_sheet_name("Empty").with_print_header(false),
            ),
        )
        .unwrap(),
        0
    );
    assert_eq!(
        MiniExcel::insert_serialized(
            &path,
            &[PublicInsertRelease { name: "Serde".to_owned(), version: 3 }],
            &InsertOptions::new().with_write_options(
                WriteOptions::new().with_sheet_name("Serde").with_column_format("Version", "0.00"),
            ),
        )
        .unwrap(),
        1
    );

    assert_eq!(
        MiniExcel::get_sheet_names(&path).unwrap(),
        ["Current", "Headerless", "Header Only", "Empty", "Serde"]
    );
    let bytes = std::fs::read(&path).unwrap();
    let inventory = package_inventory(&bytes);
    let worksheet = |name: &str| {
        let sheet = inventory.sheets.iter().find(|sheet| sheet.name == name).unwrap();
        entry_text(&bytes, &format!("xl/{}", sheet.target))
    };
    assert!(worksheet("Headerless").contains("<autoFilter ref=\"A1:B1\""));
    assert!(worksheet("Header Only").contains("<autoFilter ref=\"A1:B1\""));
    assert!(!worksheet("Empty").contains("<autoFilter"));
    let serde_worksheet = worksheet("Serde");
    assert!(serde_worksheet.contains("<autoFilter ref=\"A1:B2\""));
    let styles = read_entry(&bytes, "xl/styles.xml");
    assert_ne!(cell_xf(&styles, cell_style_index(&serde_worksheet, "B2")).num_fmt_id, 0);
    assert!(String::from_utf8_lossy(&styles).contains("formatCode=\"0.00\""));

    let before = bytes;
    assert!(
        MiniExcel::insert(
            &path,
            &[dynamic_insert_row("Duplicate", 4)],
            &InsertOptions::new().with_sheet_name("sErDe"),
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn write_options_matrix_repeated_inserts_preserve_and_deduplicate_styles() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("styles.xlsx");
    MiniExcel::insert(
        &path,
        &[dynamic_insert_row("Current", 1)],
        &InsertOptions::new().with_sheet_name("Current"),
    )
    .unwrap();
    let initial = std::fs::read(&path).unwrap();
    let initial_inventory = package_inventory(&initial);
    let initial_styles = cell_xf_signatures(&read_entry(&initial, "xl/styles.xml"));
    let initial_sheet = read_entry(&initial, &format!("xl/{}", initial_inventory.sheets[0].target));

    let styled_options = |name: String| {
        InsertOptions::new().with_write_options(
            WriteOptions::new()
                .with_sheet_name(name)
                .with_wrap_cell_contents(true)
                .with_horizontal_alignment(HorizontalAlignment::Right)
                .with_vertical_alignment(VerticalAlignment::Center)
                .with_header_style(
                    HeaderStyle::new()
                        .with_wrap_text(true)
                        .with_background_color(RgbColor::new(0x22, 0x66, 0xAA)),
                )
                .with_column_format("Version", "0.000"),
        )
    };
    MiniExcel::insert(
        &path,
        &[dynamic_insert_row("Styled", 2)],
        &styled_options("Styled0".to_owned()),
    )
    .unwrap();
    let after_first = std::fs::read(&path).unwrap();
    let styles_after_first = read_entry(&after_first, "xl/styles.xml");
    for index in 1..10 {
        MiniExcel::insert(
            &path,
            &[dynamic_insert_row("Styled", index + 2)],
            &styled_options(format!("Styled{index}")),
        )
        .unwrap();
    }

    let final_bytes = std::fs::read(&path).unwrap();
    let final_inventory = package_inventory(&final_bytes);
    let final_styles = read_entry(&final_bytes, "xl/styles.xml");
    assert_eq!(final_styles, styles_after_first);
    assert_eq!(
        &cell_xf_signatures(&final_styles)[..initial_styles.len()],
        initial_styles.as_slice()
    );
    assert_eq!(
        read_entry(&final_bytes, &format!("xl/{}", final_inventory.sheets[0].target)),
        initial_sheet
    );
    assert_eq!(final_inventory.sheets.len(), 11);
}

#[test]
fn write_options_matrix_hundred_insert_stress_has_unique_ids_and_bounded_growth() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("stress.xlsx");
    MiniExcel::insert(
        &path,
        &[dynamic_insert_row("Current", 0)],
        &InsertOptions::new().with_sheet_name("Current"),
    )
    .unwrap();
    let base_size = std::fs::metadata(&path).unwrap().len();
    let mut styles_after_first = None;
    for index in 0..100 {
        MiniExcel::insert(
            &path,
            &[dynamic_insert_row("Stress", index)],
            &InsertOptions::new().with_write_options(
                WriteOptions::new()
                    .with_sheet_name(format!("Stress{index:03}"))
                    .with_column_format("Version", "0.000"),
            ),
        )
        .unwrap();
        if index == 0 {
            styles_after_first = Some(read_entry(&std::fs::read(&path).unwrap(), "xl/styles.xml"));
        }
    }

    let bytes = std::fs::read(&path).unwrap();
    let inventory = package_inventory(&bytes);
    assert_eq!(inventory.sheets.len(), 101);
    assert_eq!(
        inventory.sheets.iter().map(|sheet| sheet.sheet_id).collect::<BTreeSet<_>>().len(),
        101
    );
    assert_eq!(
        inventory
            .sheets
            .iter()
            .map(|sheet| sheet.relationship_id.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        101
    );
    assert_eq!(
        inventory.sheets.iter().map(|sheet| sheet.target.as_str()).collect::<BTreeSet<_>>().len(),
        101
    );
    let mut archive = ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let entry_names = (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().name().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(entry_names.iter().collect::<BTreeSet<_>>().len(), entry_names.len());
    assert_eq!(read_entry(&bytes, "xl/styles.xml"), styles_after_first.unwrap());
    assert!(bytes.len() as u64 <= base_size + 2_000_000);

    let rows = MiniExcel::query_with_options(
        &path,
        &ReadOptions::new().with_sheet_name("Stress099").with_header_mode(HeaderMode::FirstRow),
    )
    .unwrap()
    .collect::<miniexcel::Result<Vec<_>>>()
    .unwrap();
    assert_eq!(rows[0]["Version"], CellValue::Int(99));
    if let Some(output) = std::env::var_os("MINIEXCEL_TEST_INSERT_OUTPUT") {
        std::fs::write(output, bytes).unwrap();
    }
}

#[test]
fn borrowed_io_append_and_replace_match_path_inventory_and_leave_handles_open() {
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("source.xlsx");
    MiniExcel::insert(
        &source_path,
        &[dynamic_insert_row("Current", 1)],
        &InsertOptions::new().with_sheet_name("Current"),
    )
    .unwrap();
    let source_bytes = std::fs::read(&source_path).unwrap();

    let append_options = InsertOptions::new().with_sheet_name("Archive");
    let append_rows = [dynamic_insert_row("Archive", 2)];
    let path_append = directory.path().join("path-append.xlsx");
    std::fs::write(&path_append, &source_bytes).unwrap();
    MiniExcel::insert(&path_append, &append_rows, &append_options).unwrap();

    let mut source = Cursor::new(source_bytes.clone());
    let mut destination = Cursor::new(Vec::new());
    assert_eq!(
        MiniExcel::insert_from_reader_to_writer(
            &mut source,
            &mut destination,
            &append_rows,
            &append_options,
        )
        .unwrap(),
        1
    );
    assert_eq!(source.get_ref(), &source_bytes);
    source.seek(SeekFrom::Start(0)).unwrap();
    destination.seek(SeekFrom::End(0)).unwrap();
    destination.write_all(&[]).unwrap();
    assert_eq!(
        package_inventory(destination.get_ref()),
        package_inventory(&std::fs::read(path_append).unwrap())
    );

    let replacement = [PublicInsertRelease { name: "Replaced".to_owned(), version: 3 }];
    let replace_options = InsertOptions::new()
        .with_sheet_name("Current")
        .with_existing_sheet_policy(ExistingSheetPolicy::Replace);
    let path_replace = directory.path().join("path-replace.xlsx");
    std::fs::write(&path_replace, &source_bytes).unwrap();
    MiniExcel::insert_serialized(&path_replace, &replacement, &replace_options).unwrap();

    let mut source = Cursor::new(source_bytes.clone());
    let mut destination = Cursor::new(Vec::new());
    assert_eq!(
        MiniExcel::insert_serialized_from_reader_to_writer(
            &mut source,
            &mut destination,
            &replacement,
            &replace_options,
        )
        .unwrap(),
        1
    );
    assert_eq!(source.get_ref(), &source_bytes);
    assert_eq!(
        package_inventory(destination.get_ref()),
        package_inventory(&std::fs::read(path_replace).unwrap())
    );
}

#[test]
fn borrowed_io_preflight_and_producer_failures_do_not_overconsume_or_write() {
    let schema = ["Name".to_owned(), "Version".to_owned()];
    let complex = complex_fixture();
    let calls = Rc::new(Cell::new(0));
    let observed = Rc::clone(&calls);
    let rows = std::iter::once_with(move || {
        observed.set(observed.get() + 1);
        Ok(dynamic_insert_row("Never read", 1))
    });
    let mut source = Cursor::new(complex.clone());
    let mut destination = Cursor::new(Vec::new());
    let options = InsertOptions::new()
        .with_sheet_name("Data")
        .with_existing_sheet_policy(ExistingSheetPolicy::Replace);
    assert!(
        MiniExcel::insert_with_schema_from_reader_to_writer(
            &mut source,
            &mut destination,
            &schema,
            rows,
            &options,
        )
        .is_err()
    );
    assert_eq!(calls.get(), 0);
    assert!(destination.get_ref().is_empty());
    assert_eq!(source.get_ref(), &complex);
    source.seek(SeekFrom::Start(0)).unwrap();
    destination.seek(SeekFrom::Start(0)).unwrap();

    let calls = Rc::new(Cell::new(0));
    let observed = Rc::clone(&calls);
    let rows = std::iter::once_with(move || {
        observed.set(observed.get() + 1);
        Ok(dynamic_insert_row("Never read", 1))
    });
    let mut source = FailOnceReader::new(complex.clone());
    let mut destination = Cursor::new(Vec::new());
    let error = MiniExcel::insert_with_schema_from_reader_to_writer(
        &mut source,
        &mut destination,
        &schema,
        rows,
        &InsertOptions::new().with_sheet_name("Archive 2"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("source failed"));
    assert_eq!(calls.get(), 0);
    assert!(destination.get_ref().is_empty());
    source.seek(SeekFrom::Start(0)).unwrap();
    let mut signature = [0; 2];
    source.read_exact(&mut signature).unwrap();
    assert_eq!(&signature, b"PK");

    let simple = MiniExcel::save_as_bytes(
        &[dynamic_insert_row("Current", 1)],
        &WriteOptions::new().with_sheet_name("Current"),
    )
    .unwrap();
    let calls = Rc::new(Cell::new(0));
    let observed = Rc::clone(&calls);
    let rows = std::iter::from_fn(move || {
        let call = observed.get() + 1;
        observed.set(call);
        match call {
            1 => Some(Ok(dynamic_insert_row("Before error", 1))),
            2 => Some(Err(std::io::Error::other("producer failed").into())),
            _ => panic!("iterator consumed after producer error"),
        }
    });
    let mut source = Cursor::new(simple.clone());
    let mut destination = Cursor::new(Vec::new());
    let error = MiniExcel::insert_with_schema_from_reader_to_writer(
        &mut source,
        &mut destination,
        &schema,
        rows,
        &InsertOptions::new().with_sheet_name("Broken"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("producer failed"));
    assert_eq!(calls.get(), 2);
    assert!(destination.get_ref().is_empty());
    assert_eq!(source.get_ref(), &simple);
}

#[test]
fn borrowed_io_rejects_nonempty_sink_and_propagates_destination_errors() {
    let source_bytes = MiniExcel::save_as_bytes(
        &[dynamic_insert_row("Current", 1)],
        &WriteOptions::new().with_sheet_name("Current"),
    )
    .unwrap();
    let schema = ["Name".to_owned(), "Version".to_owned()];
    let calls = Rc::new(Cell::new(0));
    let observed = Rc::clone(&calls);
    let rows = std::iter::once_with(move || {
        observed.set(observed.get() + 1);
        Ok(dynamic_insert_row("Never read", 1))
    });
    let mut source = Cursor::new(source_bytes.clone());
    let mut destination = Cursor::new(b"preserve me".to_vec());
    let error = MiniExcel::insert_with_schema_from_reader_to_writer(
        &mut source,
        &mut destination,
        &schema,
        rows,
        &InsertOptions::new().with_sheet_name("Archive"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("empty seekable sink"));
    assert_eq!(calls.get(), 0);
    assert_eq!(destination.get_ref(), b"preserve me");
    assert_eq!(source.get_ref(), &source_bytes);

    let mut source = Cursor::new(source_bytes.clone());
    let mut destination = FailOnceWriter::new();
    let error = MiniExcel::insert_from_reader_to_writer(
        &mut source,
        &mut destination,
        &[dynamic_insert_row("Archive", 2)],
        &InsertOptions::new().with_sheet_name("Archive"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("destination failed"));
    assert_eq!(source.get_ref(), &source_bytes);
    destination.seek(SeekFrom::Start(0)).unwrap();
    destination.write_all(b"usable").unwrap();
}

#[cfg(feature = "async")]
#[test]
fn async_producer_public_api_appends_and_stops_on_error() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("async.xlsx");
    MiniExcel::insert(
        &path,
        &[dynamic_insert_row("Current", 1)],
        &InsertOptions::new().with_sheet_name("Current"),
    )
    .unwrap();

    let rows = futures_util::stream::iter([
        Ok(dynamic_insert_row("Async", 2)),
        Ok(dynamic_insert_row("Async", 3)),
    ]);
    let count = futures_executor::block_on(MiniExcel::insert_with_schema_async(
        &path,
        &["Name".to_owned(), "Version".to_owned()],
        rows,
        &InsertOptions::new().with_sheet_name("Async"),
    ))
    .unwrap();
    assert_eq!(count, 2);
    assert_eq!(MiniExcel::get_sheet_names(&path).unwrap(), ["Current", "Async"]);

    let before = std::fs::read(&path).unwrap();
    let polls = Rc::new(Cell::new(0));
    let observed = Rc::clone(&polls);
    let rows = futures_util::stream::poll_fn(move |_| {
        let poll = observed.get() + 1;
        observed.set(poll);
        match poll {
            1 => std::task::Poll::Ready(Some(Ok(dynamic_insert_row("Before error", 1)))),
            2 => std::task::Poll::Ready(Some(Err(
                std::io::Error::other("async producer failed").into()
            ))),
            _ => panic!("async producer was polled after its error"),
        }
    });
    let error = futures_executor::block_on(MiniExcel::insert_with_schema_async(
        &path,
        &["Name".to_owned(), "Version".to_owned()],
        rows,
        &InsertOptions::new().with_sheet_name("Broken"),
    ))
    .unwrap_err();
    assert!(error.to_string().contains("async producer failed"));
    assert_eq!(polls.get(), 2);
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[cfg(feature = "async")]
#[test]
fn async_producer_pre_cancel_does_not_poll_or_change_source() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cancelled.xlsx");
    MiniExcel::insert(
        &path,
        &[dynamic_insert_row("Current", 1)],
        &InsertOptions::new().with_sheet_name("Current"),
    )
    .unwrap();
    let before = std::fs::read(&path).unwrap();
    let cancellation = miniexcel::CancellationToken::new();
    cancellation.cancel();
    let polls = Rc::new(Cell::new(0));
    let observed = Rc::clone(&polls);
    let rows = futures_util::stream::iter([Ok(dynamic_insert_row("Never read", 2))])
        .inspect(move |_| observed.set(observed.get() + 1));

    let error = futures_executor::block_on(MiniExcel::insert_with_schema_async_with_cancellation(
        &path,
        &["Name".to_owned(), "Version".to_owned()],
        rows,
        &InsertOptions::new().with_sheet_name("Cancelled"),
        cancellation,
    ))
    .unwrap_err();

    assert!(error.is_cancelled());
    assert_eq!(polls.get(), 0);
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn hardening_strict_namespace_fails_preflight_without_consuming_rows() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("strict.xlsx");
    let source = strict_namespace_fixture();
    std::fs::write(&path, &source).unwrap();

    let calls = Rc::new(Cell::new(0));
    let observed = Rc::clone(&calls);
    let rows = std::iter::once_with(move || {
        observed.set(observed.get() + 1);
        Ok(dynamic_insert_row("Never read", 1))
    });
    let error = MiniExcel::insert_with_schema(
        &path,
        &["Name".to_owned(), "Version".to_owned()],
        rows,
        &InsertOptions::new().with_sheet_name("Strict Append"),
    )
    .unwrap_err();

    assert!(error.to_string().contains("Strict OOXML"));
    assert_eq!(calls.get(), 0);
    assert_eq!(std::fs::read(path).unwrap(), source);
}

#[test]
fn hardening_security_fixtures_fail_before_consuming_rows_or_committing() {
    let oversized_value = "x".repeat(65_537);
    let fixtures = [
        (
            "traversal",
            add_fixture_entry(complex_fixture(), "../escape.xml", b"escape"),
            "unsafe ZIP entry path",
        ),
        (
            "normalized-alias",
            add_fixture_entry(complex_fixture(), "xl/%73tyles.xml", b"alias"),
            "non-canonical percent encoding",
        ),
        (
            "duplicate-target",
            rewrite_fixture_entry(complex_fixture(), "xl/_rels/workbook.xml.rels", |xml| {
                xml.replace(
                    "</Relationships>",
                    r#"<Relationship Id="rId100" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/data.xml"/></Relationships>"#,
                )
            }),
            "relationship target",
        ),
        (
            "relationship-cycle",
            rewrite_fixture_entry(complex_fixture(), "xl/worksheets/_rels/data.xml.rels", |xml| {
                xml.replace(
                        "</Relationships>",
                        r#"<Relationship Id="rId99" Type="urn:miniexcel:test-cycle" Target="../workbook.xml"/></Relationships>"#,
                    )
            }),
            "relationship cycle",
        ),
        (
            "oversized-attribute",
            rewrite_fixture_entry(complex_fixture(), "xl/workbook.xml", |xml| {
                xml.replacen(
                    "<workbook ",
                    &format!("<workbook oversized=\"{oversized_value}\" "),
                    1,
                )
            }),
            "oversized XML attribute",
        ),
    ];

    let directory = tempfile::tempdir().unwrap();
    for (name, source, expected_error) in fixtures {
        let path = directory.path().join(format!("{name}.xlsx"));
        std::fs::write(&path, &source).unwrap();
        let calls = Rc::new(Cell::new(0));
        let observed = Rc::clone(&calls);
        let rows = std::iter::once_with(move || {
            observed.set(observed.get() + 1);
            Ok(dynamic_insert_row("Never read", 1))
        });
        let error = MiniExcel::insert_with_schema(
            &path,
            &["Name".to_owned(), "Version".to_owned()],
            rows,
            &InsertOptions::new().with_sheet_name("Hardened Append"),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains(expected_error),
            "fixture {name}: expected '{expected_error}', got '{error}'"
        );
        assert_eq!(calls.get(), 0, "fixture {name} consumed rows");
        assert_eq!(std::fs::read(&path).unwrap(), source, "fixture {name} changed source");
    }
}

#[test]
fn hardening_concurrent_same_path_insert_has_one_success_and_one_conflict() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("concurrent.xlsx");
    MiniExcel::insert(
        &path,
        &[dynamic_insert_row("Current", 1)],
        &InsertOptions::new().with_sheet_name("Current"),
    )
    .unwrap();

    let (entered_sender, entered_receiver) = std::sync::mpsc::channel();
    let (release_sender, release_receiver) = std::sync::mpsc::channel();
    let first_path = path.clone();
    let first = std::thread::spawn(move || {
        let rows = std::iter::once_with(move || {
            entered_sender.send(()).unwrap();
            release_receiver.recv().unwrap();
            Ok(dynamic_insert_row("First", 2))
        });
        MiniExcel::insert_with_schema(
            first_path,
            &["Name".to_owned(), "Version".to_owned()],
            rows,
            &InsertOptions::new().with_sheet_name("First"),
        )
    });

    entered_receiver.recv().unwrap();
    let second = MiniExcel::insert(
        &path,
        &[dynamic_insert_row("Second", 3)],
        &InsertOptions::new().with_sheet_name("Second"),
    )
    .unwrap_err();
    assert!(second.to_string().contains("concurrent Insert conflict"));
    release_sender.send(()).unwrap();
    assert_eq!(first.join().unwrap().unwrap(), 1);

    assert_eq!(MiniExcel::get_sheet_names(&path).unwrap(), ["Current", "First"]);
    let rows = MiniExcel::query_with_options(
        &path,
        &ReadOptions::new().with_sheet_name("First").with_header_mode(HeaderMode::FirstRow),
    )
    .unwrap()
    .collect::<miniexcel::Result<Vec<_>>>()
    .unwrap();
    assert_eq!(rows[0]["Version"], CellValue::Int(2));
}

#[test]
fn hardening_source_change_during_insert_aborts_before_commit() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("source-change.xlsx");
    MiniExcel::insert(
        &path,
        &[dynamic_insert_row("Current", 1)],
        &InsertOptions::new().with_sheet_name("Current"),
    )
    .unwrap();

    let replacement = MiniExcel::save_as_bytes(
        &[dynamic_insert_row("External", 9)],
        &WriteOptions::new().with_sheet_name("External"),
    )
    .unwrap();
    let expected = replacement.clone();
    let (entered_sender, entered_receiver) = std::sync::mpsc::channel();
    let (release_sender, release_receiver) = std::sync::mpsc::channel();
    let insert_path = path.clone();
    let insert = std::thread::spawn(move || {
        let rows = std::iter::once_with(move || {
            entered_sender.send(()).unwrap();
            release_receiver.recv().unwrap();
            Ok(dynamic_insert_row("Inserted", 2))
        });
        MiniExcel::insert_with_schema(
            insert_path,
            &["Name".to_owned(), "Version".to_owned()],
            rows,
            &InsertOptions::new().with_sheet_name("Inserted"),
        )
    });

    entered_receiver.recv().unwrap();
    std::fs::write(&path, replacement).unwrap();
    release_sender.send(()).unwrap();
    let error = insert.join().unwrap().unwrap_err();
    assert!(error.to_string().contains("changed during Insert"));
    assert_eq!(std::fs::read(&path).unwrap(), expected);
    assert_eq!(MiniExcel::get_sheet_names(path).unwrap(), ["External"]);
}

#[test]
#[ignore = "million-row disk and memory stress; set MINIEXCEL_INSERT_STRESS_ROWS to override"]
fn hardening_million_row_insert_is_disk_backed() {
    let row_count = std::env::var("MINIEXCEL_INSERT_STRESS_ROWS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1_000_000_usize);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("million.xlsx");
    MiniExcel::insert(
        &path,
        &[dynamic_insert_row("Current", 1)],
        &InsertOptions::new().with_sheet_name("Current"),
    )
    .unwrap();

    let rows = (0..row_count).map(|index| {
        let mut row = DynamicRow::new();
        row.insert("Name".to_owned(), CellValue::String("Stream".to_owned()));
        row.insert("Version".to_owned(), CellValue::Int(index as i64));
        Ok(row)
    });
    let written = MiniExcel::insert_with_schema(
        &path,
        &["Name".to_owned(), "Version".to_owned()],
        rows,
        &InsertOptions::new().with_write_options(
            WriteOptions::new().with_sheet_name("Million").with_auto_filter(false),
        ),
    )
    .unwrap();

    assert_eq!(written, row_count);
    assert_eq!(MiniExcel::get_sheet_names(&path).unwrap(), ["Current", "Million"]);
    assert!(std::fs::metadata(&path).unwrap().len() < 1024 * 1024 * 1024);
    if let Some(output) = std::env::var_os("MINIEXCEL_INSERT_STRESS_OUTPUT") {
        std::fs::copy(path, output).unwrap();
    }
}

#[test]
fn public_append_explicit_schema_iterator_is_one_pass_and_failure_safe() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("streamed.xlsx");
    let schema = vec!["Name".to_owned(), "Version".to_owned()];
    let calls = Rc::new(Cell::new(0));
    let observed = Rc::clone(&calls);
    let rows = (0..3).map(move |version| {
        observed.set(observed.get() + 1);
        Ok(dynamic_insert_row("Stream", version))
    });

    let count = MiniExcel::insert_with_schema(
        &path,
        &schema,
        rows,
        &InsertOptions::new().with_sheet_name("Stream"),
    )
    .unwrap();
    assert_eq!(count, 3);
    assert_eq!(calls.get(), 3);

    let appended = MiniExcel::insert_with_schema(
        &path,
        &schema,
        std::iter::once(Ok(dynamic_insert_row("Archive", 4))),
        &InsertOptions::new().with_sheet_name("Archive"),
    )
    .unwrap();
    assert_eq!(appended, 1);
    assert_eq!(MiniExcel::get_sheet_names(&path).unwrap(), ["Stream", "Archive"]);

    let before = std::fs::read(&path).unwrap();
    let failing_rows = [
        Ok(dynamic_insert_row("Before error", 1)),
        Err(std::io::Error::other("producer failed").into()),
    ];
    assert!(
        MiniExcel::insert_with_schema(
            &path,
            &schema,
            failing_rows,
            &InsertOptions::new().with_sheet_name("Broken"),
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(MiniExcel::get_sheet_names(&path).unwrap(), ["Stream", "Archive"]);

    let missing = directory.path().join("missing-failure.xlsx");
    let failing_rows = [Err(std::io::Error::other("producer failed").into())];
    assert!(
        MiniExcel::insert_with_schema(
            &missing,
            &schema,
            failing_rows,
            &InsertOptions::new().with_sheet_name("Broken"),
        )
        .is_err()
    );
    assert!(!missing.exists());
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct PublicInsertRelease {
    name: String,
    version: u32,
}

#[test]
fn public_append_supports_serde_rows_for_missing_and_existing_paths() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("typed.xlsx");
    let current = [PublicInsertRelease { name: "MiniExcel".to_owned(), version: 2 }];
    assert_eq!(
        MiniExcel::insert_serialized(
            &path,
            &current,
            &InsertOptions::new().with_sheet_name("Current"),
        )
        .unwrap(),
        1
    );

    let archive = [PublicInsertRelease { name: "Rust".to_owned(), version: 1 }];
    assert_eq!(
        MiniExcel::insert_serialized(
            &path,
            &archive,
            &InsertOptions::new().with_sheet_name("Archive"),
        )
        .unwrap(),
        1
    );
    assert_eq!(MiniExcel::get_sheet_names(&path).unwrap(), ["Current", "Archive"]);
}

#[test]
fn public_append_rejects_unsupported_policies_and_invalid_options_before_output() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing.xlsx");
    let rows = [dynamic_insert_row("MiniExcel", 1)];

    for options in [
        InsertOptions::new()
            .with_sheet_name("Data")
            .with_target_relationship_policy(TargetRelationshipPolicy::RemoveSupported),
        InsertOptions::new().with_sheet_name("Invalid/Name"),
        InsertOptions::new().with_write_options(
            WriteOptions::new().with_sheet_name("Data").with_min_width(f64::NAN),
        ),
        InsertOptions::new().with_write_options(
            WriteOptions::new().with_sheet_name("Data").with_overwrite_file(true),
        ),
    ] {
        assert!(MiniExcel::insert(&missing, &rows, &options).is_err());
        assert!(!missing.exists());
    }

    let replace_missing = directory.path().join("replace-missing.xlsx");
    assert_eq!(
        MiniExcel::insert(
            &replace_missing,
            &rows,
            &InsertOptions::new()
                .with_sheet_name("Data")
                .with_existing_sheet_policy(ExistingSheetPolicy::Replace),
        )
        .unwrap(),
        1
    );
    assert_eq!(MiniExcel::get_sheet_names(&replace_missing).unwrap(), ["Data"]);

    let invalid_schema = vec!["Value".to_owned(), "Value".to_owned()];
    let calls = Rc::new(Cell::new(0));
    let observed = Rc::clone(&calls);
    let rows = std::iter::once_with(move || {
        observed.set(observed.get() + 1);
        Ok(dynamic_insert_row("Never read", 1))
    });
    assert!(
        MiniExcel::insert_with_schema(
            &missing,
            &invalid_schema,
            rows,
            &InsertOptions::new().with_sheet_name("Data"),
        )
        .is_err()
    );
    assert_eq!(calls.get(), 0);
    assert!(!missing.exists());

    let oversized_schema = (0..=16_384).map(|column| format!("Column{column}")).collect::<Vec<_>>();
    let calls = Rc::new(Cell::new(0));
    let observed = Rc::clone(&calls);
    let rows = std::iter::once_with(move || {
        observed.set(observed.get() + 1);
        Ok(dynamic_insert_row("Never read", 1))
    });
    assert!(
        MiniExcel::insert_with_schema(
            &missing,
            &oversized_schema,
            rows,
            &InsertOptions::new().with_sheet_name("Data"),
        )
        .is_err()
    );
    assert_eq!(calls.get(), 0);
    assert!(!missing.exists());

    let macro_path = directory.path().join("book.xlsm");
    assert!(MiniExcel::insert(&macro_path, &rows_for_macro(), &InsertOptions::new()).is_err());
    assert!(!macro_path.exists());

    MiniExcel::insert(&missing, &rows_for_macro(), &InsertOptions::new().with_sheet_name("Data"))
        .unwrap();
    let before = std::fs::read(&missing).unwrap();
    let calls = Rc::new(Cell::new(0));
    let observed = Rc::clone(&calls);
    let duplicate_rows = std::iter::once_with(move || {
        observed.set(observed.get() + 1);
        Ok(dynamic_insert_row("Never read", 1))
    });
    assert!(
        MiniExcel::insert_with_schema(
            &missing,
            &["Name".to_owned(), "Version".to_owned()],
            duplicate_rows,
            &InsertOptions::new().with_sheet_name("data"),
        )
        .is_err()
    );
    assert_eq!(calls.get(), 0);
    assert!(
        MiniExcel::insert(
            &missing,
            &rows_for_macro(),
            &InsertOptions::new().with_sheet_name("data"),
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&missing).unwrap(), before);
}

#[test]
fn replace_sheet_preserves_target_identity_order_visibility_and_active_state() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("replace-plain.xlsx");
    let current = [dynamic_insert_row("Current", 1)];
    let hidden = [dynamic_insert_row("Old", 2)];
    MiniExcel::save_as_sheets(
        &path,
        [("Current", current.as_slice()), ("Hidden", hidden.as_slice())],
        &WriteOptions::new().with_sheet_visibility("Hidden", SheetVisibility::Hidden),
    )
    .unwrap();
    let before_bytes = std::fs::read(&path).unwrap();
    let before = package_inventory(&before_bytes);
    let current_xml = read_entry(&before_bytes, &format!("xl/{}", before.sheets[0].target));
    let target_xml = entry_text(&before_bytes, &format!("xl/{}", before.sheets[1].target));
    let workbook_relationships = before
        .relationships
        .iter()
        .filter(|relationship| relationship.source == "xl/workbook.xml")
        .collect::<Vec<_>>();

    let replacement = [dynamic_insert_row("New", 7)];
    let count = MiniExcel::insert(
        &path,
        &replacement,
        &InsertOptions::new()
            .with_write_options(
                WriteOptions::new().with_sheet_name("hidden").with_auto_filter(false),
            )
            .with_existing_sheet_policy(ExistingSheetPolicy::Replace),
    )
    .unwrap();
    assert_eq!(count, 1);

    let after_bytes = std::fs::read(&path).unwrap();
    let after = package_inventory(&after_bytes);
    assert_eq!(after.sheets, before.sheets);
    assert_eq!(after.active_tab, before.active_tab);
    assert_eq!(
        after
            .relationships
            .iter()
            .filter(|relationship| relationship.source == "xl/workbook.xml")
            .collect::<Vec<_>>(),
        workbook_relationships
    );
    assert_eq!(read_entry(&after_bytes, &format!("xl/{}", after.sheets[0].target)), current_xml);
    assert_eq!(
        entry_text(&after_bytes, &format!("xl/{}", after.sheets[1].target))
            .contains("tabSelected="),
        target_xml.contains("tabSelected=")
    );
    assert!(
        !after
            .defined_names
            .iter()
            .any(|name| { name.name == "_xlnm._FilterDatabase" && name.local_sheet_id == Some(1) })
    );
    let rows = MiniExcel::query_with_options(
        &path,
        &ReadOptions::new().with_sheet_name("Hidden").with_header_mode(HeaderMode::FirstRow),
    )
    .unwrap()
    .collect::<miniexcel::Result<Vec<_>>>()
    .unwrap();
    assert_eq!(rows[0]["Name"], CellValue::String("New".to_owned()));
    assert_eq!(rows[0]["Version"], CellValue::Int(7));
}

#[test]
fn replace_sheet_strict_policy_rejects_complex_target_before_consuming_rows() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("replace-strict.xlsx");
    let source = complex_fixture();
    std::fs::write(&path, &source).unwrap();
    let calls = Rc::new(Cell::new(0));
    let observed = Rc::clone(&calls);
    let rows = std::iter::once_with(move || {
        observed.set(observed.get() + 1);
        Ok(dynamic_insert_row("Never written", 1))
    });

    assert!(
        MiniExcel::insert_with_schema(
            &path,
            &["Name".to_owned(), "Version".to_owned()],
            rows,
            &InsertOptions::new()
                .with_sheet_name("Data")
                .with_existing_sheet_policy(ExistingSheetPolicy::Replace),
        )
        .is_err()
    );
    assert_eq!(calls.get(), 0);
    assert_eq!(std::fs::read(&path).unwrap(), source);
}

#[test]
fn replace_sheet_remove_supported_deletes_only_target_owned_closure() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("replace-remove-supported.xlsx");
    let source = complex_fixture();
    let before = package_inventory(&source);
    let hidden_sheet = read_entry(&source, "xl/worksheets/sheet7.xml");
    let archive_sheet = read_entry(&source, "xl/worksheets/archive.xml");
    let custom_xml = read_entry(&source, "customXml/item1.xml");
    let removed_entries = BTreeSet::from([
        "xl/worksheets/_rels/data.xml.rels",
        "xl/tables/table1.xml",
        "xl/drawings/drawing1.xml",
        "xl/drawings/_rels/drawing1.xml.rels",
        "xl/media/image1.png",
        "xl/comments1.xml",
        "xl/drawings/vmlDrawing1.vml",
    ]);
    std::fs::write(&path, &source).unwrap();

    let count = MiniExcel::insert(
        &path,
        &[dynamic_insert_row("Replacement", 9)],
        &InsertOptions::new()
            .with_sheet_name("data")
            .with_existing_sheet_policy(ExistingSheetPolicy::Replace)
            .with_target_relationship_policy(TargetRelationshipPolicy::RemoveSupported),
    )
    .unwrap();
    assert_eq!(count, 1);

    let output = std::fs::read(&path).unwrap();
    let after = package_inventory(&output);
    assert_eq!(after.sheets, before.sheets);
    assert_eq!(after.active_tab, before.active_tab);
    assert_eq!(read_entry(&output, "xl/worksheets/sheet7.xml"), hidden_sheet);
    assert_eq!(read_entry(&output, "xl/worksheets/archive.xml"), archive_sheet);
    assert_eq!(read_entry(&output, "customXml/item1.xml"), custom_xml);
    for removed in &removed_entries {
        assert!(!after.entries.contains_key(*removed), "entry '{removed}' was retained");
    }
    for name in before.entries.keys().filter(|name| name.ends_with(".rels")) {
        if !removed_entries.contains(name.as_str()) {
            assert_eq!(read_entry(&output, name), read_entry(&source, name), "{name} changed");
        }
    }
    assert!(
        after.defined_names.iter().any(|name| {
            name.name == "_xlnm._FilterDatabase"
                && name.local_sheet_id == Some(0)
                && name.formula == "'Data'!$A$1:$B$2"
        }),
        "defined names: {:?}",
        after.defined_names
    );
    assert!(after.defined_names.iter().any(|name| name.name == "GlobalTotal"));
    assert!(after.defined_names.iter().any(|name| name.name == "_xlnm.Print_Area"));
    assert!(after.defined_names.iter().any(|name| name.name == "HiddenFormula"));
    assert!(after.relationships.iter().all(|relationship| {
        relationship.source != "xl/worksheets/data.xml"
            && relationship.source != "xl/drawings/drawing1.xml"
    }));
    let rows = MiniExcel::query_with_options(
        &path,
        &ReadOptions::new().with_sheet_name("Data").with_header_mode(HeaderMode::FirstRow),
    )
    .unwrap()
    .collect::<miniexcel::Result<Vec<_>>>()
    .unwrap();
    assert_eq!(rows[0]["Name"], CellValue::String("Replacement".to_owned()));
    assert_eq!(rows[0]["Version"], CellValue::Int(9));
}

#[test]
fn calculation_policy_append_preserves_chain_relationship_and_calc_properties() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("append-calc-chain.xlsx");
    let source = calculation_fixture();
    std::fs::write(&path, &source).unwrap();
    let calc_chain = read_entry(&source, "xl/calcChain.xml");
    let calc_chain_relationship = xml_element_by_attribute(
        &read_entry(&source, "xl/_rels/workbook.xml.rels"),
        b"Relationship",
        b"Type",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain",
    );
    let calc_chain_override = xml_element_by_attribute(
        &read_entry(&source, "[Content_Types].xml"),
        b"Override",
        b"PartName",
        "/xl/calcChain.xml",
    );
    let calc_pr = xml_element_bytes(&read_entry(&source, "xl/workbook.xml"), b"calcPr");

    MiniExcel::insert(
        &path,
        &[dynamic_insert_row("Appended", 10)],
        &InsertOptions::new().with_sheet_name("Appended"),
    )
    .unwrap();

    let output = std::fs::read(&path).unwrap();
    assert_eq!(read_entry(&output, "xl/calcChain.xml"), calc_chain);
    assert_eq!(
        xml_element_by_attribute(
            &read_entry(&output, "xl/_rels/workbook.xml.rels"),
            b"Relationship",
            b"Type",
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain",
        ),
        calc_chain_relationship
    );
    assert_eq!(
        xml_element_by_attribute(
            &read_entry(&output, "[Content_Types].xml"),
            b"Override",
            b"PartName",
            "/xl/calcChain.xml",
        ),
        calc_chain_override
    );
    assert_eq!(xml_element_bytes(&read_entry(&output, "xl/workbook.xml"), b"calcPr"), calc_pr);
}

#[test]
fn calculation_policy_replace_removes_chain_and_preserves_unrelated_formulas_and_names() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("replace-calc-chain.xlsx");
    let source = calculation_fixture();
    let before = package_inventory(&source);
    let hidden_sheet = read_entry(&source, "xl/worksheets/sheet7.xml");
    let archive_sheet = read_entry(&source, "xl/worksheets/archive.xml");
    std::fs::write(&path, &source).unwrap();

    MiniExcel::insert(
        &path,
        &[dynamic_insert_row("Replacement", 11)],
        &InsertOptions::new()
            .with_write_options(WriteOptions::new().with_sheet_name("Data").with_auto_filter(false))
            .with_existing_sheet_policy(ExistingSheetPolicy::Replace)
            .with_target_relationship_policy(TargetRelationshipPolicy::RemoveSupported),
    )
    .unwrap();

    let output = std::fs::read(&path).unwrap();
    let after = package_inventory(&output);
    assert!(!after.entries.contains_key("xl/calcChain.xml"));
    assert!(!entry_text(&output, "xl/_rels/workbook.xml.rels").contains("calcChain"));
    assert!(!entry_text(&output, "[Content_Types].xml").contains("calcChain"));
    let workbook = entry_text(&output, "xl/workbook.xml");
    let calc_pr = String::from_utf8(xml_element_bytes(workbook.as_bytes(), b"calcPr")).unwrap();
    assert!(calc_pr.contains("calcId=\"191029\""));
    assert!(calc_pr.contains("fullCalcOnLoad=\"1\""));
    assert!(calc_pr.contains("forceFullCalc=\"1\""));

    assert_eq!(read_entry(&output, "xl/worksheets/sheet7.xml"), hidden_sheet);
    assert_eq!(read_entry(&output, "xl/worksheets/archive.xml"), archive_sheet);
    assert!(entry_text(&output, "xl/worksheets/sheet7.xml").contains("<f>'Data'!C2</f>"));
    assert!(entry_text(&output, "xl/worksheets/archive.xml").contains("<f>'Data'!B2+1</f>"));
    assert_eq!(after.sheets, before.sheets);
    assert!(
        after
            .defined_names
            .iter()
            .any(|name| { name.name == "GlobalTotal" && name.formula == "'Data'!$C$2" })
    );
    assert!(
        after
            .defined_names
            .iter()
            .any(|name| { name.name == "_xlnm.Print_Area" && name.formula == "'Data'!$A$1:$C$2" })
    );
    assert!(
        after
            .defined_names
            .iter()
            .any(|name| { name.name == "HiddenFormula" && name.formula == "'HiddenCalc'!$A$2" })
    );
    assert!(
        !after
            .defined_names
            .iter()
            .any(|name| { name.name == "_xlnm._FilterDatabase" && name.local_sheet_id == Some(0) })
    );
    assert!(after.defined_names.iter().any(|name| {
        name.name == "_xlnm._FilterDatabase"
            && name.local_sheet_id == Some(1)
            && name.formula == "'HiddenCalc'!$A$1:$A$2"
    }));
}

fn dynamic_insert_row(name: &str, version: i64) -> DynamicRow {
    let mut row = DynamicRow::new();
    row.insert("Name".to_owned(), CellValue::String(name.to_owned()));
    row.insert("Version".to_owned(), CellValue::Int(version));
    row
}

fn rows_for_macro() -> [DynamicRow; 1] {
    [dynamic_insert_row("MiniExcel", 1)]
}

fn calculation_fixture() -> Vec<u8> {
    let source = complex_fixture();
    let mut archive = ZipArchive::new(Cursor::new(&source)).unwrap();
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let name = entry.name().to_owned();
        if name == "xl/_rels/workbook.xml.rels" {
            let options = entry.options();
            let mut xml = String::new();
            entry.read_to_string(&mut xml).unwrap();
            let xml = xml.replace(
                "</Relationships>",
                r#"<Relationship Id="rId30" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain" Target="calcChain.xml"/></Relationships>"#,
            );
            writer.start_file(name, options).unwrap();
            writer.write_all(xml.as_bytes()).unwrap();
        } else if name == "[Content_Types].xml" {
            let options = entry.options();
            let mut xml = String::new();
            entry.read_to_string(&mut xml).unwrap();
            let xml = xml.replace(
                "</Types>",
                r#"<Override PartName="/xl/calcChain.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml"/></Types>"#,
            );
            writer.start_file(name, options).unwrap();
            writer.write_all(xml.as_bytes()).unwrap();
        } else if name == "xl/workbook.xml" {
            let options = entry.options();
            let mut xml = String::new();
            entry.read_to_string(&mut xml).unwrap();
            let xml = xml.replace(
                "</definedNames>",
                "<definedName name=\"_xlnm._FilterDatabase\" localSheetId=\"0\" hidden=\"1\">'Data'!$A$1:$C$2</definedName><definedName name=\"_xlnm._FilterDatabase\" localSheetId=\"1\" hidden=\"1\">'HiddenCalc'!$A$1:$A$2</definedName></definedNames>",
            );
            writer.start_file(name, options).unwrap();
            writer.write_all(xml.as_bytes()).unwrap();
        } else {
            writer.raw_copy_file(entry).unwrap();
        }
    }
    writer
        .start_file(
            "xl/calcChain.xml",
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .unwrap();
    writer
        .write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?><calcChain xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><c r="C2" i="1"/><c r="A2" i="2"/><c r="A1" i="3"/></calcChain>"#,
        )
        .unwrap();
    writer.finish().unwrap().into_inner()
}

fn strict_namespace_fixture() -> Vec<u8> {
    let source = complex_fixture();
    let mut archive = ZipArchive::new(Cursor::new(&source)).unwrap();
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let name = entry.name().to_owned();
        if name == "xl/workbook.xml" || name == "xl/_rels/workbook.xml.rels" {
            let options = entry.options();
            let mut xml = String::new();
            entry.read_to_string(&mut xml).unwrap();
            let xml = xml
                .replace(
                    "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
                    "http://purl.oclc.org/ooxml/spreadsheetml/main",
                )
                .replace(
                    "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
                    "http://purl.oclc.org/ooxml/officeDocument/relationships",
                );
            writer.start_file(name, options).unwrap();
            writer.write_all(xml.as_bytes()).unwrap();
        } else {
            writer.raw_copy_file(entry).unwrap();
        }
    }
    writer.finish().unwrap().into_inner()
}

fn rewrite_fixture_entry<F>(source: Vec<u8>, target: &str, transform: F) -> Vec<u8>
where
    F: FnOnce(String) -> String,
{
    let mut archive = ZipArchive::new(Cursor::new(source)).unwrap();
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let mut transform = Some(transform);
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let name = entry.name().to_owned();
        if name == target {
            let options = entry.options();
            let mut xml = String::new();
            entry.read_to_string(&mut xml).unwrap();
            writer.start_file(name, options).unwrap();
            writer.write_all(transform.take().unwrap()(xml).as_bytes()).unwrap();
        } else {
            writer.raw_copy_file(entry).unwrap();
        }
    }
    assert!(transform.is_none(), "fixture entry '{target}' was not found");
    writer.finish().unwrap().into_inner()
}

fn add_fixture_entry(source: Vec<u8>, name: &str, payload: &[u8]) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(source)).unwrap();
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        writer.raw_copy_file(archive.by_index(index).unwrap()).unwrap();
    }
    writer
        .start_file(
            name,
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .unwrap();
    writer.write_all(payload).unwrap();
    writer.finish().unwrap().into_inner()
}

fn xml_element_bytes(xml: &[u8], name: &[u8]) -> Vec<u8> {
    let mut reader = Reader::from_reader(xml);
    let mut writer = quick_xml::Writer::new(Vec::new());
    let mut depth = 0;
    loop {
        let event = reader.read_event().unwrap();
        if depth == 0 {
            match &event {
                Event::Start(start) if local_name(start.name().as_ref()) == name => depth = 1,
                Event::Empty(empty) if local_name(empty.name().as_ref()) == name => {
                    writer.write_event(event).unwrap();
                    break;
                }
                Event::Eof => panic!("XML element '{}' not found", String::from_utf8_lossy(name)),
                _ => continue,
            }
        } else {
            match &event {
                Event::Start(_) => depth += 1,
                Event::End(_) => depth -= 1,
                Event::Eof => {
                    panic!("XML element '{}' is incomplete", String::from_utf8_lossy(name))
                }
                _ => {}
            }
        }
        writer.write_event(event).unwrap();
        if depth == 0 {
            break;
        }
    }
    writer.into_inner()
}

fn xml_element_by_attribute(
    xml: &[u8],
    name: &[u8],
    attribute_name: &[u8],
    expected_value: &str,
) -> Vec<u8> {
    let mut reader = Reader::from_reader(xml);
    loop {
        let event = reader.read_event().unwrap();
        match event {
            Event::Empty(element)
                if local_name(element.name().as_ref()) == name
                    && attribute(&element, attribute_name).as_deref() == Some(expected_value) =>
            {
                let mut writer = quick_xml::Writer::new(Vec::new());
                writer.write_event(Event::Empty(element)).unwrap();
                return writer.into_inner();
            }
            Event::Start(element)
                if local_name(element.name().as_ref()) == name
                    && attribute(&element, attribute_name).as_deref() == Some(expected_value) =>
            {
                let mut writer = quick_xml::Writer::new(Vec::new());
                writer.write_event(Event::Start(element)).unwrap();
                let mut depth = 1;
                while depth > 0 {
                    let event = reader.read_event().unwrap();
                    match &event {
                        Event::Start(_) => depth += 1,
                        Event::End(_) => depth -= 1,
                        Event::Eof => {
                            panic!("XML element '{}' is incomplete", String::from_utf8_lossy(name))
                        }
                        _ => {}
                    }
                    writer.write_event(event).unwrap();
                }
                return writer.into_inner();
            }
            Event::Eof => panic!(
                "XML element '{}' with attribute '{}'='{}' not found",
                String::from_utf8_lossy(name),
                String::from_utf8_lossy(attribute_name),
                expected_value
            ),
            _ => {}
        }
    }
}

fn complex_fixture() -> Vec<u8> {
    let entries = fixture_entries();
    let output = Cursor::new(Vec::new());
    let mut archive = ZipWriter::new(output);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(DateTime::default());
    for (name, payload) in entries {
        archive.start_file(name, options).expect("start fixture ZIP entry");
        archive.write_all(&payload).expect("write fixture ZIP entry");
    }
    archive.finish().expect("finish fixture ZIP").into_inner()
}

fn fixture_entries() -> BTreeMap<&'static str, Vec<u8>> {
    let mut entries = BTreeMap::new();
    entries.insert("[Content_Types].xml", CONTENT_TYPES.as_bytes().to_vec());
    entries.insert("_rels/.rels", ROOT_RELS.as_bytes().to_vec());
    entries.insert("docProps/app.xml", APP_PROPERTIES.as_bytes().to_vec());
    entries.insert("docProps/core.xml", CORE_PROPERTIES.as_bytes().to_vec());
    entries.insert("xl/workbook.xml", WORKBOOK.as_bytes().to_vec());
    entries.insert("xl/_rels/workbook.xml.rels", WORKBOOK_RELS.as_bytes().to_vec());
    entries.insert("xl/styles.xml", STYLES.as_bytes().to_vec());
    entries.insert("xl/sharedStrings.xml", SHARED_STRINGS.as_bytes().to_vec());
    entries.insert("xl/worksheets/data.xml", DATA_SHEET.as_bytes().to_vec());
    entries.insert("xl/worksheets/sheet7.xml", HIDDEN_SHEET.as_bytes().to_vec());
    entries.insert("xl/worksheets/archive.xml", ARCHIVE_SHEET.as_bytes().to_vec());
    entries.insert("xl/worksheets/_rels/data.xml.rels", DATA_RELS.as_bytes().to_vec());
    entries.insert("xl/tables/table1.xml", TABLE.as_bytes().to_vec());
    entries.insert("xl/drawings/drawing1.xml", DRAWING.as_bytes().to_vec());
    entries.insert("xl/drawings/_rels/drawing1.xml.rels", DRAWING_RELS.as_bytes().to_vec());
    entries.insert("xl/media/image1.png", PNG.to_vec());
    entries.insert("xl/comments1.xml", COMMENTS.as_bytes().to_vec());
    entries.insert("xl/drawings/vmlDrawing1.vml", VML.as_bytes().to_vec());
    entries.insert("customXml/item1.xml", CUSTOM_XML.as_bytes().to_vec());
    entries.insert("customXml/itemProps1.xml", CUSTOM_XML_PROPERTIES.as_bytes().to_vec());
    entries.insert("customXml/_rels/item1.xml.rels", CUSTOM_XML_RELS.as_bytes().to_vec());
    entries
}

fn package_inventory(bytes: &[u8]) -> PackageInventory {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("open fixture ZIP");
    let mut payloads = BTreeMap::new();
    let mut entries = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("read fixture ZIP entry");
        let name = entry.name().to_owned();
        let mut payload = Vec::new();
        entry.read_to_end(&mut payload).expect("read fixture payload");
        assert_eq!(entry.crc32(), crc32(&payload), "ZIP CRC mismatch for {name}");
        entries.insert(
            name.clone(),
            EntryInventory { crc32: entry.crc32(), uncompressed_size: entry.size() },
        );
        payloads.insert(name, payload);
    }

    let relationships = payloads
        .iter()
        .filter(|(name, _)| name.ends_with(".rels"))
        .flat_map(|(name, payload)| parse_relationships(&relationship_source(name), payload))
        .collect::<Vec<_>>();
    let workbook_relationships = relationships
        .iter()
        .filter(|relationship| relationship.source == "xl/workbook.xml")
        .map(|relationship| (relationship.id.clone(), relationship.target.clone()))
        .collect::<BTreeMap<_, _>>();
    let (sheets, active_tab, defined_names) = parse_workbook(
        payloads.get("xl/workbook.xml").expect("workbook entry"),
        &workbook_relationships,
    );
    let styles = parse_styles(payloads.get("xl/styles.xml").expect("styles entry"));
    PackageInventory { entries, relationships, sheets, active_tab, defined_names, styles }
}

fn parse_workbook(
    xml: &[u8],
    relationships: &BTreeMap<String, String>,
) -> (Vec<SheetIdentity>, usize, Vec<DefinedName>) {
    let mut reader = Reader::from_reader(xml);
    let mut sheets = Vec::new();
    let mut active_tab = 0;
    let mut defined_names = Vec::new();
    let mut current_name = None::<DefinedName>;
    loop {
        match reader.read_event().expect("parse workbook XML") {
            Event::Start(event) | Event::Empty(event)
                if local_name(event.name().as_ref()) == b"workbookView" =>
            {
                active_tab = attribute(&event, b"activeTab")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
            }
            Event::Start(event) | Event::Empty(event)
                if local_name(event.name().as_ref()) == b"sheet" =>
            {
                let relationship_id = attribute(&event, b"id").expect("sheet relationship id");
                sheets.push(SheetIdentity {
                    index: sheets.len(),
                    name: attribute(&event, b"name").expect("sheet name"),
                    sheet_id: attribute(&event, b"sheetId").unwrap().parse().unwrap(),
                    target: relationships.get(&relationship_id).expect("sheet target").clone(),
                    relationship_id,
                    state: attribute(&event, b"state"),
                });
            }
            Event::Start(event) if local_name(event.name().as_ref()) == b"definedName" => {
                current_name = Some(DefinedName {
                    name: attribute(&event, b"name").expect("defined name"),
                    local_sheet_id: attribute(&event, b"localSheetId")
                        .and_then(|value| value.parse().ok()),
                    hidden: attribute(&event, b"hidden").as_deref() == Some("1"),
                    formula: String::new(),
                });
            }
            Event::Text(text) if current_name.is_some() => {
                current_name.as_mut().unwrap().formula.push_str(&text.decode().unwrap());
            }
            Event::GeneralRef(reference) if current_name.is_some() => {
                let decoded = reference.decode().unwrap();
                match decoded.as_ref() {
                    "lt" => current_name.as_mut().unwrap().formula.push('<'),
                    "gt" => current_name.as_mut().unwrap().formula.push('>'),
                    "amp" => current_name.as_mut().unwrap().formula.push('&'),
                    "quot" => current_name.as_mut().unwrap().formula.push('"'),
                    "apos" => current_name.as_mut().unwrap().formula.push('\''),
                    _ => {
                        if let Some(value) = reference.resolve_char_ref().unwrap() {
                            current_name.as_mut().unwrap().formula.push(value);
                        }
                    }
                }
            }
            Event::End(event) if local_name(event.name().as_ref()) == b"definedName" => {
                defined_names.push(current_name.take().expect("defined name state"));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    (sheets, active_tab, defined_names)
}

fn parse_relationships(source: &str, xml: &[u8]) -> Vec<RelationshipIdentity> {
    let mut reader = Reader::from_reader(xml);
    let mut relationships = Vec::new();
    loop {
        match reader.read_event().expect("parse relationships XML") {
            Event::Start(event) | Event::Empty(event)
                if local_name(event.name().as_ref()) == b"Relationship" =>
            {
                relationships.push(RelationshipIdentity {
                    source: source.to_owned(),
                    id: attribute(&event, b"Id").expect("relationship id"),
                    relationship_type: attribute(&event, b"Type").expect("relationship type"),
                    target: attribute(&event, b"Target").expect("relationship target"),
                    target_mode: attribute(&event, b"TargetMode"),
                });
            }
            Event::Eof => break,
            _ => {}
        }
    }
    relationships
}

fn parse_styles(xml: &[u8]) -> StyleCounts {
    let mut reader = Reader::from_reader(xml);
    let mut counts = StyleCounts::default();
    loop {
        match reader.read_event().expect("parse styles XML") {
            Event::Start(event) | Event::Empty(event) => {
                let count =
                    attribute(&event, b"count").and_then(|value| value.parse().ok()).unwrap_or(0);
                match local_name(event.name().as_ref()) {
                    b"numFmts" => counts.num_fmts = count,
                    b"fonts" => counts.fonts = count,
                    b"fills" => counts.fills = count,
                    b"borders" => counts.borders = count,
                    b"cellStyleXfs" => counts.cell_style_xfs = count,
                    b"cellXfs" => counts.cell_xfs = count,
                    b"cellStyles" => counts.cell_styles = count,
                    b"dxfs" => counts.dxfs = count,
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    counts
}

fn cell_style_index(worksheet_xml: &str, address: &str) -> usize {
    let mut reader = Reader::from_str(worksheet_xml);
    loop {
        match reader.read_event().expect("parse worksheet XML") {
            Event::Start(cell) | Event::Empty(cell) if local_name(cell.name().as_ref()) == b"c" => {
                if attribute(&cell, b"r").as_deref() == Some(address) {
                    return attribute(&cell, b"s")
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(0);
                }
            }
            Event::Eof => panic!("cell {address} not found"),
            _ => {}
        }
    }
}

fn cell_xf(styles_xml: &[u8], style_index: usize) -> CellXf {
    let mut reader = Reader::from_reader(styles_xml);
    let mut in_cell_xfs = false;
    let mut next_index = 0;
    let mut current = None::<CellXf>;
    loop {
        match reader.read_event().expect("parse styles XML") {
            Event::Start(event) if local_name(event.name().as_ref()) == b"cellXfs" => {
                in_cell_xfs = true;
            }
            Event::End(event) if local_name(event.name().as_ref()) == b"cellXfs" => {
                in_cell_xfs = false;
            }
            Event::Start(event) if in_cell_xfs && local_name(event.name().as_ref()) == b"xf" => {
                let index = next_index;
                next_index += 1;
                if index == style_index {
                    current = Some(cell_xf_attributes(&event));
                }
            }
            Event::Empty(event) if in_cell_xfs && local_name(event.name().as_ref()) == b"xf" => {
                let index = next_index;
                next_index += 1;
                if index == style_index {
                    return cell_xf_attributes(&event);
                }
            }
            Event::Start(event) | Event::Empty(event)
                if current.is_some() && local_name(event.name().as_ref()) == b"alignment" =>
            {
                let style = current.as_mut().unwrap();
                style.horizontal = attribute(&event, b"horizontal");
                style.vertical = attribute(&event, b"vertical");
                style.wrap_text = attribute(&event, b"wrapText").as_deref() == Some("1");
            }
            Event::End(event)
                if current.is_some() && local_name(event.name().as_ref()) == b"xf" =>
            {
                return current.take().unwrap();
            }
            Event::Eof => panic!("style {style_index} not found"),
            _ => {}
        }
    }
}

fn cell_xf_attributes(event: &BytesStart<'_>) -> CellXf {
    CellXf {
        num_fmt_id: attribute(event, b"numFmtId").and_then(|value| value.parse().ok()).unwrap_or(0),
        font_id: attribute(event, b"fontId").and_then(|value| value.parse().ok()).unwrap_or(0),
        fill_id: attribute(event, b"fillId").and_then(|value| value.parse().ok()).unwrap_or(0),
        border_id: attribute(event, b"borderId").and_then(|value| value.parse().ok()).unwrap_or(0),
        ..CellXf::default()
    }
}

fn cell_xf_signatures(styles_xml: &[u8]) -> Vec<CellXf> {
    let count = parse_styles(styles_xml).cell_xfs;
    (0..count).map(|index| cell_xf(styles_xml, index)).collect()
}

fn attribute(event: &BytesStart<'_>, key: &[u8]) -> Option<String> {
    event
        .attributes()
        .flatten()
        .find(|attribute| local_name(attribute.key.as_ref()) == key)
        .map(|attribute| String::from_utf8_lossy(attribute.value.as_ref()).into_owned())
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}

fn relationship_source(path: &str) -> String {
    if path == "_rels/.rels" {
        return "/".to_owned();
    }
    let (prefix, file) = path.rsplit_once("_rels/").expect("relationship path");
    format!("{}{}", prefix, file.strip_suffix(".rels").expect("relationship suffix"))
}

fn read_entry(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("open fixture ZIP");
    let mut entry = archive.by_name(name).expect("find fixture entry");
    let mut payload = Vec::new();
    entry.read_to_end(&mut payload).expect("read fixture entry");
    payload
}

fn entry_text(bytes: &[u8], name: &str) -> String {
    String::from_utf8(read_entry(bytes, name)).expect("fixture XML is UTF-8")
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0_u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn write_fixture(bytes: &[u8]) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().expect("create fixture file");
    file.write_all(bytes).expect("write fixture file");
    file
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="vml" ContentType="application/vnd.openxmlformats-officedocument.vmlDrawing"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/data.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/worksheets/sheet7.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/worksheets/archive.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/><Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/><Override PartName="/xl/tables/table1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml"/><Override PartName="/xl/drawings/drawing1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawing+xml"/><Override PartName="/xl/comments1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.comments+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/><Override PartName="/customXml/itemProps1.xml" ContentType="application/vnd.openxmlformats-officedocument.customXmlProperties+xml"/>
</Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/><Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml" Target="customXml/item1.xml"/></Relationships>"#;

const WORKBOOK: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><bookViews><workbookView activeTab="0" firstSheet="0"/></bookViews><sheets><sheet name="Data" sheetId="7" r:id="rId9"/><sheet name="HiddenCalc" sheetId="42" state="hidden" r:id="rId2"/><sheet name="Archive" sheetId="3" state="veryHidden" r:id="rId14"/></sheets><definedNames><definedName name="GlobalTotal">'Data'!$C$2</definedName><definedName name="_xlnm.Print_Area" localSheetId="0">'Data'!$A$1:$C$2</definedName><definedName name="HiddenFormula" localSheetId="1" hidden="1">'HiddenCalc'!$A$2</definedName></definedNames><calcPr calcId="191029" fullCalcOnLoad="1"/></workbook>"#;

const WORKBOOK_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/data.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet7.xml"/><Relationship Id="rId14" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/archive.xml"/><Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId7" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/><Relationship Id="rId21" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml" Target="../customXml/item1.xml"/></Relationships>"#;

const SHARED_STRINGS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="4" uniqueCount="4"><si><t>Name</t></si><si><t>Amount</t></si><si><t>Alpha</t></si><si><t>Hidden label</t></si></sst>"#;

const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><numFmts count="1"><numFmt numFmtId="164" formatCode="yyyy-mm-dd"/></numFmts><fonts count="2"><font><sz val="11"/><name val="Calibri"/></font><font><b/><sz val="11"/><color rgb="FFFFFFFF"/></font></fonts><fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FF4472C4"/></patternFill></fill></fills><borders count="2"><border/><border><left style="thin"/><right style="thin"/><top style="thin"/><bottom style="thin"/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="3"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/><xf numFmtId="0" fontId="1" fillId="2" borderId="1" applyFont="1" applyFill="1" applyBorder="1"/><xf numFmtId="164" fontId="0" fillId="0" borderId="0" applyNumberFormat="1"/></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles><dxfs count="0"/></styleSheet>"#;

const DATA_SHEET: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><dimension ref="A1:C2"/><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row><row r="2"><c r="A2" t="s" s="1"><v>2</v></c><c r="B2" s="2"><v>41.5</v></c><c r="C2"><f>B2*2</f><v>83</v></c></row></sheetData><drawing r:id="rId11"/><legacyDrawing r:id="rId20"/><tableParts count="1"><tablePart r:id="rId4"/></tableParts></worksheet>"#;

const HIDDEN_SHEET: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:A2"/><sheetData><row r="1"><c r="A1" t="s"><v>3</v></c></row><row r="2"><c r="A2"><f>'Data'!C2</f><v>83</v></c></row></sheetData></worksheet>"#;

const ARCHIVE_SHEET: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:A1"/><sheetData><row r="1"><c r="A1"><f>'Data'!B2+1</f><v>42.5</v></c></row></sheetData></worksheet>"#;

const DATA_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/table" Target="../tables/table1.xml"/><Relationship Id="rId11" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="../comments1.xml"/><Relationship Id="rId20" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/vmlDrawing" Target="../drawings/vmlDrawing1.vml"/></Relationships>"#;

const TABLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" id="5" name="DataTable" displayName="DataTable" ref="A1:B2" totalsRowShown="0"><autoFilter ref="A1:B2"/><tableColumns count="2"><tableColumn id="1" name="Name"/><tableColumn id="2" name="Amount"/></tableColumns></table>"#;

const DRAWING: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><xdr:oneCellAnchor><xdr:from><xdr:col>0</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:ext cx="9525" cy="9525"/><xdr:pic><xdr:blipFill><a:blip r:embed="rId8"/></xdr:blipFill></xdr:pic><xdr:clientData/></xdr:oneCellAnchor></xdr:wsDr>"#;

const DRAWING_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId8" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/></Relationships>"#;

const COMMENTS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<comments xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><authors><author>Task0</author></authors><commentList><comment ref="A2" authorId="0"><text><t>preserve me</t></text></comment></commentList></comments>"#;

const VML: &str = r#"<xml xmlns:v="urn:schemas-microsoft-com:vml" xmlns:x="urn:schemas-microsoft-com:office:excel"><v:shape id="_x0000_s1025"><x:ClientData ObjectType="Note"><x:Row>1</x:Row><x:Column>0</x:Column></x:ClientData></v:shape></xml>"#;

const CUSTOM_XML: &str =
    r#"<fixture xmlns="urn:miniexcel:insert:test"><value>preserve me</value></fixture>"#;
const CUSTOM_XML_PROPERTIES: &str = r#"<ds:datastoreItem ds:itemID="{11111111-2222-3333-4444-555555555555}" xmlns:ds="http://schemas.openxmlformats.org/officeDocument/2006/customXml"><ds:schemaRefs><ds:schemaRef ds:uri="urn:miniexcel:insert:test"/></ds:schemaRefs></ds:datastoreItem>"#;
const CUSTOM_XML_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXmlProps" Target="itemProps1.xml"/></Relationships>"#;
const APP_PROPERTIES: &str = r#"<?xml version="1.0" encoding="UTF-8"?><Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>MiniExcel Insert Task 0</Application></Properties>"#;
const CORE_PROPERTIES: &str = r#"<?xml version="1.0" encoding="UTF-8"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties"><cp:keywords>insert characterization</cp:keywords></cp:coreProperties>"#;
