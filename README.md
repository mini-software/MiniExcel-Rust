<div align="center">

# MiniExcel for Rust

[简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [Français](README.fr.md) | [日本語](README.ja.md) | [Español](README.es.md)

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

### .NET Package

The repository also builds the prerelease `MiniExcel.Rust` NuGet package. It depends on MiniExcel
v1 `1.46.0`, reuses its configuration and mapping types, and routes calls
made through `MiniExcelRust` to the Rust native library.

```bash
dotnet add package MiniExcel.Rust --prerelease
```

```csharp
using MiniExcelLibs;
using MiniExcelLibs.OpenXml;

var rows = MiniExcelRust.Query(
    "book.xlsx",
    useHeaderRow: true,
    configuration: new OpenXmlConfiguration { IgnoreEmptyRows = true });
```

Use `MiniExcel.Query` for the original managed implementation and `MiniExcelRust.Query` for the
Rust-backed implementation. Build and consume a local package with
`./scripts/dotnet/Test-Package.ps1 -Rid win-x64`.

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

### Latest NuGet Results

The latest Windows x64 run compared public `MiniExcel 1.46.0`, the
`MiniExcel.Rust 0.1.0-preview.1` .NET wrapper, and native `MiniExcel Rust 0.4.0` over 100,000 rows
x 10 columns, using five fresh processes per runtime and scenario. Every row, column, and
normalized value matched before timing.

| Scenario | Runtime | Median elapsed | Rows/s | First row | Managed allocation | Peak working set |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Cold | MiniExcel | 2,598.50 ms | 38,484 | 1,051.97 ms | 2,541.93 MB | 50.93 MB |
| Cold | MiniExcel.Rust (.NET) | 1,899.42 ms | 52,648 | 450.85 ms | 289.36 MB | 49.64 MB |
| Cold | MiniExcel.Rust | 1,220.82 ms | 81,912 | 537.17 ms | n/a | 4.14 MB |
| Steady | MiniExcel | 5,997.76 ms | 50,019 | 616.40 ms | 7,625.15 MB | 52.80 MB |
| Steady | MiniExcel.Rust (.NET) | 4,202.10 ms | 71,393 | 442.59 ms | 868.05 MB | 48.79 MB |
| Steady | MiniExcel.Rust | 3,344.20 ms | 89,708 | 467.30 ms | n/a | 4.20 MB |

Results are machine-specific. Run `pwsh ./scripts/compare-nuget-v1-rust.ps1` on representative
workbooks; see the [benchmark methodology](docs/dotnet-v1-query-benchmark.md).

Place this repository beside [.NET MiniExcel](https://github.com/mini-software/MiniExcel), then run:

```powershell
pwsh ./scripts/compare-dotnet-v1-rust.ps1 -DotNetRepository D:\git\MiniExcel
```

The report is written to `target/benchmarks/dotnet-v1-vs-rust.json`. Compare only results produced on the same machine; see the [benchmark methodology](docs/dotnet-v1-query-benchmark.md).
