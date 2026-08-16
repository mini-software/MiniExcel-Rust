# MiniExcel for Rust

[简体中文](README.zh-CN.md)

[![Crates.io](https://img.shields.io/crates/v/miniexcel.svg)](https://crates.io/crates/miniexcel)
[![Documentation](https://docs.rs/miniexcel/badge.svg)](https://docs.rs/miniexcel)
[![License](https://img.shields.io/crates/l/miniexcel.svg)](LICENSE)

A Rust XLSX reader and writer with bounded-memory streaming, Serde integration, structured cell access, analytics, and RAG exports.

**[Open the MiniExcel Browser Lab](https://mini-software.github.io/MiniExcel-Rust/)** to inspect or generate XLSX files locally. Workbooks never leave the browser.

## Installation

```bash
cargo add miniexcel
```

MiniExcel requires Rust 1.85.0 or later.

## Features

- Bounded-memory dynamic and typed worksheet streaming.
- Structured reads with cell addresses, formulas, and number formats.
- Worksheet selection, A1 ranges, headers, and empty-row filtering.
- Serde-based typed reading and writing.
- Dynamic workbook creation with stable column ordering.
- Filtered and grouped streaming analytics with explicit memory limits.
- Source-addressed JSONL and Markdown exports for LLM/RAG workflows.
- Strings, numbers, booleans, errors, dates, times, datetimes, and durations.

## Public API

`MiniExcel` is the main entry point. Date/time Serde adapters are available under `serde_helpers`.

Worksheet metadata is available from paths or in-memory XLSX data:

```rust
use miniexcel::{MiniExcel, SheetVisibility};

for sheet in MiniExcel::get_sheet_info("book.xlsx")? {
    println!(
        "{} (id={}): {:?}, active={}",
        sheet.name(),
        sheet.id(),
        sheet.visibility(),
        sheet.is_active()
    );
    if sheet.visibility() == SheetVisibility::Hidden {
        println!("{} is hidden", sheet.name());
    }
}
```

## Simple Streaming Query

The closest Rust equivalent to `MiniExcel.Query` is an iterator:

```rust
use miniexcel::MiniExcel;

for row in MiniExcel::query("book.xlsx")? {
    let row = row?;
    println!("{:?}", row["A"]);
}
```

Worksheet XML is decompressed and parsed incrementally. Rows are delivered through a bounded channel and mapped as the iterator advances, so callers can use operations such as `take`, `filter`, and `find` without collecting every row. Dropping the iterator stops its worker. Use `MiniExcel::query_with_options()` for worksheet, header, start-cell, and empty-row options.

Typed rows use the same model:

```rust
use miniexcel::MiniExcel;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Record {
    name: String,
}

for record in MiniExcel::query_as::<Record>("book.xlsx")? {
    println!("{}", record?.name);
}
```

`MiniExcel::query()` and `query_as()` accept paths because a worker owns the ZIP archive while the iterator is alive.

> **Memory boundary:** the streaming path keeps workbook metadata, styles, and the shared-string table in memory, plus a small row channel and parser buffers. It does not retain worksheet XML or all worksheet rows. It performs one bounded-memory metadata pass before the streaming pass so every dynamic row has a stable global column schema and explicitly declared style-only rows are preserved even when `<dimension>` is missing or stale. Peak memory can still grow with the shared-string table or a single exceptionally large row, but not with the full worksheet row count.

## Structured Streaming Query

Use the structured stream when a consumer needs source coordinates, formulas, or number formats:

```rust
use miniexcel::MiniExcel;

for row in MiniExcel::query_structured("book.xlsx")? {
    for cell in row?.cells() {
        println!(
            "{} value={:?} formula={:?} format={:?}",
            cell.address(),
            cell.value(),
            cell.formula(),
            cell.number_format()
        );
    }
}
```

Structured rows contain only cells explicitly represented in worksheet XML. Row and column indices are one-based, and `address()` returns the corresponding A1 reference. The sheet name is stored once per row rather than repeated on every cell. `HeaderMode` does not consume the first row for structured reads because source rows are returned as stored.

Formula text and its cached value are preserved separately. MiniExcel does not calculate formulas, expand shared-formula definitions, or guarantee that a producer refreshed cached values. Raw custom and standard built-in number formats are exposed when their style is known.

## Streaming Analytics

Analytics applies strict, serializable predicates and grouped aggregates while source rows stream past:

```rust
use miniexcel::{AggregateOp, AggregateSpec, HeaderMode, MiniExcel, QueryPlan, ReadOptions};

let options = ReadOptions::new().with_header_mode(HeaderMode::FirstRow);
let plan = QueryPlan::new([
    AggregateSpec::count_all("rows"),
    AggregateSpec::column(AggregateOp::Sum, "Amount", "totalAmount"),
])
.with_group_by(["Category", "Region"])
.with_max_groups(10_000);

let result = MiniExcel::analyze_with_options("book.xlsx", &options, &plan)?;
```

Path analytics do not retain source rows. Memory grows with shared strings, styles, parser buffers, and distinct group state. `max_groups` turns high-cardinality grouping into a deterministic error; it is not constant-memory grouping and does not spill to disk.

## RAG Evidence Export

`MiniExcel::export_rag()` streams sparse, source-addressed chunks. Each cell preserves its A1 address, typed cached value, formula text, style ID, and number format. The manifest records the workbook SHA-256, sheet visibility, selected range, chunk policy, output counts, truncation, and formula-cache limitation.

```rust
use miniexcel::{HeaderMode, MiniExcel, RagExportOptions, ReadOptions};

let options = ReadOptions::new().with_header_mode(HeaderMode::FirstRow);
let mut export = MiniExcel::export_rag(
    "book.xlsx",
    &options,
    &RagExportOptions::new().with_chunk_rows(25),
)?;
for chunk in export.by_ref() {
    println!("{}", chunk?.data_range());
}
println!("{}", export.manifest().source_sha256());
```

Hidden and very-hidden sheets require explicit opt-in. JSONL chunks are the canonical evidence format; the manifest is the canonical extraction/provenance record. See [the full analytics and RAG contract](docs/analytics-and-rag.md).

## Dynamic Reading

```rust
use miniexcel::{HeaderMode, MiniExcel, ReadOptions};

let options = ReadOptions::new()
    .with_sheet_name("Data")
    .with_start_cell("B2".parse()?)
    .with_end_cell("E20".parse()?)
    .with_header_mode(HeaderMode::FirstRow);

for row in MiniExcel::query_with_options("book.xlsx", &options)? {
    println!("{:?}", row?["Name"]);
}
```

`HeaderMode::Auto` is the default. It means no header for `query()` and a first-row header for `query_as()`.

Without headers, dynamic keys use the actual Excel column names such as `A`, `B`, and `AA`. Empty rows are retained by default to match MiniExcel. Use `with_ignore_empty_rows(true)` to filter rows whose cells are all empty.

## Typed Reading

```rust
use chrono::NaiveDate;
use miniexcel::MiniExcel;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Release {
    name: String,
    version: u32,
    #[serde(deserialize_with = "miniexcel::serde_helpers::deserialize_date")]
    released_on: NaiveDate,
}

let rows = MiniExcel::query_as::<Release>("book.xlsx")?
    .collect::<miniexcel::Result<Vec<_>>>()?;
```

Serde `rename`, `alias`, `default`, `skip`, and `Option` semantics are supported. MiniExcel-specific column-index attributes are not supported.

## Dynamic Writing

```rust
use miniexcel::{CellValue, DynamicRow, MiniExcel, WriteOptions};

let mut row = DynamicRow::new();
row.insert("Name".to_owned(), CellValue::String("MiniExcel".to_owned()));
row.insert("Version".to_owned(), CellValue::Int(2));

MiniExcel::save_as_with_options(
    "book.xlsx",
    &[row],
    &WriteOptions::new().with_sheet_name("Data"),
)?;
```

Dynamic schemas are the union of row keys in first-seen order. Missing values are written as blank cells. Use `MiniExcel::save_as_with_schema()` when an explicit schema is required, including header-only exports.

## Typed Writing

```rust
use chrono::NaiveDate;
use miniexcel::{MiniExcel, WriteOptions};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct Release {
    name: String,
    #[serde(serialize_with = "miniexcel::serde_helpers::serialize_date_to_excel")]
    released_on: NaiveDate,
}

let values = [Release {
    name: "MiniExcel Rust".to_owned(),
    released_on: NaiveDate::from_ymd_opt(2026, 8, 13).unwrap(),
}];
let options = WriteOptions::new()
    .with_sheet_name("Releases")
    .with_column_format("ReleasedOn", "yyyy-mm-dd");

MiniExcel::save_as_serialized_with_options("releases.xlsx", &values, &options)?;
```

The column-format key is the final Serde field/header name. Typed Serde writing supports structs and vectors of structs; maps and `flatten` are handled through the dynamic API instead.

## Important Semantics

- The default worksheet is the first workbook worksheet, not the active tab.
- Dynamic XLSX numbers with an exact `i64` representation are returned as `CellValue::Int`; other numeric values remain `Float`.
- Excel serial dates cannot always distinguish date-only, time-only, and datetime intent. Dynamic serial values are normalized to `CellValue::DateTime`; ISO values retain the more specific variant when possible.
- Formula expressions are not returned. Reading uses their cached values.
- `MiniExcel::query()` and `query_as()` strictly stream worksheet XML from paths.
- Grouped analytics retain state proportional to distinct groups and stop at `max_groups`.
- RAG exports never recalculate formulas and reject hidden sheets unless explicitly allowed.
- Streaming is synchronous and uses one worker thread per active query. Async I/O is not supported.
- Writing creates new workbooks and overwrites target paths. It cannot modify an existing workbook.

## Not Supported

CSV, `.xls`, `.xlsb`, `.ods`, templates, macros, images, merged-cell operations, formula authoring, a general style system, and editing existing workbooks are not currently supported.

See the [compatibility matrix](docs/compatibility.md) for the current support scope.