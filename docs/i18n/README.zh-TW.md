<div align="center">

# MiniExcel for Rust

[English](../../README.md) | [简体中文](README.zh-CN.md) | [Français](README.fr.md) | [日本語](README.ja.md) | [Español](README.es.md)

[![Crates.io](https://img.shields.io/crates/v/miniexcel.svg)](https://crates.io/crates/miniexcel)
[![下載量](https://img.shields.io/crates/d/miniexcel.svg)](https://crates.io/crates/miniexcel)
[![文件](https://docs.rs/miniexcel/badge.svg)](https://docs.rs/miniexcel)
[![CI](https://github.com/mini-software/MiniExcel-Rust/actions/workflows/rust.yml/badge.svg)](https://github.com/mini-software/MiniExcel-Rust/actions/workflows/rust.yml)
[![GitHub Stars](https://img.shields.io/github/stars/mini-software/MiniExcel-Rust?logo=github)](https://github.com/mini-software/MiniExcel-Rust)
[![授權](https://img.shields.io/crates/l/miniexcel.svg)](../../LICENSE)

**快速、低記憶體的 XLSX 與 CSV 處理。**

</div>

---

<div align="center">

本專案屬於 [MiniExcel](https://github.com/mini-software/MiniExcel) 專案家族，並以 .NET 版本作為相容性參考。

</div>

---

<div align="center">

**[開啟 Browser Lab](https://mini-software.github.io/MiniExcel-Rust/)**，可在瀏覽器本機檢查或產生 XLSX；workbook 資料不會離開瀏覽器。

</div>

---

## 簡介

MiniExcel for Rust 是支援有界記憶體串流、Serde、資料分析與 RAG 匯出的 XLSX/CSV 讀寫程式庫。

## 安裝

```bash
cargo add miniexcel
```

最低支援 Rust 1.85.0。

## 快速開始

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

可以用自己的檔案直接執行儲存庫內的範例：

```bash
cargo run -p miniexcel --example read -- book.xlsx
cargo run -p miniexcel --example write -- output.xlsx
cargo run -p miniexcel --example rag_export -- book.xlsx
```

## 常用工作流程

所有 API 都回傳 `miniexcel::Result`。以下程式碼可以放進
`fn main() -> miniexcel::Result<()>`。型別化範例需要執行
`cargo add serde --features derive`，Template 範例還需要執行
`cargo add serde_json`。

### 選擇工作表與範圍

`query()` 預設不使用標題列，並以 Excel 欄位字母作為 key。第一列是欄位名稱時，
請使用 `HeaderMode::FirstRow`。起始與結束 cell 都包含在查詢範圍內。

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

迭代器擁有一個有界 worker。透過 `.take(10)` 等方式提前結束後，丟棄迭代器
會停止剩餘的 path query。

### 使用 Serde 反序列化型別化資料列

型別化查詢預設把選取範圍的第一列作為標題，並逐列 mapping。

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

需要把 Excel serial date/time 嚴格轉換為 `chrono` 型別時，請使用
`miniexcel::serde_helpers`。

### 從 Serde 資料建立工作簿

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

Save API 建立新工作簿；除非明確指定 `with_overwrite_file(true)`，否則會拒絕
既有的輸出 path。

### 原子修改既有工作簿

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

Path 修改會鎖定、重寫並驗證工作簿，最後才原子取代原檔。預設拒絕既有的同名
工作表；確實需要取代時，請明確選擇 `ExistingSheetPolicy::Replace`。

### 填入 XLSX Template

在 Template 工作簿中放置 `{{title}}`、`{{items.name}}` 與
`{{items.score}}` 等 placeholder。List 會展開所在的 Template row。

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

### 匯出可追溯的 RAG Chunk

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

每個 chunk 都保留工作表／範圍識別、A1 cell 位址、具型別的快取值、公式文字、
style ID 與 number format。隱藏工作表需要明確的隱私 opt-in。JSONL 與串流
Markdown 輸出詳見 [RAG 合約](../analytics-and-rag.md)。

### 使用儲存庫內 CLI

CLI 是此 workspace 的本機工具，不會以獨立 crate 發佈。

```bash
cargo run -p miniexcel-cli -- sheets book.xlsx
cargo run -p miniexcel-cli -- query book.xlsx --sheet Data --header --start-cell A1 --end-cell F100 --format jsonl
cargo run -p miniexcel-cli -- rag-export book.xlsx --header --chunk-rows 25 --format both --output-prefix ./out/book
```

### 選擇 I/O 方式

| 輸入或輸出 | 主要 API | 記憶體行為 |
| --- | --- | --- |
| 檔案 path | `query*`、`query_as*`、`save_as*`、`insert*` | 有界 row pipeline；大型 shared strings 可使用磁碟索引 |
| XLSX bytes | `query_bytes`、`save_as_bytes`、`visit_rag_chunks_from_bytes` | 工作簿 bytes 保留在記憶體中 |
| 借用 stream | `visit_rows_from_reader`、`save_as_to_writer` | Stream 所有權仍屬於呼叫端 |
| 瀏覽器 | `miniexcel-wasm` 與 [Browser Lab](https://mini-software.github.io/MiniExcel-Rust/) | 本機 WebAssembly；上傳 bytes 與完整下載結果使用瀏覽器記憶體 |

更多可執行程式位於 [`miniexcel/examples`](../../miniexcel/examples)。依賴工作簿編輯、
Template、公式或格式功能前，請先在[相容性矩陣](../compatibility.md)確認精確的支援範圍。

## 主要能力

- 動態、型別化、結構化、Table 與 CSV 有界記憶體查詢。
- 支援 path、byte array 及借用 reader/writer。
- Serde 讀寫、日期時間 helper 與精確 cell mapping。
- 多工作表建立、格式選項與工作表可見性。
- 原子新增/取代、重新命名、排序、複製工作表及修改可見性。
- Template 渲染、條件/群組區塊與 marker 驅動的 cell 合併。
- 具明確限制的串流分組分析。
- 面向 LLM/RAG 的來源位址 JSONL 與 Markdown 匯出。
- 選用的 runtime-neutral async stream；ZIP/XML/檔案系統操作仍為 blocking。

## 關鍵語意

- Path query 串流讀取 worksheet XML，不保留全部資料列。
- 預設工作表是第一張 worksheet，而不是 active tab。
- 一般讀取傳回公式快取值；結構化讀取還提供公式文字與格式。MiniExcel 不計算公式。
- Save 建立新 workbook，預設拒絕既有 path；Insert API 驗證後原子修改 `.xlsx`。
- 大型 shared-string table 可寫入索引暫存檔；byte/WASM query 保留在記憶體中。
- 尚未支援 `.xls`、`.xlsb`、`.ods`、macro、圖片建立、公式計算與通用 style system。

參閱[相容性矩陣](../compatibility.md)、[分析與 RAG 合約](../analytics-and-rag.md)及 [Insert migration guide](../insert-v1-migration.md)。

## Rust 與 .NET 效能比較

將本儲存庫與 [.NET MiniExcel](https://github.com/mini-software/MiniExcel) 放在同層目錄後執行：

```powershell
pwsh ./scripts/compare-dotnet-v1-rust.ps1 -DotNetRepository D:\git\MiniExcel
```

報告寫入 `target/benchmarks/dotnet-v1-vs-rust.json`。只比較同一台機器產生的結果；詳見[測試方法](../dotnet-v1-query-benchmark.md)。
