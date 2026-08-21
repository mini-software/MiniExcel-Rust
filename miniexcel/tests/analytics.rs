use miniexcel::{
    AggregateOp, AggregateSpec, CellValue, ComparisonOp, DynamicRow, FilterExpr, HeaderMode,
    MiniExcel, QueryLiteral, QueryPlan, ReadOptions, WriteOptions,
};
use tempfile::NamedTempFile;

fn workbook() -> NamedTempFile {
    let file = NamedTempFile::new().expect("create workbook path");
    let rows = [
        row("Hardware", "East", true, CellValue::Int(10)),
        row("Hardware", "East", true, CellValue::Float(12.5)),
        row("Software", "West", false, CellValue::Int(40)),
        row("Software", "West", true, CellValue::Int(20)),
        row("Hardware", "West", true, CellValue::Empty),
    ];
    MiniExcel::save_as_with_options(
        file.path(),
        &rows,
        &WriteOptions::new().with_sheet_name("Sales").with_overwrite_file(true),
    )
    .expect("write workbook");
    file
}

fn row(category: &str, region: &str, active: bool, amount: CellValue) -> DynamicRow {
    let mut row = DynamicRow::new();
    row.insert("Category".to_owned(), CellValue::String(category.to_owned()));
    row.insert("Region".to_owned(), CellValue::String(region.to_owned()));
    row.insert("Active".to_owned(), CellValue::Bool(active));
    row.insert("Amount".to_owned(), amount);
    row
}

fn read_options() -> ReadOptions {
    ReadOptions::new().with_sheet_name("Sales").with_header_mode(HeaderMode::FirstRow)
}

fn grouped_plan() -> QueryPlan {
    QueryPlan::new([
        AggregateSpec::count_all("rows"),
        AggregateSpec::column(AggregateOp::Count, "Amount", "amountCount"),
        AggregateSpec::column(AggregateOp::Sum, "Amount", "total"),
        AggregateSpec::column(AggregateOp::Average, "Amount", "average"),
        AggregateSpec::column(AggregateOp::Min, "Amount", "minimum"),
        AggregateSpec::column(AggregateOp::Max, "Amount", "maximum"),
    ])
    .with_filter(FilterExpr::and([
        FilterExpr::compare("Active", ComparisonOp::Eq, QueryLiteral::Bool(true)),
        FilterExpr::or([
            FilterExpr::compare(
                "Region",
                ComparisonOp::Eq,
                QueryLiteral::String("East".to_owned()),
            ),
            FilterExpr::compare(
                "Region",
                ComparisonOp::Eq,
                QueryLiteral::String("West".to_owned()),
            ),
        ]),
    ]))
    .with_group_by(["Category", "Region"])
}

#[test]
fn grouped_analysis_matches_for_paths_and_bytes() {
    let file = workbook();
    let plan = grouped_plan();
    let path_result =
        MiniExcel::analyze_with_options(file.path(), &read_options(), &plan).expect("analyze path");
    let bytes = std::fs::read(file.path()).expect("read workbook bytes");
    let byte_result =
        MiniExcel::analyze_bytes(&bytes, &read_options(), &plan).expect("analyze workbook bytes");

    assert_eq!(byte_result, path_result);
    assert_eq!(path_result.sheet_name(), "Sales");
    assert_eq!(path_result.stats().seen_rows(), 5);
    assert_eq!(path_result.stats().matched_rows(), 4);
    assert_eq!(path_result.stats().total_groups(), 3);
    assert!(!path_result.stats().truncated());

    let groups = path_result.rows();
    assert_eq!(groups[0].values()["Category"], CellValue::String("Hardware".to_owned()));
    assert_eq!(groups[0].values()["Region"], CellValue::String("East".to_owned()));
    assert_eq!(groups[0].values()["rows"], CellValue::Int(2));
    assert_eq!(groups[0].values()["amountCount"], CellValue::Int(2));
    assert_eq!(groups[0].values()["total"], CellValue::Float(22.5));
    assert_eq!(groups[0].values()["average"], CellValue::Float(11.25));
    assert_eq!(groups[0].values()["minimum"], CellValue::Int(10));
    assert_eq!(groups[0].values()["maximum"], CellValue::Float(12.5));
    assert_eq!(groups[0].source_rows(), [2, 3]);

    assert_eq!(groups[1].values()["Category"], CellValue::String("Software".to_owned()));
    assert_eq!(groups[1].values()["total"], CellValue::Int(20));
    assert_eq!(groups[2].values()["Region"], CellValue::String("West".to_owned()));
    assert_eq!(groups[2].values()["amountCount"], CellValue::Int(0));
    assert!(groups[2].values()["total"].is_empty());
}

#[test]
fn global_analysis_emits_empty_aggregate_row() {
    let file = workbook();
    let plan = QueryPlan::new([
        AggregateSpec::count_all("rows"),
        AggregateSpec::column(AggregateOp::Sum, "Amount", "total"),
    ])
    .with_filter(FilterExpr::compare(
        "Category",
        ComparisonOp::Eq,
        QueryLiteral::String("Missing".to_owned()),
    ));

    let result = MiniExcel::analyze_with_options(file.path(), &read_options(), &plan)
        .expect("analyze empty selection");
    assert_eq!(result.rows().len(), 1);
    assert_eq!(result.rows()[0].values()["rows"], CellValue::Int(0));
    assert!(result.rows()[0].values()["total"].is_empty());
    assert_eq!(result.stats().matched_rows(), 0);
}

#[test]
fn group_limit_fails_deterministically() {
    let file = workbook();
    let plan = QueryPlan::new([AggregateSpec::count_all("rows")])
        .with_group_by(["Category", "Region"])
        .with_max_groups(2);

    let error = MiniExcel::analyze_with_options(file.path(), &read_options(), &plan)
        .expect_err("third group should exceed the limit");
    assert!(error.to_string().contains("exceeded max_groups (2)"));
}

#[test]
fn query_plan_has_a_versioned_json_contract() {
    let json = serde_json::to_value(grouped_plan()).expect("serialize query plan");
    assert_eq!(json["version"], "miniexcel.query-plan/v1");
    assert_eq!(json["maxGroups"], 10_000);
    assert_eq!(json["aggregates"][0]["op"], "count");
    assert_eq!(json["filter"]["kind"], "and");
}
