# MiniExcel for Rust

[简体中文](README.zh-CN.md)

[![Crates.io](https://img.shields.io/crates/v/miniexcel.svg)](https://crates.io/crates/miniexcel)
[![Documentation](https://docs.rs/miniexcel/badge.svg)](https://docs.rs/miniexcel)
[![License](https://img.shields.io/crates/l/miniexcel.svg)](LICENSE)

A Rust XLSX and CSV reader/writer with bounded-memory streaming, Serde integration, structured cell access, analytics, and RAG exports.

**[Open the MiniExcel Browser Lab](https://mini-software.github.io/MiniExcel-Rust/)** to inspect or generate XLSX files locally. Workbooks never leave the browser.

## Installation

```bash
cargo add miniexcel
```

MiniExcel requires Rust 1.85.0 or later.

## Rust and .NET Stress Test

Keep `MiniExcel-Rust` and the [.NET MiniExcel repository](https://github.com/mini-software/MiniExcel) in sibling directories, then run the shared stress harness from the .NET repository:

```powershell
pwsh ./benchmarks/compare-rust-dotnet.ps1
```

This benchmark compares dynamic streaming Query performance: Rust uses `MiniExcel::query`, and .NET uses `OpenXmlImporter.Query`. Save performance is not included. Both implementations stream the same 100,000-row XLSX workbook. The harness verifies matching row counts and reports elapsed time and peak working set over repeated runs. Results vary by environment, so compare values produced on the same machine.

## Features

- Bounded-memory dynamic and typed worksheet streaming.
- Structured reads with cell addresses, formulas, and number formats.
- Worksheet selection, A1 ranges, headers, and empty-row filtering.
- Serde-based typed reading and writing.
- Streaming dynamic and typed CSV query, save, append, encodings, and dialect options.
- Dynamic workbook creation with stable column ordering.
- Atomic worksheet append to existing XLSX workbooks.
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

### Async Query

Enable the optional `async` feature to consume dynamic or Serde-typed path queries as a
runtime-neutral stream:

```rust
use futures_util::StreamExt;
use miniexcel::{CancellationToken, MiniExcel, ReadOptions};

let cancellation = CancellationToken::new();
let mut rows = MiniExcel::query_async_with_options_and_cancellation(
    "book.xlsx",
    &ReadOptions::new(),
    cancellation.clone(),
)?;

while let Some(row) = rows.next().await {
    println!("{:?}", row?);
}
```

`query_async*()` and `query_as_async*()` use bounded channels and keep blocking ZIP/XML work off
the async executor. They do not turn filesystem access into async I/O and do not require Tokio.
Explicit cancellation yields an error recognized by `Error::is_cancelled()`; dropping the stream
requests cancellation without blocking the executor. Parser initialization or the current row may
finish before background cleanup completes.

## Borrowed Readers And Writers

Use visitor APIs for caller-owned `Read + Seek` sources without materializing all rows or transferring ownership:

```rust
MiniExcel::visit_rows_from_reader(&mut input, &options, |excel_row, row| {
    println!("{excel_row}: {:?}", row);
    Ok(true)
})?;
```

Typed and structured visitors plus sheet names, information, dimensions, and columns are also available from borrowed readers. Borrowed lazy iterators are intentionally not exposed because path iterators move their reader into a worker thread.

Dynamic, explicit-schema, typed, and multi-sheet workbooks can be written to a borrowed `Write + Send` sink with the `*_to_writer` APIs. The library does not close readers or writers. Reader position is unspecified after a call. Writer output begins at the current position and does not truncate existing content, so callers should provide an empty or already-truncated sink.

Existing workbooks can be appended to or replaced through separate borrowed streams with the
`insert*_from_reader_to_writer` APIs. These require a `Read + Seek` source and an empty
`Write + Seek` destination. Both remain open, but output is not atomic and is not rolled back after
a destination error. Source and destination must not alias the same underlying stream.

> **Memory boundary:** the streaming path keeps workbook metadata, styles, a small row channel, and parser buffers in memory. Shared-string tables at least 5 MiB spill to indexed temporary files by default; dropping the iterator removes them. Configure this with `with_shared_string_disk_cache()`, `with_shared_string_cache_size()`, and `with_shared_string_cache_path()`. The directory must already exist. Byte/WASM queries always keep shared strings in memory. Worksheet XML and prior rows are never retained. Peak memory can still grow with a single exceptionally large row, but not with the full worksheet row count.

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

Merged cells retain only their physical top-left value by default. Use `ReadOptions::with_fill_merged_cells(true)` to project that value across the merged range for dynamic, typed, and byte queries. Structured queries remain sparse and expose only physically stored cells.

### Named Table Query

Query an OpenXML table by its metadata name without reading cells outside its declared range:

```rust
let rows = MiniExcel::query_table("book.xlsx", "SalesTable", Some("Data"))?
    .collect::<miniexcel::Result<Vec<_>>>()?;
```

`query_table_as::<T>()` provides Serde mapping, `query_table_bytes()` supports in-memory XLSX data,
and `visit_table_rows*_from_reader()` keeps borrowed readers open. Table names are matched
case-insensitively against the table `name` (not `displayName`). When no sheet is provided, only
the first worksheet is searched. Column names come from table metadata, the physical header row is
skipped unless `headerRowCount="0"`, and the complete declared range is returned, including totals
rows. Path queries retain the existing bounded-memory two-pass worksheet pipeline.

### Comments And Notes

Read threaded comments and legacy notes without reading worksheet rows:

```rust
let comments = MiniExcel::get_comments("book.xlsx", Some("Data"))?;

for thread in comments.threaded_comments() {
    println!("{}: {}", thread.cell(), thread.text());
    for reply in thread.replies() {
        println!("  {}", reply.text());
    }
}
```

`get_comments_from_bytes()` and `get_comments_from_reader()` provide the same metadata for
in-memory and borrowed sources. Results include typed UUIDs and cell references, people, provider
and user IDs, resolution state, local or offset timestamps, replies, and legacy note authors/text.
Compatibility-shadow notes are suppressed only when their `tc={thread-id}` author marker and cell
both match a threaded root; unrelated notes at the same cell remain visible. Comment metadata is
materialized, while worksheet rows are never read.

### CSV

CSV uses a separate streaming provider and keeps dynamic values as strings:

```rust
use miniexcel::{CsvConfiguration, CsvEncoding, CsvReadOptions, HeaderMode, MiniExcel};

let options = CsvReadOptions::new()
    .with_header_mode(HeaderMode::FirstRow)
    .with_configuration(CsvConfiguration::new().with_encoding(CsvEncoding::Gbk));

for row in MiniExcel::query_csv_with_options("data.csv", &options)? {
    println!("{:?}", row?["Name"]);
}
```

Dynamic and Serde APIs support paths, bytes, and borrowed readers/writers. Save and append accept
inferred or explicit schemas. Configuration covers single-byte delimiters, CRLF/LF/CR, UTF-8,
UTF-16LE/BE, GBK, Windows-1252, BOM output, empty-as-null reads, and quoting. Defaults match
MiniExcel where practical: comma, CRLF, UTF-8 BOM, quoted spaces, and empty strings. Records must
have a consistent width. The CLI exposes the same reader as `miniexcel query-csv`.

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

New worksheets freeze the first physical row by default for MiniExcel v1 compatibility. Configure physical row and column counts with `with_freeze_row_count()` and `with_freeze_column_count()`; set both to `0` to disable frozen panes.

AutoFilter dropdowns cover the complete written range by default, including header-only exports. Use `with_auto_filter(false)` to disable them. When headers are disabled, Excel treats the first data row as the filter-header row.

Use `WriteOptions::with_right_to_left(true)` to display a worksheet from right to left. This changes the worksheet view without changing cell coordinates or values.

Enable MiniExcel v1-style fixed column sizing with `with_auto_width(true)`. Data payloads are measured without headers, bounded by `with_min_width()` and `with_max_width()` (defaults `8.42857143` and `200`), and written as fixed widths without `bestFit`. Typed rows perform one additional lightweight Serde pass when this option is enabled. Unlike .NET v1, Rust does not require a separate fast mode.

Set explicit per-column layout by final dynamic/Serde header name with `with_column_width()` and `with_column_hidden()`. Explicit widths are AutoWidth starting minima; hidden state does not remove data and hidden columns remain queryable.

Use `with_wrap_cell_contents(true)` to wrap ordinary body values. Headers, dates, times, durations, and fields with explicit number formats remain unwrapped, matching the MiniExcel v1 style boundary.

Configure body-cell alignment with `with_horizontal_alignment()` and `with_vertical_alignment()`. Horizontal choices are left/general, center, and right; vertical choices are bottom, center, and top. Alignment composes with wrapping and number formats but does not affect headers.

Headers use the MiniExcel v1 visual default: blue background (`#4472C4`), white text, thin borders, no wrapping, left/general horizontal alignment, and bottom vertical alignment. Customize wrapping, RGB background color, and alignment with `HeaderStyle` and `with_header_style()`. Rust emits opaque RGB (`FFRRGGBB`); v1's alpha byte is not preserved.

`TableStyle::Default` is the default cell-style mode and applies thin borders plus header/body options. `TableStyle::None` removes header and body visual styling while retaining date/time/custom number formats and AutoFilter. This option does not create an Excel table or `xl/tables` package parts.

Create multiple worksheets in input order with `MiniExcel::save_as_sheets()`. It returns one data-row count per worksheet:

```rust
let counts = MiniExcel::save_as_sheets(
    "report.xlsx",
    [("Current", current.as_slice()), ("Archive", archive.as_slice())],
    &WriteOptions::new(),
)?;
```

Configure final sheet names as visible, hidden, or very hidden with `with_sheet_visibility(name, SheetVisibility::...)`. Matching is case-insensitive, the first visible sheet becomes active, and unknown names or an all-hidden workbook are rejected before output is created. Hidden states are UI organization, not data protection; hidden worksheets remain queryable.

## Append Or Replace A Worksheet

Use `MiniExcel::insert()` to atomically append a visible worksheet. A missing path creates a new workbook with the same row-count semantics:

```rust
use miniexcel::{CellValue, DynamicRow, InsertOptions, MiniExcel};

let mut row = DynamicRow::new();
row.insert("Name".to_owned(), CellValue::String("Archived".to_owned()));

let count = MiniExcel::insert(
    "book.xlsx",
    &[row],
    &InsertOptions::new().with_sheet_name("Archive"),
)?;
assert_eq!(count, 1);
```

Replace an existing worksheet in place while preserving its workbook identity:

```rust
use miniexcel::ExistingSheetPolicy;

let mut replacement_row = DynamicRow::new();
replacement_row.insert("Name".to_owned(), CellValue::String("Replaced".to_owned()));

let count = MiniExcel::insert(
    "book.xlsx",
    &[replacement_row],
    &InsertOptions::new()
        .with_sheet_name("Archive")
        .with_existing_sheet_policy(ExistingSheetPolicy::Replace),
)?;
```

`insert_with_schema()` accepts a fallible, one-pass dynamic iterator. Source rows and generated donor worksheet XML are disk-spooled; row generation, shared-string conversion, style-ID rebasing, and ZIP output are streamed without retaining the complete worksheet XML. `insert_serialized()` accepts Serde structs. Existing unrelated ZIP entries, worksheet identities, formulas, and cached values are preserved, and an existing workbook is replaced only after the rewritten package validates and syncs.

For separate borrowed streams, use `insert_from_reader_to_writer()`,
`insert_with_schema_from_reader_to_writer()`, or
`insert_serialized_from_reader_to_writer()`. The source must implement `Read + Seek`, the
destination must implement `Write + Seek`, and both remain open. The destination must be empty:
MiniExcel neither truncates it nor rolls it back after an error, so a destination failure can
leave a partial XLSX package. The two handles must not alias the same underlying stream. These
stream APIs preserve the same package behavior as path Insert but do not provide its atomic commit
or post-write validation guarantee.

The default `ExistingSheetPolicy::Reject` rejects duplicate worksheet names case-insensitively. Use `ExistingSheetPolicy::Replace` to replace a worksheet in place while preserving its workbook order, ID, relationship/path, visibility, and active state. The default `TargetRelationshipPolicy::Reject` accepts only a plain target with no worksheet relationships. `RemoveSupported` can remove target-owned tables, drawings with exclusively owned images, comments, VML drawings, and external hyperlinks; pivots, external links, unknown relationships, and shared/global parts are rejected or preserved conservatively. Insert writes XLSX packages, rejects macro-enabled `.xlsm` paths, and rejects `WriteOptions::with_overwrite_file(true)` because workbook replacement is controlled by the insert policy.

Insert preflight limits package entry count, individual and aggregate control XML, XML attribute
size, XML depth, and relationship count. It rejects unsafe or aliased part paths, internal
relationship cycles, duplicate semantic relationship targets, and Strict OOXML packages. Path
Insert holds a cross-process advisory lock and verifies a source SHA-256 fingerprint before commit;
concurrent writers or external source changes return a deterministic conflict instead of silently
overwriting newer content.

Rename an existing worksheet without changing its package identity or position:

```rust
MiniExcel::rename_sheet("book.xlsx", "Sheet1", "Archive")?;
```

Source matching is case-insensitive, including case-only renames. The path update is atomic and
uses the same lock, fingerprint, package validation, permission preservation, and raw ZIP copy
pipeline as Insert. Duplicate and invalid target names are rejected before commit. Worksheet
relationships, IDs, order, visibility, active state, formulas, and defined names are preserved.
Like .NET `AlterSheet`, formula and defined-name text that references the old sheet name is not
rewritten; callers must update those references separately.

Enable the optional `async` feature to feed an existing-workbook Insert from a runtime-neutral
`Stream<Item = miniexcel::Result<DynamicRow>>`:

```rust
use miniexcel::{CancellationToken, InsertOptions, MiniExcel};

let cancellation = CancellationToken::new();
let count = MiniExcel::insert_with_schema_async_with_cancellation(
    "book.xlsx",
    &["Name".to_owned(), "Version".to_owned()],
    rows,
    &InsertOptions::new().with_sheet_name("Async"),
    cancellation,
).await?;
```

The async API uses a bounded channel for row backpressure and isolates blocking XLSX work on a
dedicated thread; ZIP, XML, and filesystem I/O are not async. Explicit cancellation waits for
worker cleanup before returning. Dropping the future requests cooperative cancellation and cleanup
finishes in the background. Cancellation that wins before commit preserves the original workbook;
once atomic replacement begins, cancellation cannot revoke it. No async runtime is enabled by
default, and Tokio is not a dependency.

Appending a formula-free worksheet preserves an existing calculation chain and workbook calculation properties. Replacement removes the complete stale `calcChain` part, relationship, and content-type override, then sets `fullCalcOnLoad` and `forceFullCalc` so Excel recalculates on the next open. MiniExcel does not evaluate or rewrite formulas; formulas and cached values in untouched worksheets remain byte-identical.

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

## Template Writing

`MiniExcel::save_as_template()` fills placeholders in an existing XLSX package while preserving worksheet styles and unrelated workbook parts:

```rust
use miniexcel::{MiniExcel, TemplateOptions};
use serde::Serialize;

#[derive(Serialize)]
struct Report<'a> {
    title: &'a str,
    items: Vec<Item<'a>>,
}

#[derive(Serialize)]
struct Item<'a> {
    name: &'a str,
    score: u32,
}

MiniExcel::save_as_template(
    "report.xlsx",
    "template.xlsx",
    &Report {
        title: "Quarterly Report",
        items: vec![Item { name: "Ada", score: 10 }],
    },
    &TemplateOptions::new(),
)?;
```

Scalar placeholders use `{{title}}`. A row containing `{{items.name}}` is repeated once per array item. Exact number, boolean, and null placeholders become native cell values; mixed text becomes an inline string. Missing variables are blank by default and can be rejected with `with_ignore_missing_variables(false)`. Path output refuses existing files unless `with_overwrite_file(true)` is set. `save_as_template_bytes()` supports in-memory templates.

Version 1 does not implement `@group`, `@if`, parametrized sheet cloning, `$=` formula templates, or formula recalculation.

## Important Semantics

- The default worksheet is the first workbook worksheet, not the active tab.
- Dynamic XLSX numbers with an exact `i64` representation are returned as `CellValue::Int`; other numeric values remain `Float`.
- Excel serial dates cannot always distinguish date-only, time-only, and datetime intent. Dynamic serial values are normalized to `CellValue::DateTime`; ISO values retain the more specific variant when possible.
- Formula expressions are not returned. Reading uses their cached values.
- `MiniExcel::query()` and `query_as()` strictly stream worksheet XML from paths.
- Grouped analytics retain state proportional to distinct groups and stop at `max_groups`.
- RAG exports never recalculate formulas and reject hidden sheets unless explicitly allowed.
- Synchronous streaming queries use one worker thread per active query. Optional async query and Insert APIs use bounded channels around blocking XLSX workers; ZIP/XML/filesystem work is not async I/O.
- Save creates new workbooks and refuses existing target paths by default. `MiniExcel::insert*()` atomically appends or strictly replaces a worksheet in an existing `.xlsx` path, or creates a workbook when the path is missing. `rename_sheet()` atomically changes only existing workbook sheet metadata.

## Not Supported

`.xls`, `.xlsb`, `.ods`, advanced template directives, macros, images, merged-cell operations, formula authoring, a general style system, and arbitrary worksheet copying/reordering/visibility mutation are not currently supported.

See the [compatibility matrix](docs/compatibility.md) for the current support scope and the [MiniExcel v1 Insert migration guide](docs/insert-v1-migration.md) for deliberate differences.