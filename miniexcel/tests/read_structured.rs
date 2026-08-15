mod common;

use miniexcel::{CellValue, HeaderMode, MiniExcel, ReadOptions};

#[test]
fn structured_query_preserves_formula_value_address_and_format() {
    let rows = MiniExcel::query_structured(common::fixture("TestIssue157.xlsx"))
        .expect("create structured query")
        .collect::<miniexcel::Result<Vec<_>>>()
        .expect("stream structured rows");

    let row = rows.iter().find(|row| row.row_index() == 2).expect("Excel row 2");
    let cell = row.cells().iter().find(|cell| cell.address() == "D2").expect("cell D2");

    assert_eq!(row.sheet_name(), "Sheet1");
    assert_eq!(cell.row_index(), 2);
    assert_eq!(cell.column_index(), 4);
    assert_eq!(cell.value(), &CellValue::Bool(false));
    assert_eq!(cell.formula(), Some("FALSE()"));
    assert_eq!(cell.style_id(), 2);
    assert_eq!(cell.number_format(), Some("General"));
}

#[test]
fn structured_query_preserves_builtin_date_format() {
    let rows = MiniExcel::query_structured(common::fixture("TestTypeMapping.xlsx"))
        .expect("create structured query")
        .collect::<miniexcel::Result<Vec<_>>>()
        .expect("stream structured rows");

    let row = rows.iter().find(|row| row.row_index() == 2).expect("Excel row 2");
    let cell = row.cells().iter().find(|cell| cell.address() == "C2").expect("cell C2");

    assert_eq!(cell.style_id(), 5);
    assert_eq!(cell.number_format(), Some("mm-dd-yy"));
    assert!(cell.formula().is_none());
}

#[test]
fn structured_query_keeps_header_row_and_honors_ranges() {
    let options = ReadOptions::new()
        .with_header_mode(HeaderMode::FirstRow)
        .with_start_cell("B2".parse().expect("valid start cell"))
        .with_end_cell("D3".parse().expect("valid end cell"));
    let rows = MiniExcel::query_structured_with_options(
        common::fixture("TestDynamicQueryBasic.xlsx"),
        &options,
    )
    .expect("create ranged structured query")
    .collect::<miniexcel::Result<Vec<_>>>()
    .expect("stream ranged structured rows");

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].row_index(), 2);
    assert!(rows[0].cells().iter().all(|cell| (2..=4).contains(&cell.column_index())));
}

#[test]
fn structured_query_can_stop_early() {
    let mut rows = MiniExcel::query_structured(common::fixture("TestTypeMapping.xlsx"))
        .expect("create structured query");
    assert!(rows.next().expect("first row").is_ok());
    drop(rows);
}
