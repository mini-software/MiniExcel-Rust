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

### .NET 套件

本儲存庫也會建置預覽版 `MiniExcel.Rust` NuGet 套件。它固定依賴 MiniExcel v1 `1.46.0`，
重用其設定與 mapping 型別，並將透過 `MiniExcelRust` 發起的呼叫交給 Rust 原生程式庫執行。

```bash
dotnet add package MiniExcel.Rust --version 0.1.0-preview.2
```

```csharp
using MiniExcelLibs;
using MiniExcelLibs.OpenXml;

var rows = MiniExcelRust.Query(
    "book.xlsx",
    useHeaderRow: true,
    configuration: new OpenXmlConfiguration { IgnoreEmptyRows = true });
```

`MiniExcel.Query` 使用原本的 managed 實作，`MiniExcelRust.Query` 使用 Rust 後端。可透過
`./scripts/dotnet/Test-Package.ps1 -Rid win-x64` 建置並使用本機套件。

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

### 最新 NuGet 結果

最新 Windows x64 測試使用已宣告 dimension 的 100,000 列 x 10 欄 workbook，比較公開的
`MiniExcel 1.46.0`、`.NET` 包裝器 `MiniExcel.Rust 0.1.0-preview.2` 與原生
`MiniExcel Rust 0.4.0`；每個 runtime、每個場景執行 5 個獨立 process。計時前已逐列、逐欄、逐值驗證一致。

| 場景 | Runtime | 耗時中位數 | 列/秒 | 首列延遲 | Managed allocation | Peak working set |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Cold | MiniExcel | 2,145.90 ms | 46,600 | 38.77 ms | 1,167.07 MB | 51.26 MB |
| Cold | MiniExcel.Rust (.NET) | 1,047.67 ms | 95,450 | 12.05 ms | 107.04 MB | 44.59 MB |
| Cold | MiniExcel.Rust | 855.09 ms | 116,947 | 0.42 ms | 不適用 | 3.70 MB |
| Steady | MiniExcel | 4,341.55 ms | 69,100 | 3.95 ms | 3,500.58 MB | 54.74 MB |
| Steady | MiniExcel.Rust (.NET) | 2,972.94 ms | 100,910 | 5.50 ms | 321.09 MB | 48.08 MB |
| Steady | MiniExcel.Rust | 2,143.31 ms | 139,970 | 0.41 ms | 不適用 | 3.79 MB |

完整 Query 中，preview.2 在 Cold 與 Steady 場景分別比 MiniExcel v1 快 2.05 倍與 1.46 倍，
兩個場景的 managed allocation 都減少 90.8%。

結果會受機器與 workbook 影響。請用 `pwsh ./scripts/compare-nuget-v1-rust.ps1` 在代表性 workbook 上重測；
詳見[測試方法](docs/dotnet-v1-query-benchmark.md)。

將本儲存庫與 [.NET MiniExcel](https://github.com/mini-software/MiniExcel) 放在同層目錄後執行：

```powershell
pwsh ./scripts/compare-dotnet-v1-rust.ps1 -DotNetRepository D:\git\MiniExcel
```

報告寫入 `target/benchmarks/dotnet-v1-vs-rust.json`。只比較同一台機器產生的結果；詳見[測試方法](docs/dotnet-v1-query-benchmark.md)。
