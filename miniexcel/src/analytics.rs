use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::Path;

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::streaming::{StreamingRows, visit_dynamic_rows};
use crate::{CellValue, DynamicRow, Error, ReadOptions, Result};

const QUERY_PLAN_VERSION: &str = "miniexcel.query-plan/v1";
const DEFAULT_MAX_GROUPS: usize = 10_000;
const DEFAULT_EVIDENCE_ROWS: usize = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ComparisonOp {
    Eq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    Contains,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum QueryLiteral {
    Empty,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Date(NaiveDate),
    Time(NaiveTime),
    DateTime(NaiveDateTime),
    Duration(Duration),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FilterExpr {
    And { expressions: Vec<Self> },
    Or { expressions: Vec<Self> },
    Not { expression: Box<Self> },
    Compare { column: String, op: ComparisonOp, value: QueryLiteral },
    IsEmpty { column: String },
    IsNotEmpty { column: String },
}

impl FilterExpr {
    #[must_use]
    pub fn compare(column: impl Into<String>, op: ComparisonOp, value: QueryLiteral) -> Self {
        Self::Compare { column: column.into(), op, value }
    }

    #[must_use]
    pub fn and(expressions: impl IntoIterator<Item = Self>) -> Self {
        Self::And { expressions: expressions.into_iter().collect() }
    }

    #[must_use]
    pub fn or(expressions: impl IntoIterator<Item = Self>) -> Self {
        Self::Or { expressions: expressions.into_iter().collect() }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AggregateOp {
    Count,
    Sum,
    Average,
    Min,
    Max,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateSpec {
    op: AggregateOp,
    column: Option<String>,
    alias: String,
}

impl AggregateSpec {
    #[must_use]
    pub fn count_all(alias: impl Into<String>) -> Self {
        Self { op: AggregateOp::Count, column: None, alias: alias.into() }
    }

    #[must_use]
    pub fn column(op: AggregateOp, column: impl Into<String>, alias: impl Into<String>) -> Self {
        Self { op, column: Some(column.into()), alias: alias.into() }
    }

    #[must_use]
    pub const fn op(&self) -> AggregateOp {
        self.op
    }

    #[must_use]
    pub fn column_name(&self) -> Option<&str> {
        self.column.as_deref()
    }

    #[must_use]
    pub fn alias(&self) -> &str {
        &self.alias
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct QueryPlan {
    version: String,
    filter: Option<FilterExpr>,
    group_by: Vec<String>,
    aggregates: Vec<AggregateSpec>,
    max_groups: usize,
    limit: Option<usize>,
    evidence_rows_per_group: usize,
}

impl QueryPlan {
    #[must_use]
    pub fn new(aggregates: impl IntoIterator<Item = AggregateSpec>) -> Self {
        Self { aggregates: aggregates.into_iter().collect(), ..Self::default() }
    }

    #[must_use]
    pub fn with_filter(mut self, filter: FilterExpr) -> Self {
        self.filter = Some(filter);
        self
    }

    #[must_use]
    pub fn with_group_by(mut self, columns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.group_by = columns.into_iter().map(Into::into).collect();
        self
    }

    #[must_use]
    pub const fn with_max_groups(mut self, max_groups: usize) -> Self {
        self.max_groups = max_groups;
        self
    }

    #[must_use]
    pub const fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    #[must_use]
    pub const fn with_evidence_rows_per_group(mut self, rows: usize) -> Self {
        self.evidence_rows_per_group = rows;
        self
    }

    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    #[must_use]
    pub fn filter(&self) -> Option<&FilterExpr> {
        self.filter.as_ref()
    }

    #[must_use]
    pub fn group_by(&self) -> &[String] {
        &self.group_by
    }

    #[must_use]
    pub fn aggregates(&self) -> &[AggregateSpec] {
        &self.aggregates
    }

    #[must_use]
    pub const fn max_groups(&self) -> usize {
        self.max_groups
    }

    #[must_use]
    pub const fn limit(&self) -> Option<usize> {
        self.limit
    }
}

impl Default for QueryPlan {
    fn default() -> Self {
        Self {
            version: QUERY_PLAN_VERSION.to_owned(),
            filter: None,
            group_by: Vec::new(),
            aggregates: Vec::new(),
            max_groups: DEFAULT_MAX_GROUPS,
            limit: None,
            evidence_rows_per_group: DEFAULT_EVIDENCE_ROWS,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisRow {
    values: DynamicRow,
    source_rows: Vec<u32>,
}

impl AnalysisRow {
    #[must_use]
    pub fn values(&self) -> &DynamicRow {
        &self.values
    }

    #[must_use]
    pub fn source_rows(&self) -> &[u32] {
        &self.source_rows
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisStats {
    seen_rows: usize,
    matched_rows: usize,
    total_groups: usize,
    returned_rows: usize,
    truncated: bool,
}

impl AnalysisStats {
    #[must_use]
    pub const fn seen_rows(&self) -> usize {
        self.seen_rows
    }

    #[must_use]
    pub const fn matched_rows(&self) -> usize {
        self.matched_rows
    }

    #[must_use]
    pub const fn total_groups(&self) -> usize {
        self.total_groups
    }

    #[must_use]
    pub const fn returned_rows(&self) -> usize {
        self.returned_rows
    }

    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.truncated
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisResult {
    version: String,
    sheet_name: String,
    plan: QueryPlan,
    rows: Vec<AnalysisRow>,
    stats: AnalysisStats,
}

impl AnalysisResult {
    #[must_use]
    pub fn sheet_name(&self) -> &str {
        &self.sheet_name
    }

    #[must_use]
    pub fn plan(&self) -> &QueryPlan {
        &self.plan
    }

    #[must_use]
    pub fn rows(&self) -> &[AnalysisRow] {
        &self.rows
    }

    #[must_use]
    pub const fn stats(&self) -> &AnalysisStats {
        &self.stats
    }
}

pub(crate) fn analyze_path(
    path: impl AsRef<Path>,
    options: &ReadOptions,
    plan: &QueryPlan,
) -> Result<AnalysisResult> {
    let mut rows = StreamingRows::open(path, options)?;
    let sheet_name = rows.sheet_name().to_owned();
    let mut accumulator = AnalysisAccumulator::new(plan.clone())?;
    while let Some(row) = rows.next_with_excel_row() {
        let (excel_row, row) = row?;
        accumulator.consume(&sheet_name, excel_row, &row)?;
    }
    accumulator.finish(sheet_name, &rows.columns())
}

pub(crate) fn analyze_bytes(
    bytes: &[u8],
    options: &ReadOptions,
    plan: &QueryPlan,
) -> Result<AnalysisResult> {
    let mut accumulator = AnalysisAccumulator::new(plan.clone())?;
    let summary = visit_dynamic_rows(bytes, options, |sheet_name, excel_row, row| {
        accumulator.consume(sheet_name, excel_row, &row)?;
        Ok(true)
    })?;
    accumulator.finish(summary.sheet_name().to_owned(), summary.columns())
}

struct AnalysisAccumulator {
    plan: QueryPlan,
    groups: IndexMap<GroupKey, GroupState>,
    seen_rows: usize,
    matched_rows: usize,
    columns_validated: bool,
}

impl AnalysisAccumulator {
    fn new(plan: QueryPlan) -> Result<Self> {
        validate_plan(&plan)?;
        let mut groups = IndexMap::new();
        if plan.group_by.is_empty() {
            groups.insert(GroupKey(Vec::new()), GroupState::new(Vec::new(), &plan));
        }
        Ok(Self { plan, groups, seen_rows: 0, matched_rows: 0, columns_validated: false })
    }

    fn consume(&mut self, sheet_name: &str, excel_row: usize, row: &DynamicRow) -> Result<()> {
        if !self.columns_validated {
            self.validate_columns(row.keys().map(String::as_str))?;
        }
        self.seen_rows += 1;
        if let Some(filter) = &self.plan.filter {
            if !evaluate_filter(filter, row, sheet_name, excel_row)? {
                return Ok(());
            }
        }
        self.matched_rows += 1;

        let mut group_values = Vec::with_capacity(self.plan.group_by.len());
        let mut group_key = Vec::with_capacity(self.plan.group_by.len());
        for column in &self.plan.group_by {
            let value = row.get(column).expect("group column validated");
            group_key.push(group_part(value, sheet_name, excel_row, column)?);
            group_values.push(value.clone());
        }
        let key = GroupKey(group_key);
        if !self.groups.contains_key(&key) {
            if self.groups.len() >= self.plan.max_groups {
                return Err(Error::group_limit(self.plan.max_groups));
            }
            self.groups.insert(key.clone(), GroupState::new(group_values, &self.plan));
        }
        let group = self.groups.get_mut(&key).expect("group inserted");
        if group.source_rows.len() < self.plan.evidence_rows_per_group {
            group.source_rows.push(excel_row as u32);
        }
        for (state, spec) in group.aggregates.iter_mut().zip(&self.plan.aggregates) {
            let value = spec.column.as_deref().and_then(|column| row.get(column));
            state.update(value).map_err(|message| {
                Error::analytics(
                    sheet_name,
                    excel_row,
                    spec.column.as_deref().unwrap_or("*"),
                    message,
                )
            })?;
        }
        Ok(())
    }

    fn validate_columns<'a>(&mut self, columns: impl IntoIterator<Item = &'a str>) -> Result<()> {
        let available = columns.into_iter().collect::<HashSet<_>>();
        for column in referenced_columns(&self.plan) {
            if !available.contains(column.as_str()) {
                return Err(Error::invalid_query(format!("column '{column}' was not found")));
            }
        }
        self.columns_validated = true;
        Ok(())
    }

    fn finish(mut self, sheet_name: String, columns: &[String]) -> Result<AnalysisResult> {
        if !self.columns_validated {
            self.validate_columns(columns.iter().map(String::as_str))?;
        }
        let total_groups = self.groups.len();
        let limit = self.plan.limit.unwrap_or(usize::MAX);
        let mut rows = Vec::with_capacity(total_groups.min(limit));
        for (_, group) in self.groups.into_iter().take(limit) {
            let mut values = DynamicRow::new();
            for (column, value) in self.plan.group_by.iter().zip(group.group_values) {
                values.insert(column.clone(), value);
            }
            for (spec, state) in self.plan.aggregates.iter().zip(group.aggregates) {
                values.insert(spec.alias.clone(), state.finish()?);
            }
            rows.push(AnalysisRow { values, source_rows: group.source_rows });
        }
        let returned_rows = rows.len();
        Ok(AnalysisResult {
            version: "miniexcel.analysis-result/v1".to_owned(),
            sheet_name,
            plan: self.plan,
            rows,
            stats: AnalysisStats {
                seen_rows: self.seen_rows,
                matched_rows: self.matched_rows,
                total_groups,
                returned_rows,
                truncated: returned_rows < total_groups,
            },
        })
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct GroupKey(Vec<GroupPart>);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum GroupPart {
    Empty,
    Bool(bool),
    Int(i64),
    Float(u64),
    String(String),
    Date(NaiveDate),
    Time(NaiveTime),
    DateTime(NaiveDateTime),
    Duration(i64),
}

fn group_part(
    value: &CellValue,
    sheet_name: &str,
    excel_row: usize,
    column: &str,
) -> Result<GroupPart> {
    let part = match value {
        CellValue::Empty => GroupPart::Empty,
        CellValue::Bool(value) => GroupPart::Bool(*value),
        CellValue::Int(value) => GroupPart::Int(*value),
        CellValue::Float(value)
            if value.is_finite()
                && value.fract() == 0.0
                && *value >= i64::MIN as f64
                && *value <= i64::MAX as f64 =>
        {
            GroupPart::Int(*value as i64)
        }
        CellValue::Float(value) => {
            GroupPart::Float(if *value == 0.0 { 0.0_f64.to_bits() } else { value.to_bits() })
        }
        CellValue::String(value) => GroupPart::String(value.clone()),
        CellValue::Date(value) => GroupPart::Date(*value),
        CellValue::Time(value) => GroupPart::Time(*value),
        CellValue::DateTime(value) => GroupPart::DateTime(*value),
        CellValue::Duration(value) => {
            GroupPart::Duration(value.num_nanoseconds().ok_or_else(|| {
                Error::analytics(sheet_name, excel_row, column, "duration is out of range")
            })?)
        }
        CellValue::Error(error) => {
            return Err(Error::analytics(
                sheet_name,
                excel_row,
                column,
                format!("cell contains Excel error {error}"),
            ));
        }
    };
    Ok(part)
}

struct GroupState {
    group_values: Vec<CellValue>,
    aggregates: Vec<AggregateState>,
    source_rows: Vec<u32>,
}

impl GroupState {
    fn new(group_values: Vec<CellValue>, plan: &QueryPlan) -> Self {
        Self {
            group_values,
            aggregates: plan.aggregates.iter().map(AggregateState::new).collect(),
            source_rows: Vec::with_capacity(plan.evidence_rows_per_group),
        }
    }
}

enum AggregateState {
    Count(i64),
    Sum(Option<NumericValue>),
    Average { sum: f64, count: u64 },
    Min(Option<CellValue>),
    Max(Option<CellValue>),
}

impl AggregateState {
    fn new(spec: &AggregateSpec) -> Self {
        match spec.op {
            AggregateOp::Count => Self::Count(0),
            AggregateOp::Sum => Self::Sum(None),
            AggregateOp::Average => Self::Average { sum: 0.0, count: 0 },
            AggregateOp::Min => Self::Min(None),
            AggregateOp::Max => Self::Max(None),
        }
    }

    fn update(&mut self, value: Option<&CellValue>) -> std::result::Result<(), String> {
        match self {
            Self::Count(count) => {
                if value.is_none_or(|value| !value.is_empty()) {
                    *count = count.checked_add(1).ok_or("count overflow")?;
                }
            }
            Self::Sum(sum) => {
                let Some(value) = value.filter(|value| !value.is_empty()) else {
                    return Ok(());
                };
                let number = NumericValue::from_cell(value)?;
                *sum = Some(match sum.take() {
                    None => number,
                    Some(current) => current.checked_add(number)?,
                });
            }
            Self::Average { sum, count } => {
                let Some(value) = value.filter(|value| !value.is_empty()) else {
                    return Ok(());
                };
                *sum += NumericValue::from_cell(value)?.as_f64();
                *count = count.checked_add(1).ok_or("average count overflow")?;
            }
            Self::Min(current) => update_extreme(current, value, Ordering::Greater)?,
            Self::Max(current) => update_extreme(current, value, Ordering::Less)?,
        }
        Ok(())
    }

    fn finish(self) -> Result<CellValue> {
        match self {
            Self::Count(value) => Ok(CellValue::Int(value)),
            Self::Sum(value) => Ok(value.map_or(CellValue::Empty, NumericValue::into_cell)),
            Self::Average { count: 0, .. } => Ok(CellValue::Empty),
            Self::Average { sum, count } => Ok(CellValue::Float(sum / count as f64)),
            Self::Min(value) | Self::Max(value) => Ok(value.unwrap_or(CellValue::Empty)),
        }
    }
}

fn update_extreme(
    current: &mut Option<CellValue>,
    value: Option<&CellValue>,
    replace_when: Ordering,
) -> std::result::Result<(), String> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    if matches!(value, CellValue::Error(_)) {
        return Err("cell contains an Excel error".to_owned());
    }
    match current {
        None => *current = Some(value.clone()),
        Some(existing) => {
            let ordering = compare_cell_values(existing, value)?;
            if ordering == replace_when {
                *existing = value.clone();
            }
        }
    }
    Ok(())
}

enum NumericValue {
    Int(i64),
    Float(f64),
}

impl NumericValue {
    fn from_cell(value: &CellValue) -> std::result::Result<Self, String> {
        match value {
            CellValue::Int(value) => Ok(Self::Int(*value)),
            CellValue::Float(value) => Ok(Self::Float(*value)),
            CellValue::Error(error) => Err(format!("cell contains Excel error {error}")),
            _ => Err("aggregate requires a numeric value".to_owned()),
        }
    }

    fn checked_add(self, other: Self) -> std::result::Result<Self, String> {
        match (self, other) {
            (Self::Int(left), Self::Int(right)) => left
                .checked_add(right)
                .map(Self::Int)
                .ok_or_else(|| "integer sum overflow".to_owned()),
            (left, right) => Ok(Self::Float(left.as_f64() + right.as_f64())),
        }
    }

    const fn as_f64(&self) -> f64 {
        match self {
            Self::Int(value) => *value as f64,
            Self::Float(value) => *value,
        }
    }

    fn into_cell(self) -> CellValue {
        match self {
            Self::Int(value) => CellValue::Int(value),
            Self::Float(value) => CellValue::Float(value),
        }
    }
}

fn validate_plan(plan: &QueryPlan) -> Result<()> {
    if plan.version != QUERY_PLAN_VERSION {
        return Err(Error::invalid_query(format!(
            "unsupported plan version '{}'; expected '{QUERY_PLAN_VERSION}'",
            plan.version
        )));
    }
    if plan.aggregates.is_empty() {
        return Err(Error::invalid_query("at least one aggregate is required"));
    }
    if plan.max_groups == 0 {
        return Err(Error::invalid_query("max_groups must be greater than zero"));
    }
    if plan.limit == Some(0) {
        return Err(Error::invalid_query("limit must be greater than zero"));
    }
    let mut names = HashSet::new();
    for column in &plan.group_by {
        if column.is_empty() || !names.insert(column.as_str()) {
            return Err(Error::invalid_query(format!(
                "invalid or duplicate group column '{column}'"
            )));
        }
    }
    for aggregate in &plan.aggregates {
        if aggregate.alias.is_empty() || !names.insert(aggregate.alias.as_str()) {
            return Err(Error::invalid_query(format!(
                "invalid, duplicate, or colliding aggregate alias '{}'",
                aggregate.alias
            )));
        }
        if aggregate.op != AggregateOp::Count && aggregate.column.is_none() {
            return Err(Error::invalid_query(format!(
                "aggregate '{}' requires a column",
                aggregate.alias
            )));
        }
    }
    Ok(())
}

fn referenced_columns(plan: &QueryPlan) -> Vec<String> {
    let mut columns = plan.group_by.clone();
    for aggregate in &plan.aggregates {
        if let Some(column) = &aggregate.column {
            columns.push(column.clone());
        }
    }
    if let Some(filter) = &plan.filter {
        filter_columns(filter, &mut columns);
    }
    columns.sort();
    columns.dedup();
    columns
}

fn filter_columns(filter: &FilterExpr, columns: &mut Vec<String>) {
    match filter {
        FilterExpr::And { expressions } | FilterExpr::Or { expressions } => {
            for expression in expressions {
                filter_columns(expression, columns);
            }
        }
        FilterExpr::Not { expression } => filter_columns(expression, columns),
        FilterExpr::Compare { column, .. }
        | FilterExpr::IsEmpty { column }
        | FilterExpr::IsNotEmpty { column } => columns.push(column.clone()),
    }
}

fn evaluate_filter(
    filter: &FilterExpr,
    row: &DynamicRow,
    sheet_name: &str,
    excel_row: usize,
) -> Result<bool> {
    match filter {
        FilterExpr::And { expressions } => {
            for expression in expressions {
                if !evaluate_filter(expression, row, sheet_name, excel_row)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        FilterExpr::Or { expressions } => {
            for expression in expressions {
                if evaluate_filter(expression, row, sheet_name, excel_row)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        FilterExpr::Not { expression } => {
            Ok(!evaluate_filter(expression, row, sheet_name, excel_row)?)
        }
        FilterExpr::IsEmpty { column } => Ok(row[column].is_empty()),
        FilterExpr::IsNotEmpty { column } => Ok(!row[column].is_empty()),
        FilterExpr::Compare { column, op, value } => {
            let cell = &row[column];
            if let CellValue::Error(error) = cell {
                return Err(Error::analytics(
                    sheet_name,
                    excel_row,
                    column,
                    format!("cell contains Excel error {error}"),
                ));
            }
            evaluate_comparison(cell, *op, value)
                .map_err(|message| Error::analytics(sheet_name, excel_row, column, message))
        }
    }
}

fn evaluate_comparison(
    cell: &CellValue,
    op: ComparisonOp,
    literal: &QueryLiteral,
) -> std::result::Result<bool, String> {
    let other = literal_to_cell(literal);
    if op == ComparisonOp::Contains {
        return match (cell, other) {
            (CellValue::String(value), CellValue::String(needle)) => Ok(value.contains(&needle)),
            _ => Err("contains requires string operands".to_owned()),
        };
    }
    if matches!((cell, &other), (CellValue::Empty, CellValue::Empty)) {
        return Ok(matches!(op, ComparisonOp::Eq | ComparisonOp::Le | ComparisonOp::Ge));
    }
    if cell.is_empty() || other.is_empty() {
        return match op {
            ComparisonOp::Eq => Ok(false),
            ComparisonOp::NotEq => Ok(true),
            _ => Err("empty values only support equality comparisons".to_owned()),
        };
    }
    let ordering = compare_cell_values(cell, &other)?;
    Ok(match op {
        ComparisonOp::Eq => ordering == Ordering::Equal,
        ComparisonOp::NotEq => ordering != Ordering::Equal,
        ComparisonOp::Lt => ordering == Ordering::Less,
        ComparisonOp::Le => ordering != Ordering::Greater,
        ComparisonOp::Gt => ordering == Ordering::Greater,
        ComparisonOp::Ge => ordering != Ordering::Less,
        ComparisonOp::Contains => unreachable!(),
    })
}

fn compare_cell_values(
    left: &CellValue,
    right: &CellValue,
) -> std::result::Result<Ordering, String> {
    match (left, right) {
        (CellValue::Int(left), CellValue::Int(right)) => Ok(left.cmp(right)),
        (CellValue::Int(left), CellValue::Float(right)) => (*left as f64)
            .partial_cmp(right)
            .ok_or_else(|| "numeric comparison contains NaN".to_owned()),
        (CellValue::Float(left), CellValue::Int(right)) => left
            .partial_cmp(&(*right as f64))
            .ok_or_else(|| "numeric comparison contains NaN".to_owned()),
        (CellValue::Float(left), CellValue::Float(right)) => {
            left.partial_cmp(right).ok_or_else(|| "numeric comparison contains NaN".to_owned())
        }
        (CellValue::Bool(left), CellValue::Bool(right)) => Ok(left.cmp(right)),
        (CellValue::String(left), CellValue::String(right)) => Ok(left.cmp(right)),
        (CellValue::Date(left), CellValue::Date(right)) => Ok(left.cmp(right)),
        (CellValue::Time(left), CellValue::Time(right)) => Ok(left.cmp(right)),
        (CellValue::DateTime(left), CellValue::DateTime(right)) => Ok(left.cmp(right)),
        (CellValue::Duration(left), CellValue::Duration(right)) => Ok(left.cmp(right)),
        (CellValue::Error(error), _) | (_, CellValue::Error(error)) => {
            Err(format!("cell contains Excel error {error}"))
        }
        _ => Err("comparison operands have incompatible types".to_owned()),
    }
}

fn literal_to_cell(literal: &QueryLiteral) -> CellValue {
    match literal {
        QueryLiteral::Empty => CellValue::Empty,
        QueryLiteral::Bool(value) => CellValue::Bool(*value),
        QueryLiteral::Int(value) => CellValue::Int(*value),
        QueryLiteral::Float(value) => CellValue::Float(*value),
        QueryLiteral::String(value) => CellValue::String(value.clone()),
        QueryLiteral::Date(value) => CellValue::Date(*value),
        QueryLiteral::Time(value) => CellValue::Time(*value),
        QueryLiteral::DateTime(value) => CellValue::DateTime(*value),
        QueryLiteral::Duration(value) => CellValue::Duration(*value),
    }
}
