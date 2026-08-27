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

Place this repository beside [.NET MiniExcel](https://github.com/mini-software/MiniExcel), then run:

```powershell
pwsh ./scripts/compare-dotnet-v1-rust.ps1 -DotNetRepository D:\git\MiniExcel
```

The report is written to `target/benchmarks/dotnet-v1-vs-rust.json`. Compare only results produced on the same machine; see the [benchmark methodology](docs/dotnet-v1-query-benchmark.md).
