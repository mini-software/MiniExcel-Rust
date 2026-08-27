<div align="center">

# MiniExcel for Rust

[English](README.md) | [简体中文](README.zh-CN.md) | [Français](README.fr.md) | [日本語](README.ja.md) | [Español](README.es.md)

[![Crates.io](https://img.shields.io/crates/v/miniexcel.svg)](https://crates.io/crates/miniexcel)
[![下載量](https://img.shields.io/crates/d/miniexcel.svg)](https://crates.io/crates/miniexcel)
[![文件](https://docs.rs/miniexcel/badge.svg)](https://docs.rs/miniexcel)
[![CI](https://github.com/mini-software/MiniExcel-Rust/actions/workflows/rust.yml/badge.svg)](https://github.com/mini-software/MiniExcel-Rust/actions/workflows/rust.yml)
[![GitHub Stars](https://img.shields.io/github/stars/mini-software/MiniExcel-Rust?logo=github)](https://github.com/mini-software/MiniExcel-Rust)
[![授權](https://img.shields.io/crates/l/miniexcel.svg)](LICENSE)

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

參閱[相容性矩陣](docs/compatibility.md)、[分析與 RAG 合約](docs/analytics-and-rag.md)及 [Insert migration guide](docs/insert-v1-migration.md)。

## Rust 與 .NET 效能比較

將本儲存庫與 [.NET MiniExcel](https://github.com/mini-software/MiniExcel) 放在同層目錄後執行：

```powershell
pwsh ./scripts/compare-dotnet-v1-rust.ps1 -DotNetRepository D:\git\MiniExcel
```

報告寫入 `target/benchmarks/dotnet-v1-vs-rust.json`。只比較同一台機器產生的結果；詳見[測試方法](docs/dotnet-v1-query-benchmark.md)。
