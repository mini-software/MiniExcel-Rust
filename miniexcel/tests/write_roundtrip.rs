use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
use miniexcel::{CellValue, DynamicRow, HeaderMode, MiniExcel, ReadOptions, WriteOptions};
use serde::{Deserialize, Serialize};

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
    let options = WriteOptions::new().with_column_format("ReleasedOn", "yyyy-mm-dd");

    let row_counts = MiniExcel::save_as_serialized_sheets(
        &path,
        [("Stable", stable.as_slice()), ("Preview", preview.as_slice())],
        &options,
    )
    .expect("write serialized worksheets");

    assert_eq!(row_counts, [1, 1]);
    assert_eq!(MiniExcel::get_sheet_names(&path).unwrap(), ["Stable", "Preview"]);
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
