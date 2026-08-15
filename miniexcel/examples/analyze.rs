use miniexcel::{
    AggregateOp, AggregateSpec, ComparisonOp, FilterExpr, HeaderMode, MiniExcel, QueryLiteral,
    QueryPlan, ReadOptions,
};

fn main() -> miniexcel::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| "book.xlsx".to_owned());
    let options = ReadOptions::new().with_header_mode(HeaderMode::FirstRow);
    let plan = QueryPlan::new([
        AggregateSpec::count_all("rows"),
        AggregateSpec::column(AggregateOp::Sum, "Amount", "totalAmount"),
        AggregateSpec::column(AggregateOp::Average, "Amount", "averageAmount"),
    ])
    .with_filter(FilterExpr::compare(
        "Status",
        ComparisonOp::Eq,
        QueryLiteral::String("Ready".to_owned()),
    ))
    .with_group_by(["Category", "Region"])
    .with_max_groups(10_000)
    .with_limit(200);

    let result = MiniExcel::analyze_with_options(path, &options, &plan)?;
    println!("{}", serde_json::to_string_pretty(&result).expect("serialize analysis"));
    Ok(())
}
