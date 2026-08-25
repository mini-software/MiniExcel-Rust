use std::io::{Cursor, Write};

use miniexcel::{CellValue, MiniExcel};
use serde::Deserialize;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

mod common;

#[derive(Debug, Deserialize, PartialEq)]
struct TableRecord {
    #[serde(rename = "MetaName")]
    name: Option<String>,
    #[serde(rename = "MetaValue")]
    value: Option<i64>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct TrimmedRecord {
    #[serde(rename = "MetaName")]
    name: Option<String>,
    #[serde(rename = "MetaValue")]
    value: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct DotNetTableRecord {
    #[serde(rename = "Col1")]
    col1: String,
    #[serde(rename = "Col2")]
    col2: i64,
    #[serde(rename = "Col3", deserialize_with = "miniexcel::serde_helpers::deserialize_date")]
    col3: chrono::NaiveDate,
}

#[test]
fn matches_dotnet_query_table_reference_fixture() {
    let path = common::fixture("TestQueryTable.xlsx");
    let table1 = MiniExcel::query_table(&path, "table1", None)
        .unwrap()
        .collect::<miniexcel::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(table1.len(), 3);
    assert_eq!(table1[0]["Col1"], CellValue::String("aaa".to_owned()));
    assert_eq!(table1[0]["Col2"], CellValue::Int(123));

    let typed = MiniExcel::query_table_as::<DotNetTableRecord>(&path, "Table1", None)
        .unwrap()
        .collect::<miniexcel::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(typed.len(), 3);
    assert_eq!(typed[2].col1, "ccc");
    assert_eq!(typed[2].col2, 789);
    assert_eq!(typed[2].col3, chrono::NaiveDate::from_ymd_opt(2026, 5, 19).unwrap());

    let table2 = MiniExcel::query_table(&path, "Table2", Some("Sheet1"))
        .unwrap()
        .collect::<miniexcel::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(table2.len(), 2);
    assert_eq!(table2[0]["Prop1"], CellValue::String("test".to_owned()));
    assert_eq!(table2[0]["Prop2"], CellValue::Int(11));
    assert_eq!(table2[0]["Prop3"], CellValue::String("aaa".to_owned()));

    assert!(
        MiniExcel::query_table(&path, "CustomTable", Some("CustomSheet"))
            .unwrap()
            .next()
            .transpose()
            .unwrap()
            .is_some()
    );

    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(MiniExcel::query_table_bytes(&bytes, "Table1", None).unwrap(), table1);
    let mut reader = Cursor::new(bytes);
    let mut borrowed = Vec::new();
    MiniExcel::visit_table_rows_as_from_reader::<DotNetTableRecord, _, _>(
        &mut reader,
        "Table1",
        None,
        |_, row| {
            borrowed.push(row.clone());
            Ok(true)
        },
    )
    .unwrap();
    assert_eq!(borrowed, typed);
}

#[test]
fn queries_named_table_with_metadata_headers_and_bounds() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tables.xlsx");
    let bytes = table_fixture("DataTable", "B2:C5", None, "MetaName", "MetaValue");
    std::fs::write(&path, &bytes).unwrap();

    let rows = MiniExcel::query_table(&path, "datatable", None)
        .unwrap()
        .collect::<miniexcel::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].keys().map(String::as_str).collect::<Vec<_>>(), ["MetaName", "MetaValue"]);
    assert_eq!(rows[0]["MetaName"], CellValue::String("Alpha".to_owned()));
    assert_eq!(rows[0]["MetaValue"], CellValue::Int(1));
    assert!(rows[1].values().all(CellValue::is_empty));
    assert_eq!(rows[2]["MetaName"], CellValue::String("Total".to_owned()));
    assert_eq!(rows[2]["MetaValue"], CellValue::Int(3));

    let typed = MiniExcel::query_table_as::<TableRecord>(&path, "DataTable", Some("Data"))
        .unwrap()
        .collect::<miniexcel::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(typed[0], TableRecord { name: Some("Alpha".to_owned()), value: Some(1) });
    assert_eq!(typed[1], TableRecord { name: None, value: None });

    assert_eq!(MiniExcel::query_table_bytes(&bytes, "DataTable", None).unwrap(), rows);

    let mut reader = Cursor::new(bytes);
    let mut borrowed = Vec::new();
    let summary = MiniExcel::visit_table_rows_from_reader(
        &mut reader,
        "DataTable",
        Some("Data"),
        |excel_row, row| {
            borrowed.push((excel_row, row.clone()));
            Ok(true)
        },
    )
    .unwrap();
    assert_eq!(summary.sheet_name(), "Data");
    assert_eq!(summary.columns(), ["MetaName", "MetaValue"]);
    assert_eq!(summary.visited_rows(), 3);
    assert_eq!(borrowed.iter().map(|(row, _)| *row).collect::<Vec<_>>(), [3, 4, 5]);
}

#[test]
fn headerless_table_includes_first_referenced_row() {
    let bytes = table_fixture("RawTable", "B2:C3", Some(0), "First", "Second");
    let rows = MiniExcel::query_table_bytes(&bytes, "RawTable", None).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["First"], CellValue::String("PhysicalName".to_owned()));
    assert_eq!(rows[0]["Second"], CellValue::String("PhysicalValue".to_owned()));
}

#[test]
fn missing_and_malformed_tables_return_specific_errors() {
    let bytes = table_fixture("DataTable", "B2:C5", None, "Name", "Value");
    let missing = MiniExcel::query_table_bytes(&bytes, "Missing", None).unwrap_err();
    assert!(missing.to_string().contains("table 'Missing' was not found"));
    let display_name =
        MiniExcel::query_table_bytes(&bytes, "DifferentDisplayName", None).unwrap_err();
    assert!(display_name.to_string().contains("table 'DifferentDisplayName' was not found"));

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("table.xlsx");
    std::fs::write(&path, &bytes).unwrap();
    let missing_sheet = MiniExcel::query_table(&path, "DataTable", Some("MissingSheet"))
        .err()
        .expect("missing sheet should fail");
    assert!(missing_sheet.to_string().contains("worksheet 'MissingSheet' was not found"));

    let malformed = table_fixture("DataTable", "B2", None, "Name", "Value");
    let error = MiniExcel::query_table_bytes(&malformed, "DataTable", None).unwrap_err();
    assert!(error.to_string().contains("table range must contain two cells"));
}

#[test]
fn table_metadata_handles_trim_fallback_and_header_only_ranges() {
    let trimmed = table_fixture("Trimmed", "B2:C3", None, " MetaName ", " MetaValue ");
    let typed =
        MiniExcel::query_table_as::<TrimmedRecord>(write_fixture(&trimmed).path(), "Trimmed", None)
            .unwrap()
            .collect::<miniexcel::Result<Vec<_>>>()
            .unwrap();
    assert_eq!(typed[0], TrimmedRecord { name: Some("Alpha".to_owned()), value: Some(1) });

    let missing_column = table_fixture("Fallback", "B2:C3", None, "OnlyName", "");
    let rows = MiniExcel::query_table_bytes(&missing_column, "Fallback", None).unwrap();
    assert_eq!(rows[0].keys().map(String::as_str).collect::<Vec<_>>(), ["OnlyName", ""]);

    let header_only = table_fixture("Headers", "B2:C2", None, "Name", "Value");
    let mut reader = Cursor::new(header_only);
    let summary = MiniExcel::visit_table_rows_from_reader(&mut reader, "Headers", None, |_, _| {
        panic!("header-only table emitted a data row")
    })
    .unwrap();
    assert_eq!(summary.columns(), ["Name", "Value"]);
    assert_eq!(summary.visited_rows(), 0);
}

fn write_fixture(bytes: &[u8]) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(bytes).unwrap();
    file
}

fn table_fixture(
    table_name: &str,
    reference: &str,
    header_row_count: Option<u32>,
    first_header: &str,
    second_header: &str,
) -> Vec<u8> {
    let header_attribute =
        header_row_count.map(|count| format!(" headerRowCount=\"{count}\"")).unwrap_or_default();
    let table = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" id="1" name="{table_name}" displayName="DifferentDisplayName" ref="{reference}"{header_attribute}><tableColumns count="2"><tableColumn id="1" name="{first_header}"/><tableColumn id="2" name="{second_header}"/></tableColumns></table>"#
    );
    let entries = [
        (
            "[Content_Types].xml",
            br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/custom.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/tables/nonstandard.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml"/></Types>"#.as_slice(),
        ),
        (
            "_rels/.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.as_slice(),
        ),
        (
            "xl/workbook.xml",
            br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#.as_slice(),
        ),
        (
            "xl/_rels/workbook.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/custom.xml"/></Relationships>"#.as_slice(),
        ),
        (
            "xl/worksheets/custom.xml",
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><dimension ref="A1:D6"/><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Outside</t></is></c></row><row r="2"><c r="B2" t="inlineStr"><is><t>PhysicalName</t></is></c><c r="C2" t="inlineStr"><is><t>PhysicalValue</t></is></c></row><row r="3"><c r="B3" t="inlineStr"><is><t>Alpha</t></is></c><c r="C3"><v>1</v></c></row><row r="4"/><row r="5"><c r="B5" t="inlineStr"><is><t>Total</t></is></c><c r="C5"><v>3</v></c></row><row r="6"><c r="D6"><v>999</v></c></row></sheetData><tableParts count="1"><tablePart r:id="tableRel"/></tableParts></worksheet>"#.as_slice(),
        ),
        (
            "xl/worksheets/_rels/custom.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="tableRel" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/table" Target="../tables/nonstandard.xml"/></Relationships>"#.as_slice(),
        ),
        ("xl/tables/nonstandard.xml", table.as_bytes()),
    ];

    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, payload) in entries {
        writer.start_file(name, options).unwrap();
        writer.write_all(payload).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
