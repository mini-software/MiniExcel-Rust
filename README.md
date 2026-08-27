<div align="center">

# MiniExcel for Rust

[简体中文](docs/i18n/README.zh-CN.md) | [繁體中文](docs/i18n/README.zh-TW.md) | [Français](docs/i18n/README.fr.md) | [日本語](docs/i18n/README.ja.md) | [Español](docs/i18n/README.es.md)

[![Crates.io](https://img.shields.io/crates/v/miniexcel.svg)](https://crates.io/crates/miniexcel)
[![Downloads](https://img.shields.io/crates/d/miniexcel.svg)](https://crates.io/crates/miniexcel)
[![Documentation](https://docs.rs/miniexcel/badge.svg)](https://docs.rs/miniexcel)
[![CI](https://github.com/mini-software/MiniExcel-Rust/actions/workflows/rust.yml/badge.svg)](https://github.com/mini-software/MiniExcel-Rust/actions/workflows/rust.yml)
[![GitHub stars](https://img.shields.io/github/stars/mini-software/MiniExcel-Rust?logo=github)](https://github.com/mini-software/MiniExcel-Rust)
[![License](https://img.shields.io/crates/l/miniexcel.svg)](LICENSE)

**Fast XLSX and CSV processing with bounded-memory streaming.**

</div>

---

<div align="center">

Part of the [MiniExcel](https://github.com/mini-software/MiniExcel) project family, with the .NET library as its compatibility reference.

</div>

---

<div align="center">

**[Open the Browser Lab](https://mini-software.github.io/MiniExcel-Rust/)** to inspect or generate XLSX files locally. Workbook data stays in the browser.

</div>

---

## Introduction

MiniExcel for Rust is an XLSX and CSV reader/writer with bounded-memory streaming, Serde support, analytics, and RAG exports.

## Install

```bash
cargo add miniexcel
```

Requires Rust 1.85.0 or later.

## Quick Start

```rust
use miniexcel::MiniExcel;

for row in MiniExcel::query("book.xlsx")? {
    println!("{:?}", row?["A"]);
}
```

```rust
use miniexcel::{CellValue, DynamicRow, MiniExcel};

let mut row = DynamicRow::new();
row.insert("Name".into(), CellValue::String("MiniExcel".into()));
MiniExcel::save_as("book.xlsx", &[row])?;
```

Run the included examples against your own files:

```bash
cargo run -p miniexcel --example read -- book.xlsx
cargo run -p miniexcel --example write -- output.xlsx
cargo run -p miniexcel --example rag_export -- book.xlsx
```

## Common Workflows

All APIs return `miniexcel::Result`. The snippets below can be placed inside a
`fn main() -> miniexcel::Result<()>`. Typed examples require
`cargo add serde --features derive`; the template example also requires
`cargo add serde_json`.

### Select A Worksheet And Range

`query()` is headerless by default and uses Excel column letters as keys. Use
`HeaderMode::FirstRow` when the first selected row contains names. Start and
end cells are inclusive.

```rust
use miniexcel::{HeaderMode, MiniExcel, ReadOptions};

let options = ReadOptions::new()
    .with_sheet_name("Data")
    .with_header_mode(HeaderMode::FirstRow)
    .with_start_cell("A1".parse()?)
    .with_end_cell("F100".parse()?)
    .with_ignore_empty_rows(true);

for row in MiniExcel::query_with_options("book.xlsx", &options)? {
    let row = row?;
    println!("{:?}", row["Name"]);
}
```

The iterator owns a bounded worker. Stopping iteration early, for example with
`.take(10)`, stops the remaining path query when the iterator is dropped.

### Deserialize Typed Rows With Serde

Typed queries treat the first selected row as headers by default and map one
row at a time.

```rust
use miniexcel::MiniExcel;

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Release {
    name: String,
    version: u32,
}

for release in MiniExcel::query_as::<Release>("book.xlsx")? {
    let release = release?;
    println!("{} {}", release.name, release.version);
}
```

See `miniexcel::serde_helpers` when Excel serial dates or times need strict
`chrono` conversion.

### Create A Workbook From Serde Rows

```rust
use miniexcel::{MiniExcel, WriteOptions};

#[derive(serde::Serialize)]
#[serde(rename_all = "PascalCase")]
struct Release<'a> {
    name: &'a str,
    version: u32,
}

let rows = [
    Release { name: "MiniExcel", version: 1 },
    Release { name: "MiniExcel Rust", version: 4 },
];
let options = WriteOptions::new()
    .with_sheet_name("Releases")
    .with_auto_width(true);
MiniExcel::save_as_serialized_with_options("releases.xlsx", &rows, &options)?;
```

Save APIs create a workbook and refuse an existing output path unless
`with_overwrite_file(true)` is explicit.

### Modify An Existing Workbook Atomically

```rust
use miniexcel::{InsertOptions, MiniExcel};

let inserted = MiniExcel::insert_serialized(
    "book.xlsx",
    &rows,
    &InsertOptions::new().with_sheet_name("Archive"),
)?;
MiniExcel::rename_sheet("book.xlsx", "Archive", "History")?;
MiniExcel::reorder_sheet("book.xlsx", "History", 0)?;
println!("inserted {inserted} rows");
```

Path mutations lock, rewrite, validate, and atomically replace the workbook.
An existing worksheet name is rejected by default; select
`ExistingSheetPolicy::Replace` explicitly when replacement is intended.

### Fill An XLSX Template

Put placeholders such as `{{title}}`, `{{items.name}}`, and `{{items.score}}`
in the template workbook. A list expands its template row.

```rust
use miniexcel::{MiniExcel, TemplateOptions};
use serde_json::json;

MiniExcel::save_as_template(
    "report.xlsx",
    "template.xlsx",
    &json!({
        "title": "Quarterly report",
        "items": [
            { "name": "Ada", "score": 10 },
            { "name": "Linus", "score": 20 }
        ]
    }),
    &TemplateOptions::new(),
)?;
```

### Export Source-Grounded RAG Chunks

```rust
use miniexcel::{HeaderMode, MiniExcel, RagExportOptions, ReadOptions};

let read = ReadOptions::new().with_header_mode(HeaderMode::FirstRow);
let rag = RagExportOptions::new().with_chunk_rows(25).with_max_rows(500);
let mut export = MiniExcel::export_rag("book.xlsx", &read, &rag)?;

for chunk in export.by_ref() {
    let chunk = chunk?;
    println!("{} {}", chunk.chunk_id(), chunk.data_range());
}
println!("source SHA-256: {}", export.manifest().source_sha256());
```

Each chunk preserves sheet/range identity, A1 cell addresses, typed cached
values, formula text, style IDs, and number formats. Hidden sheets require an
explicit privacy opt-in. See the [RAG contract](docs/analytics-and-rag.md) for
JSONL and streaming Markdown output.

### Use The Repository CLI

The CLI is a local workspace tool and is not published as a separate crate.

```bash
cargo run -p miniexcel-cli -- sheets book.xlsx
cargo run -p miniexcel-cli -- query book.xlsx --sheet Data --header --start-cell A1 --end-cell F100 --format jsonl
cargo run -p miniexcel-cli -- rag-export book.xlsx --header --chunk-rows 25 --format both --output-prefix ./out/book
```

### Choose An I/O Shape

| Input or output | Main APIs | Memory behavior |
| --- | --- | --- |
| File path | `query*`, `query_as*`, `save_as*`, `insert*` | Bounded row pipeline; large shared strings may use a disk index |
| XLSX bytes | `query_bytes`, `save_as_bytes`, `visit_rag_chunks_from_bytes` | Workbook bytes remain in memory |
| Borrowed streams | `visit_rows_from_reader`, `save_as_to_writer` | Caller retains stream ownership |
| Browser | `miniexcel-wasm` and [Browser Lab](https://mini-software.github.io/MiniExcel-Rust/) | Local WebAssembly; uploaded bytes and completed downloads use browser memory |

Additional runnable programs are under [`miniexcel/examples`](miniexcel/examples).
Use the [compatibility matrix](docs/compatibility.md) to check exact support
before depending on workbook editing, templates, formulas, or formatting.

## Capabilities

- Bounded-memory dynamic, typed, structured, table, and CSV queries.
- Path, byte-array, and borrowed reader/writer APIs.
- Serde reading and writing; date/time helpers and exact-cell mapping.
- Multi-sheet creation, formatting options, and worksheet visibility.
- Atomic worksheet insert/replace, rename, reorder, copy, and visibility changes.
- Template rendering, conditional/group blocks, and marker-driven cell merging.
- Streaming grouped analytics with explicit limits.
- Source-addressed JSONL and Markdown exports for LLM/RAG workflows.
- Optional runtime-neutral async streams; ZIP/XML/filesystem work remains blocking.

## Key Semantics

- Path queries stream worksheet XML and do not retain all rows.
- The default worksheet is the first worksheet, not the active tab.
- Formula reads return cached values; structured reads also expose formula text and formats. MiniExcel does not calculate formulas.
- Save creates a new workbook and refuses existing paths by default. Insert APIs modify `.xlsx` files atomically after validation.
- Large shared-string tables may spill to indexed temporary files; byte/WASM queries keep them in memory.
- Not supported: `.xls`, `.xlsb`, `.ods`, macros, image authoring, formula calculation, or a general style system.

See the [compatibility matrix](docs/compatibility.md), [analytics and RAG contract](docs/analytics-and-rag.md), and [Insert migration guide](docs/insert-v1-migration.md).

## Rust And .NET Benchmark

Place this repository beside [.NET MiniExcel](https://github.com/mini-software/MiniExcel), then run:

```powershell
pwsh ./scripts/compare-dotnet-v1-rust.ps1 -DotNetRepository D:\git\MiniExcel
```

The report is written to `target/benchmarks/dotnet-v1-vs-rust.json`. Compare only results produced on the same machine; see the [benchmark methodology](docs/dotnet-v1-query-benchmark.md).
