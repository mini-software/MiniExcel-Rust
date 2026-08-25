use std::io::{Cursor, Read, Seek, SeekFrom, Write};

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use miniexcel::{
    CellValue, CsvConfiguration, CsvEncoding, CsvNewline, CsvReadOptions, CsvWriteOptions,
    DynamicRow, HeaderMode, MiniExcel,
};
use serde::{Deserialize, Serialize};

const CSV_FIXTURE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/csv");

#[derive(Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
struct CsvRecord {
    name: String,
    value: i64,
    note: Option<String>,
}

#[test]
fn writes_miniexcel_defaults_and_roundtrips_dynamic_and_typed_rows() {
    assert_eq!(CsvNewline::default(), CsvNewline::CrLf);
    let rows = [row("Hello World", 1, "line 1\nline \"2\""), row("Rust", 2, "")];
    let bytes = MiniExcel::save_csv_bytes(&rows, &CsvWriteOptions::new()).unwrap();
    assert!(bytes.starts_with(b"\xEF\xBB\xBF"));
    let text = std::str::from_utf8(&bytes[3..]).unwrap();
    assert_eq!(
        text,
        "Name,Value,Note\r\n\"Hello World\",1,\"line 1\r\nline \"\"2\"\"\"\r\nRust,2,\r\n"
    );

    let dynamic = MiniExcel::query_csv_bytes(
        &bytes,
        &CsvReadOptions::new().with_header_mode(HeaderMode::FirstRow),
    )
    .unwrap();
    assert_eq!(dynamic.len(), 2);
    assert_eq!(dynamic[0]["Name"], CellValue::String("Hello World".to_owned()));
    assert_eq!(dynamic[0]["Note"], CellValue::String("line 1\r\nline \"2\"".to_owned()));
    assert_eq!(dynamic[1]["Note"], CellValue::String(String::new()));

    let typed_bytes = MiniExcel::save_csv_serialized_bytes(
        &[CsvRecord { name: "Typed".to_owned(), value: 3, note: Some(String::new()) }],
        &CsvWriteOptions::new(),
    )
    .unwrap();
    let typed =
        MiniExcel::query_csv_as_bytes::<CsvRecord>(&typed_bytes, &CsvReadOptions::new()).unwrap();
    assert_eq!(typed[0].name, "Typed");
    assert_eq!(typed[0].value, 3);
}

#[test]
fn dynamic_defaults_to_column_letters_and_null_mode_is_explicit() {
    let bytes = b"Name,Value\r\nAlpha,\r\n";
    let rows = MiniExcel::query_csv_bytes(bytes, &CsvReadOptions::new()).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["A"], CellValue::String("Name".to_owned()));
    assert_eq!(rows[1]["B"], CellValue::String(String::new()));

    let options = CsvReadOptions::new()
        .with_header_mode(HeaderMode::FirstRow)
        .with_configuration(CsvConfiguration::new().with_read_empty_as_null(true));
    let rows = MiniExcel::query_csv_bytes(bytes, &options).unwrap();
    assert_eq!(rows[0]["Value"], CellValue::Empty);
}

#[test]
fn supports_custom_delimiter_newline_encoding_and_quote_policy() {
    let rows = [row("A B", 1, "é")];
    let options = CsvWriteOptions::new().with_configuration(
        CsvConfiguration::new()
            .with_delimiter(b';')
            .with_newline(CsvNewline::Lf)
            .with_encoding(CsvEncoding::Utf16Le)
            .with_always_quote(true),
    );
    let bytes = MiniExcel::save_csv_bytes(&rows, &options).unwrap();
    assert!(bytes.starts_with(b"\xFF\xFE"));
    let read_options =
        CsvReadOptions::new().with_header_mode(HeaderMode::FirstRow).with_configuration(
            CsvConfiguration::new()
                .with_delimiter(b';')
                .with_newline(CsvNewline::Lf)
                .with_encoding(CsvEncoding::Utf16Le),
        );
    let decoded = MiniExcel::query_csv_bytes(&bytes, &read_options).unwrap();
    assert_eq!(decoded[0]["Name"], CellValue::String("A B".to_owned()));
    assert_eq!(decoded[0]["Note"], CellValue::String("é".to_owned()));

    let windows = CsvWriteOptions::new().with_configuration(
        CsvConfiguration::new().with_encoding(CsvEncoding::Windows1252).with_write_bom(false),
    );
    assert!(MiniExcel::save_csv_bytes(&[row("漢字", 1, "")], &windows).is_err());
}

#[test]
fn formats_boolean_and_temporal_values_invariantly() {
    let mut values = DynamicRow::new();
    values.insert("Boolean".to_owned(), CellValue::Bool(true));
    values
        .insert("Date".to_owned(), CellValue::Date(NaiveDate::from_ymd_opt(2026, 8, 25).unwrap()));
    values.insert("Time".to_owned(), CellValue::Time(NaiveTime::from_hms_opt(9, 7, 5).unwrap()));
    values.insert(
        "DateTime".to_owned(),
        CellValue::DateTime(
            NaiveDateTime::parse_from_str("2026-08-25 09:07:05", "%Y-%m-%d %H:%M:%S").unwrap(),
        ),
    );

    let bytes = MiniExcel::save_csv_bytes(&[values], &CsvWriteOptions::new()).unwrap();
    assert_eq!(
        std::str::from_utf8(&bytes[3..]).unwrap(),
        "Boolean,Date,Time,DateTime\r\nTrue,2026-08-25,09:07:05,\"2026-08-25 09:07:05\"\r\n"
    );
}

#[test]
fn append_and_borrowed_io_follow_header_and_leave_open_rules() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("append.csv");
    let options = CsvWriteOptions::new();
    MiniExcel::append_csv(&path, &[row("First", 1, "")], &options).unwrap();
    MiniExcel::append_csv(&path, &[row("Second", 2, "")], &options).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes.iter().filter(|byte| **byte == 0xEF).count(), 1);
    let text = std::str::from_utf8(&bytes[3..]).unwrap();
    assert_eq!(text.matches("Name,Value,Note").count(), 1);

    let mut output = Cursor::new(Vec::new());
    MiniExcel::save_csv_to_writer(&mut output, &[row("Borrowed", 3, "")], &options).unwrap();
    output.write_all(&[]).unwrap();
    output.seek(SeekFrom::Start(0)).unwrap();
    let mut reader = output;
    let values = MiniExcel::query_csv_from_reader(
        &mut reader,
        &CsvReadOptions::new().with_header_mode(HeaderMode::FirstRow),
    )
    .unwrap()
    .collect::<miniexcel::Result<Vec<_>>>()
    .unwrap();
    assert_eq!(values[0]["Name"], CellValue::String("Borrowed".to_owned()));
    reader.read_to_end(&mut Vec::new()).unwrap();
}

#[test]
fn discovers_header_only_columns_and_supports_all_append_variants() {
    let options = CsvReadOptions::new().with_header_mode(HeaderMode::FirstRow);
    assert_eq!(
        MiniExcel::get_csv_columns_from_bytes(b"Name,Value,Note\r\n", &options).unwrap(),
        ["Name", "Value", "Note"]
    );
    let mut input = Cursor::new(b"Name,Value,Note\r\n".to_vec());
    assert_eq!(
        MiniExcel::get_csv_columns_from_reader(&mut input, &options).unwrap(),
        ["Name", "Value", "Note"]
    );

    let schema = ["Name".to_owned(), "Value".to_owned(), "Note".to_owned()];
    let bytes = MiniExcel::save_csv_with_schema_bytes(
        &schema,
        &[row("Schema", 4, "")],
        &CsvWriteOptions::new(),
    )
    .unwrap();
    assert!(std::str::from_utf8(&bytes[3..]).unwrap().contains("Schema,4,"));

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("serialized.csv");
    std::fs::write(&path, []).unwrap();
    MiniExcel::append_csv_serialized(
        &path,
        &[CsvRecord { name: "First".to_owned(), value: 5, note: None }],
        &CsvWriteOptions::new(),
    )
    .unwrap();
    MiniExcel::append_csv_serialized(
        &path,
        &[CsvRecord { name: "Second".to_owned(), value: 6, note: Some(String::new()) }],
        &CsvWriteOptions::new(),
    )
    .unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.matches("Name,Value,Note").count(), 1);
    assert!(text.contains("First,5,"));
    assert!(text.contains("Second,6,"));

    let mut output = Cursor::new(Vec::new());
    MiniExcel::append_csv_with_schema_to_writer(
        &mut output,
        &schema,
        &[row("Dynamic", 7, "")],
        &CsvWriteOptions::new(),
    )
    .unwrap();
    MiniExcel::append_csv_serialized_to_writer(
        &mut output,
        &[CsvRecord { name: "Typed".to_owned(), value: 8, note: None }],
        &CsvWriteOptions::new(),
    )
    .unwrap();
    let text = String::from_utf8(output.into_inner()).unwrap();
    assert_eq!(text.matches("Name,Value,Note").count(), 1);
    assert!(text.contains("Dynamic,7,"));
    assert!(text.contains("Typed,8,"));
}

#[test]
fn supports_gbk_and_cr_newlines_and_rejects_unclosed_uneven_records() {
    let configuration = CsvConfiguration::new()
        .with_encoding(CsvEncoding::Gbk)
        .with_newline(CsvNewline::Cr)
        .with_write_bom(false);
    let write_options = CsvWriteOptions::new().with_configuration(configuration.clone());
    let bytes = MiniExcel::save_csv_bytes(&[row("中文", 9, "甲\n乙")], &write_options).unwrap();
    assert!(!bytes.starts_with(b"\xEF\xBB\xBF"));

    let read_options = CsvReadOptions::new()
        .with_header_mode(HeaderMode::FirstRow)
        .with_configuration(configuration);
    let rows = MiniExcel::query_csv_bytes(&bytes, &read_options).unwrap();
    assert_eq!(rows[0]["Name"], CellValue::String("中文".to_owned()));
    assert_eq!(rows[0]["Note"], CellValue::String("甲\r乙".to_owned()));

    let error =
        MiniExcel::query_csv_bytes(b"A,B,C\n\"r1a: no end quote,r1b,r1c", &CsvReadOptions::new())
            .unwrap_err();
    assert!(error.to_string().contains("record 2"));
}

#[test]
fn matches_pinned_dotnet_header_and_gb2312_fixtures() {
    let header_path = format!("{CSV_FIXTURE_ROOT}/TestHeader.csv");
    let options = CsvReadOptions::new().with_header_mode(HeaderMode::FirstRow);
    assert_eq!(MiniExcel::get_csv_columns(&header_path, &options).unwrap(), ["Column1", "Column2"]);
    let rows = MiniExcel::query_csv_with_options(&header_path, &options)
        .unwrap()
        .collect::<miniexcel::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["Column1"], CellValue::String("A1".to_owned()));
    assert_eq!(rows[1]["Column2"], CellValue::String("B2".to_owned()));

    let gb2312_path = format!("{CSV_FIXTURE_ROOT}/gb2312_Encoding_Read_Test.csv");
    let options = CsvReadOptions::new()
        .with_header_mode(HeaderMode::FirstRow)
        .with_configuration(CsvConfiguration::new().with_encoding(CsvEncoding::Gbk));
    let rows = MiniExcel::query_csv_with_options(gb2312_path, &options)
        .unwrap()
        .collect::<miniexcel::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(rows[0]["栏位1"], CellValue::String("世界你好".to_owned()));
}

#[test]
fn rejects_uneven_records_duplicate_schema_and_existing_paths() {
    let error = MiniExcel::query_csv_bytes(
        b"A,B\r\n1\r\n",
        &CsvReadOptions::new().with_header_mode(HeaderMode::FirstRow),
    )
    .unwrap_err();
    assert!(error.to_string().contains("record 2"));

    let error = MiniExcel::query_csv_bytes(
        b"Name,Name\r\nFirst,Second\r\n",
        &CsvReadOptions::new().with_header_mode(HeaderMode::FirstRow),
    )
    .unwrap_err();
    assert!(error.to_string().contains("column name 'Name' appears more than once"));

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("data.csv");
    std::fs::write(&path, b"existing").unwrap();
    assert!(MiniExcel::save_csv(&path, &[row("A", 1, "")], &CsvWriteOptions::new()).is_err());

    let output = directory.path().join("duplicate.csv");
    assert!(
        MiniExcel::save_csv_with_schema(
            &output,
            &["Name".to_owned(), "Name".to_owned()],
            &[],
            &CsvWriteOptions::new(),
        )
        .is_err()
    );
    assert!(!output.exists());

    let invalid = directory.path().join("invalid.csv");
    let options =
        CsvWriteOptions::new().with_configuration(CsvConfiguration::new().with_delimiter(b'\n'));
    assert!(MiniExcel::save_csv(&invalid, &[row("A", 1, "")], &options).is_err());
    assert!(!invalid.exists());
}

fn row(name: &str, value: i64, note: &str) -> DynamicRow {
    let mut row = DynamicRow::new();
    row.insert("Name".to_owned(), CellValue::String(name.to_owned()));
    row.insert("Value".to_owned(), CellValue::Int(value));
    row.insert("Note".to_owned(), CellValue::String(note.to_owned()));
    row
}
