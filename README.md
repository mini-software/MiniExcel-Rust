# MiniExcel Rust XLSX MVP

This directory contains an experimental Rust implementation of MiniExcel's basic XLSX read and write workflows. It is a research track and does not replace the .NET packages. The core crate is ready for crates.io packaging but has not been published yet.

[简体中文](README.zh-CN.md)

**[Open the MiniExcel Browser Lab](https://mini-software.github.io/MiniExcel-Rust/)** to inspect or generate XLSX files locally in your browser. Uploaded workbooks never leave the browser.

## Status

The MVP currently supports:

- Reading `.xlsx` files from paths.
- Bounded-memory worksheet streaming through `MiniExcel::query()` and `MiniExcel::query_as()`.
- Sparse structure-preserving streaming through `MiniExcel::query_structured()`.
- Versioned filter/group/aggregate plans with memory bounded by an explicit maximum group count.
- Addressed JSONL and streamable Markdown chunks with SHA-256 manifests for LLM/RAG ingestion and source grounding.
- Listing worksheets with index, type, and visibility metadata, and selecting a worksheet by name.
- Listing selected column names with header and A1 start-cell semantics.
- Dynamic rows with stable column order and optional header rows.
- Typed row deserialization through Serde.
- Inclusive A1 start/end ranges, header trimming, and optional empty-row filtering.
- Creating new `.xlsx` workbooks from dynamic rows or Serde structs.
- Worksheet selection for reads and path-based workbook output.
- Strings, booleans, integers, floating-point values, empty cells, Excel errors, dates, times, datetimes, and durations.
- A Web Worker-based Browser Lab for local row inspection, grouped analysis, and RAG export.

The implementation uses Rust 2024 with an MSRV of Rust 1.85.0.

## Build

Run commands from the repository root:

```bash
cargo +1.85.0 check --workspace --all-targets --locked
cargo +1.85.0 test --workspace --all-targets --locked
```

The workspace lockfile is committed so CI and local research use the same dependency graph.

## Package

[crates.io](https://crates.io/) is Rust's equivalent of NuGet. The `miniexcel` name is currently available, and only the core library is configured for publication; the local CLI and WebAssembly adapter remain private workspace packages.

Create and verify the same archive that crates.io will receive:

```bash
cargo +1.85.0 package --manifest-path miniexcel/Cargo.toml --locked
```

The package is written to `target/package/miniexcel-0.1.0.crate`. Publishing requires a crates.io account with a verified email and an API token, and should only be done after reviewing the archive:

```bash
cargo login
cargo +1.85.0 publish --manifest-path miniexcel/Cargo.toml --locked
```

## Local CLI

Run the CLI from the repository root:

```bash
cargo +1.85.0 run -p miniexcel-cli -- --help
```

If the current directory is already `miniexcel-cli`, Cargo discovers the package and its parent workspace automatically:

```bash
cargo +1.85.0 run -- --help
```

`--manifest-path` is always resolved relative to the current directory. To name the workspace manifest explicitly from `miniexcel-cli`, use `--manifest-path ../Cargo.toml`.

List sheets and inspect rows:

```bash
cargo +1.85.0 run -p miniexcel-cli -- sheets tests/data/xlsx/TestMultiSheet.xlsx

cargo +1.85.0 run -p miniexcel-cli -- query tests/data/xlsx/TestDynamicQueryBasic.xlsx --header --limit 5
```

From `miniexcel-cli`, the equivalent commands are:

```bash
cargo +1.85.0 run -- sheets ../tests/data/xlsx/TestMultiSheet.xlsx

cargo +1.85.0 run -- query ../tests/data/xlsx/TestDynamicQueryBasic.xlsx --header --limit 5
```

`query` supports `--sheet`, `--header`, `--start-cell`, `--end-cell`, `--ignore-empty-rows`, and `--format table|json|jsonl`. It prints at most 20 rows by default. Use `--limit 0 --format jsonl` for unbounded streaming output; JSON and table output collect the selected rows before rendering.

Run a versioned analysis plan or export RAG evidence files:

```bash
cargo +1.85.0 run -p miniexcel-cli -- analyze book.xlsx --header --plan plan.json --format json

cargo +1.85.0 run -p miniexcel-cli -- rag-export book.xlsx --header --chunk-rows 25 --output-prefix ./out/book

cargo +1.85.0 run -p miniexcel-cli -- rag-export book.xlsx --header --format both --output-prefix ./out/book
```

See [Streaming Analytics and RAG Export](docs/analytics-and-rag.md) for the query contracts and [Streaming Markdown and anydoc comparison](docs/markdown-streaming.md) for appendable output, memory boundaries, and the comparison harness.

## Browser WebAssembly

The [Browser Lab](https://mini-software.github.io/MiniExcel-Rust/) uses a Web Worker and a reusable `miniexcel-wasm` workbook session. It previews bounded rows, runs grouped plans, and builds addressed JSONL, Markdown, and manifest downloads entirely in the browser. Uploaded workbooks never leave the device. Build and test it from `web-demo`:

```bash
npm ci
npm run build
npm run test:e2e
```

The build requires the `wasm32-unknown-unknown` target and `wasm-bindgen-cli 0.2.127`. The Rust workflow validates the WASM build and Playwright desktop/mobile behavior.

Create and read back a workbook, or run both parity adapters:

```bash
cargo +1.85.0 run -p miniexcel-cli -- write-demo ./tmp/miniexcel-demo.xlsx

cargo +1.85.0 run -p miniexcel-cli -- parity --repo-root ../MiniExcel
```

From `miniexcel-cli`, these become:

```bash
cargo +1.85.0 run -- write-demo ./tmp/miniexcel-demo.xlsx

cargo +1.85.0 run -- parity --repo-root ../../MiniExcel
```

After one build, the executable is `target/debug/miniexcel` (`.exe` on Windows).

## .NET Parity

.NET and Rust consume the same versioned behavior contract at `tests/data/contracts/xlsx-parity-v1.json`. It covers the common dynamic and typed query surface with the same XLSX fixtures and canonical expected values.

```bash
cargo +1.85.0 test -p miniexcel --test parity_contract --locked
dotnet test ../MiniExcel/tests/MiniExcel.OpenXml.Tests/MiniExcel.OpenXml.Tests.csproj --framework net10.0 --filter "FullyQualifiedName~RustParityContractTests"
```

Both commands must pass for a behavior to be considered equivalent. See [Compatibility and research notes](docs/compatibility.md#net-parity-contract) for normalization rules and the explicit version 1 scope.

## Public API

`MiniExcel` is the main public behavior entry point. Reader, writer, and ZIP/XML parser types are internal. Root exports contain row/configuration contracts plus versioned analytics and RAG support types. Date/time Serde adapters are available under `serde_helpers`.

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
# Ok::<(), miniexcel::Error>(())
```

## Simple Streaming Query

The closest Rust equivalent to `MiniExcel.Query` is an iterator:

```rust
use miniexcel::MiniExcel;

for row in MiniExcel::query("book.xlsx")? {
    let row = row?;
    println!("{:?}", row["A"]);
}
# Ok::<(), miniexcel::Error>(())
```

Worksheet XML is decompressed and parsed incrementally. Rows are delivered through a bounded channel and mapped as the iterator advances, so callers can use operations such as `take`, `filter`, and `find` without collecting every row. Dropping the iterator stops its worker. Use `MiniExcel::query_with_options()` for worksheet, header, start-cell, and empty-row options.

Typed rows use the same model:

```rust
# use serde::Deserialize;
use miniexcel::MiniExcel;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Record {
    name: String,
}

for record in MiniExcel::query_as::<Record>("book.xlsx")? {
    println!("{}", record?.name);
}
# Ok::<(), miniexcel::Error>(())
```

`MiniExcel::query()` and `query_as()` accept paths because a worker owns the ZIP archive while the iterator is alive. Their concrete iterator types are intentionally hidden.

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
# Ok::<(), miniexcel::Error>(())
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
# Ok::<(), miniexcel::Error>(())
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
# Ok::<(), miniexcel::Error>(())
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
# Ok::<(), miniexcel::Error>(())
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
# Ok::<(), miniexcel::Error>(())
```

Serde `rename`, `alias`, `default`, `skip`, and `Option` semantics are supported. MiniExcel-specific column-index attributes are not part of the MVP.

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
# Ok::<(), miniexcel::Error>(())
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
# Ok::<(), miniexcel::Error>(())
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
- Streaming is synchronous and uses one worker thread per active query. Async I/O is not part of the MVP.
- Writing creates new workbooks and overwrites target paths. It cannot modify an existing workbook.

## Non-Goals For This MVP

CSV, `.xls`, `.xlsb`, `.ods`, templates, macros, images, merged-cell operations, formula authoring, a general style system, and editing existing workbooks are deferred.

See [Compatibility and research notes](docs/compatibility.md) for dependency choices and behavior mapping.