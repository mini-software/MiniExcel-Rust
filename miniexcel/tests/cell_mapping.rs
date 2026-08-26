use std::io::{Read, Seek, SeekFrom};

use chrono::NaiveDate;
use miniexcel::{CellMap, MiniExcel, ReadOptions};
use rust_xlsxwriter::{Format, Formula, Workbook};
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
struct Profile {
    display_name: String,
    age: u32,
    active: bool,
    #[serde(deserialize_with = "miniexcel::serde_helpers::deserialize_date")]
    joined: NaiveDate,
    nickname: Option<String>,
    total: f64,
    far_value: String,
}

#[test]
fn maps_noncontiguous_cells_consistently_across_input_shapes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("mapped.xlsx");
    write_mapping_fixture(&path);
    let mapping = profile_mapping();
    let expected = Profile {
        display_name: "Ada".to_owned(),
        age: 36,
        active: true,
        joined: NaiveDate::from_ymd_opt(2026, 8, 26).unwrap(),
        nickname: None,
        total: 42.5,
        far_value: "Far".to_owned(),
    };

    assert_eq!(MiniExcel::read_mapped_as::<Profile>(&path, &mapping).unwrap(), expected);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(MiniExcel::read_mapped_as_bytes::<Profile>(&bytes, &mapping).unwrap(), expected);
    let mut reader = std::io::Cursor::new(bytes);
    assert_eq!(
        MiniExcel::read_mapped_as_from_reader::<Profile, _>(&mut reader, &mapping).unwrap(),
        expected
    );
    reader.seek(SeekFrom::Start(0)).unwrap();
    let mut signature = [0_u8; 2];
    reader.read_exact(&mut signature).unwrap();
    assert_eq!(&signature, b"PK");
}

#[test]
fn mapped_formula_uses_cached_value_while_structured_read_keeps_provenance() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("mapped.xlsx");
    write_mapping_fixture(&path);
    let mapped = MiniExcel::read_mapped_as::<Profile>(&path, &profile_mapping()).unwrap();
    assert_eq!(mapped.total, 42.5);

    let mut formula = None;
    MiniExcel::visit_structured_rows_from_reader(
        &mut std::fs::File::open(&path).unwrap(),
        &ReadOptions::new().with_sheet_name("Profile"),
        |row| {
            for cell in row.cells() {
                if cell.address() == "H11" {
                    formula = cell.formula().map(str::to_owned);
                }
            }
            Ok(true)
        },
    )
    .unwrap();
    assert_eq!(formula.as_deref(), Some("40+2.5"));
}

#[test]
fn validates_mapping_before_io_and_reports_mapping_context() {
    let missing = std::path::Path::new("missing-mapped-workbook.xlsx");
    let error = MiniExcel::read_mapped_as::<Profile>(missing, &CellMap::new()).unwrap_err();
    assert!(error.to_string().contains("at least one cell"));

    let duplicate_field = CellMap::new()
        .with_cell("name", "A1".parse().unwrap())
        .with_cell("name", "B2".parse().unwrap());
    let error = MiniExcel::read_mapped_as::<Profile>(missing, &duplicate_field).unwrap_err();
    assert!(error.to_string().contains("field 'name'"));

    let duplicate_cell = CellMap::new()
        .with_cell("name", "A1".parse().unwrap())
        .with_cell("age", "$a$1".parse().unwrap());
    let error = MiniExcel::read_mapped_as::<Profile>(missing, &duplicate_cell).unwrap_err();
    assert!(error.to_string().contains("cell 'A1'"));

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("mapped.xlsx");
    write_mapping_fixture(&path);
    let required_missing = CellMap::new()
        .with_sheet_name("Profile")
        .with_cell("display_name", "B3".parse().unwrap())
        .with_cell("age", "A999".parse().unwrap());
    let error = MiniExcel::read_mapped_as::<RequiredFields>(&path, &required_missing).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("Profile"));
    assert!(message.contains("age=A999"));

    let missing_sheet = profile_mapping().with_sheet_name("Missing");
    let error = MiniExcel::read_mapped_as::<Profile>(&path, &missing_sheet).unwrap_err();
    assert!(error.to_string().contains("Missing"));
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RequiredFields {
    display_name: String,
    age: u32,
}

fn profile_mapping() -> CellMap {
    CellMap::new()
        .with_sheet_name("Profile")
        .with_cell("display_name", "b3".parse().unwrap())
        .with_cell("age", "$F$2".parse().unwrap())
        .with_cell("active", "C7".parse().unwrap())
        .with_cell("joined", "D9".parse().unwrap())
        .with_cell("nickname", "J20".parse().unwrap())
        .with_cell("total", "H11".parse().unwrap())
        .with_cell("far_value", "XFD1000".parse().unwrap())
}

fn write_mapping_fixture(path: &std::path::Path) {
    let mut workbook = Workbook::new();
    workbook.add_worksheet().set_name("Ignore").unwrap();
    let sheet = workbook.add_worksheet();
    sheet.set_name("Profile").unwrap();
    sheet.write_string(2, 1, "Ada").unwrap();
    sheet.write_number(1, 5, 36).unwrap();
    sheet.write_boolean(6, 2, true).unwrap();
    sheet
        .write_datetime_with_format(
            8,
            3,
            NaiveDate::from_ymd_opt(2026, 8, 26).unwrap(),
            &Format::new().set_num_format("yyyy-mm-dd"),
        )
        .unwrap();
    sheet.write_formula(10, 7, Formula::new("=40+2.5").set_result("42.5")).unwrap();
    sheet.write_string(999, 16_383, "Far").unwrap();
    workbook.save(path).unwrap();
}
