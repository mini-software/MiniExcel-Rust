<div align="center">

# MiniExcel for Rust

[English](../../README.md) | [繁體中文](README.zh-TW.md) | [Français](README.fr.md) | [日本語](README.ja.md) | [Español](README.es.md)

[![Crates.io](https://img.shields.io/crates/v/miniexcel.svg)](https://crates.io/crates/miniexcel)
[![下载量](https://img.shields.io/crates/d/miniexcel.svg)](https://crates.io/crates/miniexcel)
[![文档](https://docs.rs/miniexcel/badge.svg)](https://docs.rs/miniexcel)
[![CI](https://github.com/mini-software/MiniExcel-Rust/actions/workflows/rust.yml/badge.svg)](https://github.com/mini-software/MiniExcel-Rust/actions/workflows/rust.yml)
[![GitHub Stars](https://img.shields.io/github/stars/mini-software/MiniExcel-Rust?logo=github)](https://github.com/mini-software/MiniExcel-Rust)
[![许可证](https://img.shields.io/crates/l/miniexcel.svg)](../../LICENSE)

**快速、低内存的 XLSX 与 CSV 处理。**

</div>

---

<div align="center">

本项目属于 [MiniExcel](https://github.com/mini-software/MiniExcel) 项目家族，并以 .NET 版本作为兼容性参考。

</div>

---

<div align="center">

**[打开 Browser Lab](https://mini-software.github.io/MiniExcel-Rust/)**，可在浏览器本地检查或生成 XLSX；工作簿数据不会离开浏览器。

</div>

---

## 简介

MiniExcel for Rust 是支持有界内存流、Serde、数据分析与 RAG 导出的 XLSX/CSV 读写库。

## 安装

```bash
cargo add miniexcel
```

最低支持 Rust 1.85.0。

## 快速开始

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

可以用自己的文件直接运行仓库内的示例：

```bash
cargo run -p miniexcel --example read -- book.xlsx
cargo run -p miniexcel --example write -- output.xlsx
cargo run -p miniexcel --example rag_export -- book.xlsx
```

## 常用工作流

所有 API 都返回 `miniexcel::Result`。以下代码可以放进
`fn main() -> miniexcel::Result<()>`。类型化示例需要执行
`cargo add serde --features derive`，模板示例还需要执行
`cargo add serde_json`。

### 选择工作表和范围

`query()` 默认不使用标题行，并以 Excel 列字母作为 key。第一行是列名时，
请使用 `HeaderMode::FirstRow`。起始和结束 cell 都包含在查询范围内。

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

迭代器拥有一个有界 worker。通过 `.take(10)` 等方式提前结束后，丢弃迭代器
会停止剩余的 path 查询。

### 使用 Serde 反序列化类型化行

类型化查询默认把选择范围的第一行作为标题，并逐行映射。

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

需要把 Excel serial date/time 严格转换为 `chrono` 类型时，请使用
`miniexcel::serde_helpers`。

### 从 Serde 数据创建工作簿

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

Save API 创建新工作簿；除非明确指定 `with_overwrite_file(true)`，否则会拒绝
已有的输出路径。

### 原子修改已有工作簿

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

Path 修改会锁定、重写并验证工作簿，最后才原子替换原文件。默认拒绝已有的同名
工作表；确实需要替换时，请明确选择 `ExistingSheetPolicy::Replace`。

### 填充 XLSX 模板

在模板工作簿中放置 `{{title}}`、`{{items.name}}` 和 `{{items.score}}`
等占位符。List 会展开所在的模板行。

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

### 导出可追溯的 RAG Chunk

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

每个 chunk 都保留工作表/范围身份、A1 cell 地址、带类型的缓存值、公式文本、
style ID 和 number format。隐藏工作表需要明确的隐私 opt-in。JSONL 和流式
Markdown 输出详见 [RAG 契约](../analytics-and-rag.zh-CN.md)。

### 使用仓库内 CLI

CLI 是本 workspace 的本地工具，不作为独立 crate 发布。

```bash
cargo run -p miniexcel-cli -- sheets book.xlsx
cargo run -p miniexcel-cli -- query book.xlsx --sheet Data --header --start-cell A1 --end-cell F100 --format jsonl
cargo run -p miniexcel-cli -- rag-export book.xlsx --header --chunk-rows 25 --format both --output-prefix ./out/book
```

### 选择 I/O 方式

| 输入或输出 | 主要 API | 内存行为 |
| --- | --- | --- |
| 文件路径 | `query*`、`query_as*`、`save_as*`、`insert*` | 有界 row pipeline；大型 shared strings 可使用磁盘索引 |
| XLSX bytes | `query_bytes`、`save_as_bytes`、`visit_rag_chunks_from_bytes` | 工作簿 bytes 保留在内存中 |
| 借用 stream | `visit_rows_from_reader`、`save_as_to_writer` | Stream 所有权仍属于调用方 |
| 浏览器 | `miniexcel-wasm` 和 [Browser Lab](https://mini-software.github.io/MiniExcel-Rust/) | 本地 WebAssembly；上传 bytes 和完整下载结果使用浏览器内存 |

更多可运行程序位于 [`miniexcel/examples`](../../miniexcel/examples)。依赖工作簿编辑、
模板、公式或格式功能前，请先在[兼容性矩阵](../compatibility.zh-CN.md)确认准确的支持范围。

## 主要能力

- 动态、类型化、结构化、Table 与 CSV 有界内存查询。
- 支持路径、字节数组及借用 reader/writer。
- Serde 读写、日期时间 helper 和精确 cell mapping。
- 多工作表创建、格式选项和工作表可见性。
- 原子新增/替换、重命名、排序、复制工作表及修改可见性。
- 模板渲染、条件/分组块和 marker 驱动的单元格合并。
- 带显式限制的流式分组分析。
- 面向 LLM/RAG 的来源地址 JSONL 与 Markdown 导出。
- 可选的 runtime-neutral async stream；ZIP/XML/文件系统操作仍为 blocking。

## 关键语义

- 路径查询流式读取 worksheet XML，不保留全部行。
- 默认工作表是第一张 worksheet，而不是 active tab。
- 普通读取返回公式缓存值；结构化读取还提供公式文本和格式。MiniExcel 不计算公式。
- Save 创建新 workbook，默认拒绝已有路径；Insert API 验证后原子修改 `.xlsx`。
- 大型 shared-string table 可写入索引临时文件；byte/WASM 查询保留在内存中。
- 暂不支持 `.xls`、`.xlsb`、`.ods`、宏、图片创建、公式计算和通用样式系统。

参见[兼容性矩阵](../compatibility.zh-CN.md)、[分析与 RAG 契约](../analytics-and-rag.zh-CN.md)和 [Insert 迁移指南](../insert-v1-migration.zh-CN.md)。

## Rust 与 .NET 性能对比

将本仓库与 [.NET MiniExcel](https://github.com/mini-software/MiniExcel) 放在同级目录后运行：

```powershell
pwsh ./scripts/compare-dotnet-v1-rust.ps1 -DotNetRepository D:\git\MiniExcel
```

报告写入 `target/benchmarks/dotnet-v1-vs-rust.json`。只比较同一台机器产生的结果；详见[测试方法](../dotnet-v1-query-benchmark.zh-CN.md)。
