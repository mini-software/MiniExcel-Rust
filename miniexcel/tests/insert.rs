use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Write};
use std::rc::Rc;

use miniexcel::{
    CellValue, DynamicRow, ExistingSheetPolicy, HeaderMode, InsertOptions, MiniExcel, ReadOptions,
    SheetVisibility, TargetRelationshipPolicy, WriteOptions,
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

#[derive(Debug, Eq, PartialEq)]
struct PackageInventory {
    entries: BTreeMap<String, EntryInventory>,
    relationships: Vec<RelationshipIdentity>,
    sheets: Vec<SheetIdentity>,
    active_tab: usize,
    defined_names: Vec<DefinedName>,
    styles: StyleCounts,
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
            .with_existing_sheet_policy(ExistingSheetPolicy::Replace),
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

fn dynamic_insert_row(name: &str, version: i64) -> DynamicRow {
    let mut row = DynamicRow::new();
    row.insert("Name".to_owned(), CellValue::String(name.to_owned()));
    row.insert("Version".to_owned(), CellValue::Int(version));
    row
}

fn rows_for_macro() -> [DynamicRow; 1] {
    [dynamic_insert_row("MiniExcel", 1)]
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
