mod common;

use chrono::NaiveDate;
use miniexcel::{
    CellReference, CellValue, HeaderMode, MiniExcel, ReadOptions, SheetType, SheetVisibility,
};
use rust_xlsxwriter::{Format, Workbook};
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
struct MergedRecord {
    department: String,
    team: String,
    code: String,
}

#[test]
fn queries_rows_through_the_simple_facade() {
    let mut rows = MiniExcel::query(common::fixture("TestDynamicQueryBasic_WithoutHead.xlsx"))
        .expect("create query");

    let first = rows.next().expect("first row").expect("read first row");
    assert_eq!(first["A"], CellValue::String("MiniExcel".to_owned()));
    assert_eq!(first["B"], CellValue::Int(1));
    assert_eq!(rows.count(), 1);
}

#[test]
fn streaming_query_can_stop_early() {
    let mut rows =
        MiniExcel::query(common::fixture("TestTypeMapping.xlsx")).expect("create streaming query");
    assert!(rows.next().expect("first row").is_ok());
    drop(rows);
}

#[test]
fn spills_large_shared_strings_to_disk_and_cleans_up_on_drop() {
    let cache_dir = tempfile::tempdir().expect("create shared-string cache directory");
    let options = ReadOptions::new()
        .with_header_mode(HeaderMode::FirstRow)
        .with_shared_string_cache_size(1)
        .with_shared_string_cache_path(cache_dir.path());
    let mut rows = MiniExcel::query_with_options(common::fixture("TestTypeMapping.xlsx"), &options)
        .expect("create disk-cached query");

    assert!(cache_dir.path().read_dir().unwrap().next().is_some());
    let first = rows.next().unwrap().unwrap();
    assert_eq!(first["Name"], CellValue::String("Wade".to_owned()));
    drop(rows);
    assert!(cache_dir.path().read_dir().unwrap().next().is_none());
}

#[test]
fn keeps_byte_queries_in_memory_and_allows_disabling_the_disk_cache() {
    let path = common::fixture("TestTypeMapping.xlsx");
    let bytes = std::fs::read(&path).expect("read fixture bytes");
    let missing_cache_dir = path.parent().unwrap().join("missing-shared-string-cache");
    let options = ReadOptions::new()
        .with_header_mode(HeaderMode::FirstRow)
        .with_shared_string_cache_size(1)
        .with_shared_string_cache_path(&missing_cache_dir);

    let byte_rows = MiniExcel::query_bytes(&bytes, &options).expect("byte query stays in memory");
    assert_eq!(byte_rows[0]["Name"], CellValue::String("Wade".to_owned()));
    assert!(MiniExcel::query_with_options(&path, &options).is_err());

    let first = MiniExcel::query_with_options(&path, &options.with_shared_string_disk_cache(false))
        .expect("disabled disk cache uses memory")
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(first["Name"], CellValue::String("Wade".to_owned()));
}

#[test]
fn fills_merged_cells_when_requested() {
    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let path = temp_dir.path().join("merged.xlsx");
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.write_string(0, 0, "Department").unwrap();
    worksheet.write_string(0, 1, "Team").unwrap();
    worksheet.write_string(0, 2, "Code").unwrap();
    worksheet.merge_range(1, 0, 3, 0, "HR", &Format::new()).unwrap();
    worksheet.merge_range(1, 1, 1, 2, "Shared", &Format::new()).unwrap();
    worksheet.write_string(2, 1, "A").unwrap();
    worksheet.write_string(2, 2, "1").unwrap();
    worksheet.write_string(3, 1, "B").unwrap();
    worksheet.write_string(3, 2, "2").unwrap();
    workbook.save(&path).expect("write merged workbook");

    let base_options = ReadOptions::new().with_header_mode(HeaderMode::FirstRow);
    let physical_rows = MiniExcel::query_with_options(&path, &base_options)
        .unwrap()
        .collect::<miniexcel::Result<Vec<_>>>()
        .unwrap();
    assert!(physical_rows[1]["Department"].is_empty());
    assert!(physical_rows[0]["Code"].is_empty());

    let filled_rows =
        MiniExcel::query_with_options(&path, &base_options.clone().with_fill_merged_cells(true))
            .unwrap()
            .collect::<miniexcel::Result<Vec<_>>>()
            .unwrap();
    assert_eq!(filled_rows[0]["Department"], CellValue::String("HR".to_owned()));
    assert_eq!(filled_rows[1]["Department"], CellValue::String("HR".to_owned()));
    assert_eq!(filled_rows[2]["Department"], CellValue::String("HR".to_owned()));
    assert_eq!(filled_rows[0]["Team"], CellValue::String("Shared".to_owned()));
    assert_eq!(filled_rows[0]["Code"], CellValue::String("Shared".to_owned()));

    let bytes = std::fs::read(&path).expect("read merged workbook bytes");
    let byte_rows =
        MiniExcel::query_bytes(&bytes, &base_options.clone().with_fill_merged_cells(true))
            .expect("query merged workbook bytes");
    assert_eq!(byte_rows, filled_rows);

    let structured =
        MiniExcel::query_structured_with_options(&path, &base_options.with_fill_merged_cells(true))
            .unwrap()
            .collect::<miniexcel::Result<Vec<_>>>()
            .unwrap();
    assert!(structured[2].cells().iter().all(|cell| cell.column_index() != 1));

    let typed_rows = MiniExcel::query_as_with_options::<MergedRecord>(
        &path,
        &ReadOptions::new().with_fill_merged_cells(true),
    )
    .unwrap()
    .collect::<miniexcel::Result<Vec<_>>>()
    .unwrap();
    assert_eq!(
        typed_rows[0],
        MergedRecord {
            department: "HR".to_owned(),
            team: "Shared".to_owned(),
            code: "Shared".to_owned(),
        }
    );
    assert_eq!(typed_rows[2].department, "HR");
}

#[test]
fn streaming_query_preserves_self_closing_empty_rows() {
    let rows = MiniExcel::query(common::fixture("TestEmptySelfClosingRow.xlsx"))
        .expect("create streaming query")
        .collect::<miniexcel::Result<Vec<_>>>()
        .expect("stream rows");

    assert_eq!(rows.len(), 30);
    assert!(rows[0]["A"].is_empty());
    assert_eq!(rows[1]["A"], CellValue::Int(1));
    assert!(rows[2]["A"].is_empty());
    assert_eq!(rows[3]["A"], CellValue::Int(2));
    assert!(rows[4..9].iter().all(|row| row["A"].is_empty()));
    assert_eq!(rows[9]["A"], CellValue::Int(1));
    assert!(rows[10..].iter().all(|row| row["A"].is_empty()));
}

#[test]
fn streaming_query_selects_sheets_and_infers_missing_cell_references() {
    let options = ReadOptions::new().with_sheet_name("Sheet3");
    let rows = MiniExcel::query_with_options(common::fixture("TestMultiSheet.xlsx"), &options)
        .expect("create Sheet3 query")
        .collect::<miniexcel::Result<Vec<_>>>()
        .expect("stream Sheet3");
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0]["A"], CellValue::Int(3));
    assert_eq!(rows[0]["B"], CellValue::Int(3));

    let rows = MiniExcel::query(common::fixture("TestWihoutRAttribute.xlsx"))
        .expect("create missing-reference query")
        .collect::<miniexcel::Result<Vec<_>>>()
        .expect("stream missing-reference rows");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].len(), 5);
    assert_eq!(rows[1]["B"], CellValue::String("\"<>+}{\\nHello World".to_owned()));
    assert_eq!(rows[1]["C"], CellValue::Bool(true));
}

#[test]
fn streaming_query_honors_start_cells_and_empty_row_filtering() {
    let start_cell: CellReference = "B6".parse().expect("valid A1 reference");
    let options = ReadOptions::new().with_start_cell(start_cell);
    let first = MiniExcel::query_with_options(common::fixture("TestTypeMapping.xlsx"), &options)
        .expect("create start-cell query")
        .next()
        .expect("first selected row")
        .expect("stream first selected row");
    assert_eq!(first["B"], CellValue::String("Raymond".to_owned()));
    assert_eq!(first["D"], CellValue::Int(18));

    let options = ReadOptions::new().with_ignore_empty_rows(true);
    let rows = MiniExcel::query_with_options(
        common::fixture("TestCenterEmptyRow/TestCenterEmptyRow.xlsx"),
        &options,
    )
    .expect("create empty-row query")
    .collect::<miniexcel::Result<Vec<_>>>()
    .expect("stream non-empty rows");
    assert_eq!(rows.len(), 5);
    assert!(rows.iter().all(|row| row.values().any(|value| !value.is_empty())));
}

#[test]
fn streaming_query_reads_cached_formula_values() {
    let path = common::fixture("TestIssue157.xlsx");
    let rows = MiniExcel::query(path)
        .expect("create formula streaming query")
        .collect::<miniexcel::Result<Vec<_>>>()
        .expect("stream cached formula values");

    assert_eq!(rows.len(), 6);
}

#[test]
fn reads_dynamic_rows_without_headers() {
    let rows = MiniExcel::query(common::fixture("TestDynamicQueryBasic_WithoutHead.xlsx"))
        .expect("create query")
        .collect::<miniexcel::Result<Vec<_>>>()
        .expect("read rows");

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["A"], CellValue::String("MiniExcel".to_owned()));
    assert_eq!(rows[0]["B"], CellValue::Int(1));
    assert_eq!(rows[1]["A"], CellValue::String("Github".to_owned()));
    assert_eq!(rows[1]["B"], CellValue::Int(2));
}

#[test]
fn reads_dynamic_rows_with_headers() {
    let options = ReadOptions::new().with_header_mode(HeaderMode::FirstRow);
    let rows =
        MiniExcel::query_with_options(common::fixture("TestDynamicQueryBasic.xlsx"), &options)
            .expect("create query")
            .collect::<miniexcel::Result<Vec<_>>>()
            .expect("read rows");

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["Column1"], CellValue::String("MiniExcel".to_owned()));
    assert_eq!(rows[0]["Column2"], CellValue::Int(1));
    assert_eq!(rows[1]["Column1"], CellValue::String("Github".to_owned()));
    assert_eq!(rows[1]["Column2"], CellValue::Int(2));
}

#[test]
fn preserves_and_can_ignore_empty_rows() {
    let path = common::fixture("TestCenterEmptyRow/TestCenterEmptyRow.xlsx");
    let rows = MiniExcel::query(&path)
        .expect("create query")
        .collect::<miniexcel::Result<Vec<_>>>()
        .expect("read rows");

    assert_eq!(rows.len(), 6);
    assert!(rows[3].values().all(CellValue::is_empty));

    let options = ReadOptions::new().with_ignore_empty_rows(true);
    let rows = MiniExcel::query_with_options(path, &options)
        .expect("create query")
        .collect::<miniexcel::Result<Vec<_>>>()
        .expect("read rows");
    assert_eq!(rows.len(), 5);
    assert!(rows.iter().all(|row| row.values().any(|value| !value.is_empty())));
}

#[test]
fn selects_sheets_in_workbook_order() {
    let path = common::fixture("TestMultiSheet.xlsx");
    assert_eq!(
        MiniExcel::get_sheet_names(&path).expect("read sheet names"),
        ["Sheet1", "Sheet2", "Sheet3"]
    );

    let options = ReadOptions::new().with_sheet_name("Sheet3");
    let rows = MiniExcel::query_with_options(path, &options)
        .expect("create Sheet3 query")
        .collect::<miniexcel::Result<Vec<_>>>()
        .expect("read Sheet3");
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0]["A"], CellValue::Int(3));
    assert_eq!(rows[0]["B"], CellValue::Int(3));
}

#[test]
fn reads_sheet_dimensions_in_workbook_order() {
    let dimensions = MiniExcel::get_sheet_dimensions(common::fixture("TestTypeMapping.xlsx"))
        .expect("read sheet dimensions");
    assert_eq!(dimensions.len(), 1);
    assert_eq!(dimensions[0].start_cell().expect("start cell").to_string(), "A1");
    assert_eq!(dimensions[0].end_cell().expect("end cell").to_string(), "H101");
    assert_eq!(dimensions[0].row_count(), 101);
    assert_eq!(dimensions[0].column_count(), 8);
    assert_eq!(dimensions[0].start_row_index(), Some(1));
    assert_eq!(dimensions[0].end_row_index(), Some(101));
    assert_eq!(dimensions[0].start_column_index(), Some(1));
    assert_eq!(dimensions[0].end_column_index(), Some(8));

    let path = common::fixture("TestMultiSheet.xlsx");
    let dimensions = MiniExcel::get_sheet_dimensions(&path).expect("read multi-sheet dimensions");
    assert_eq!(dimensions.len(), 3);
    assert_eq!(dimensions[0].end_cell().expect("Sheet1 end cell").to_string(), "D12");
    assert_eq!(dimensions[1].end_cell().expect("Sheet2 end cell").to_string(), "D12");
    assert_eq!(dimensions[2].end_cell().expect("Sheet3 end cell").to_string(), "B5");

    let bytes = std::fs::read(path).expect("read fixture bytes");
    assert_eq!(
        MiniExcel::get_sheet_dimensions_from_bytes(&bytes).expect("read in-memory dimensions"),
        dimensions
    );
}

#[test]
fn reads_sheet_metadata_and_visibility_from_paths_and_bytes() {
    let path = common::fixture("TestMultiSheetWithHiddenSheet.xlsx");
    let sheet_info = MiniExcel::get_sheet_info(&path).expect("read sheet metadata");

    assert_eq!(sheet_info.len(), 4);
    assert_eq!(sheet_info[0].index(), 0);
    assert_eq!(sheet_info[0].id(), 2);
    assert_eq!(sheet_info[0].name(), "Sheet2");
    assert_eq!(sheet_info[0].sheet_type(), SheetType::Worksheet);
    assert_eq!(sheet_info[0].visibility(), SheetVisibility::Visible);
    assert!(!sheet_info[0].is_active());
    assert_eq!(sheet_info[1].id(), 1);
    assert!(!sheet_info[1].is_active());
    assert_eq!(sheet_info[2].id(), 3);
    assert!(sheet_info[2].is_active());
    assert_eq!(sheet_info[3].index(), 3);
    assert_eq!(sheet_info[3].id(), 5);
    assert_eq!(sheet_info[3].name(), "HiddenSheet4");
    assert_eq!(sheet_info[3].visibility(), SheetVisibility::Hidden);
    assert!(!sheet_info[3].is_active());

    let bytes = std::fs::read(path).expect("read fixture bytes");
    assert_eq!(
        MiniExcel::get_sheet_info_from_bytes(&bytes).expect("read in-memory sheet metadata"),
        sheet_info
    );
}

#[test]
fn gets_columns_with_headers_start_cells_and_empty_sheets() {
    let path = common::fixture("TestTypeMapping.xlsx");
    assert_eq!(
        MiniExcel::get_columns(&path, &ReadOptions::default()).expect("get letter columns"),
        ["A", "B", "C", "D", "E", "F", "G", "H"]
    );

    let options = ReadOptions::new().with_header_mode(HeaderMode::FirstRow);
    assert_eq!(
        MiniExcel::get_columns(&path, &options).expect("get header columns"),
        ["ID", "Name", "BoD", "Age", "VIP", "Mail", "Points", "IgnoredProperty"]
    );

    let options = ReadOptions::new().with_start_cell("C3".parse().expect("valid start cell"));
    assert_eq!(
        MiniExcel::get_columns(common::fixture("TestIssue147.xlsx"), &options)
            .expect("get columns from start cell"),
        ["C", "D", "E"]
    );

    assert!(
        MiniExcel::get_columns(common::fixture("TestEmpty.xlsx"), &ReadOptions::default())
            .expect("get empty columns")
            .is_empty()
    );
}

#[test]
fn preserves_self_closing_empty_rows() {
    let rows = MiniExcel::query(common::fixture("TestEmptySelfClosingRow.xlsx"))
        .expect("create query")
        .collect::<miniexcel::Result<Vec<_>>>()
        .expect("read rows");

    assert_eq!(rows.len(), 30);
    assert!(rows[0]["A"].is_empty());
    assert_eq!(rows[1]["A"], CellValue::Int(1));
    assert!(rows[2]["A"].is_empty());
    assert_eq!(rows[3]["A"], CellValue::Int(2));
    assert!(rows[4..9].iter().all(|row| row["A"].is_empty()));
    assert_eq!(rows[9]["A"], CellValue::Int(1));
    assert!(rows[10..].iter().all(|row| row["A"].is_empty()));
}

#[test]
fn reads_from_an_a1_start_cell() {
    let start_cell: CellReference = "B6".parse().expect("valid A1 reference");
    let options = ReadOptions::new().with_start_cell(start_cell);
    let rows = MiniExcel::query_with_options(common::fixture("TestTypeMapping.xlsx"), &options)
        .expect("create query")
        .collect::<miniexcel::Result<Vec<_>>>()
        .expect("read rows");

    assert_eq!(rows[0]["B"], CellValue::String("Raymond".to_owned()));
    assert_eq!(
        rows[0]["C"],
        CellValue::DateTime(
            NaiveDate::from_ymd_opt(2021, 12, 7).unwrap().and_hms_opt(0, 0, 0).unwrap()
        )
    );
    assert_eq!(rows[0]["D"], CellValue::Int(18));
}

#[test]
fn reads_an_inclusive_cell_range_with_headers() {
    let options = ReadOptions::new()
        .with_start_cell("C3".parse().expect("valid start cell"))
        .with_end_cell("E6".parse().expect("valid end cell"))
        .with_header_mode(HeaderMode::FirstRow);
    let rows = MiniExcel::query_with_options(common::fixture("TestQueryRange.xlsx"), &options)
        .expect("create range query")
        .collect::<miniexcel::Result<Vec<_>>>()
        .expect("read range");

    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].len(), 3);
    assert_eq!(rows[0]["Name"], CellValue::String("Wade".to_owned()));
    assert_eq!(
        rows[0]["BoD"],
        CellValue::DateTime(
            NaiveDate::from_ymd_opt(2020, 9, 27).unwrap().and_hms_opt(0, 0, 0).unwrap()
        )
    );
    assert_eq!(rows[0]["Age"], CellValue::Int(36));
    assert_eq!(rows[2]["Name"], CellValue::String("Phelan".to_owned()));
    assert_eq!(rows[2]["Age"], CellValue::Int(33));
}

#[test]
fn rejects_a_range_ending_before_its_start() {
    let options = ReadOptions::new()
        .with_start_cell("C3".parse().expect("valid start cell"))
        .with_end_cell("B6".parse().expect("valid end cell"));
    let error =
        match MiniExcel::query_with_options(common::fixture("TestQueryRange.xlsx"), &options) {
            Ok(_) => panic!("reverse range should fail"),
            Err(error) => error,
        };

    assert!(error.to_string().contains("end cell B6 precedes start cell C3"));
}

#[test]
fn reads_cells_without_r_attributes() {
    let rows = MiniExcel::query(common::fixture("TestWihoutRAttribute.xlsx"))
        .expect("create query")
        .collect::<miniexcel::Result<Vec<_>>>()
        .expect("read rows");

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].len(), 5);
    assert_eq!(rows[0]["A"], CellValue::Int(1));
    assert!(rows[0]["C"].is_empty());
    assert!(rows[0]["D"].is_empty());
    assert!(rows[0]["E"].is_empty());
    assert_eq!(rows[1]["A"], CellValue::Int(1));
    assert_eq!(rows[1]["B"], CellValue::String("\"<>+}{\\nHello World".to_owned()));
    assert_eq!(rows[1]["C"], CellValue::Bool(true));
}

#[test]
fn in_memory_query_matches_path_query() {
    let path = common::fixture("TestWihoutRAttribute.xlsx");
    let bytes = std::fs::read(&path).expect("read fixture bytes");
    let path_rows = MiniExcel::query(&path)
        .expect("create path query")
        .collect::<miniexcel::Result<Vec<_>>>()
        .expect("read path rows");
    let memory_rows =
        MiniExcel::query_bytes(&bytes, &ReadOptions::default()).expect("read in-memory rows");

    assert_eq!(
        MiniExcel::get_sheet_names_from_bytes(&bytes).unwrap(),
        MiniExcel::get_sheet_names(&path).unwrap()
    );
    assert_eq!(memory_rows, path_rows);
}
