<div align="center">

# MiniExcel for Rust

[English](README.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [Français](README.fr.md) | [Español](README.es.md)

[![Crates.io](https://img.shields.io/crates/v/miniexcel.svg)](https://crates.io/crates/miniexcel)
[![ダウンロード](https://img.shields.io/crates/d/miniexcel.svg)](https://crates.io/crates/miniexcel)
[![ドキュメント](https://docs.rs/miniexcel/badge.svg)](https://docs.rs/miniexcel)
[![CI](https://github.com/mini-software/MiniExcel-Rust/actions/workflows/rust.yml/badge.svg)](https://github.com/mini-software/MiniExcel-Rust/actions/workflows/rust.yml)
[![GitHub Stars](https://img.shields.io/github/stars/mini-software/MiniExcel-Rust?logo=github)](https://github.com/mini-software/MiniExcel-Rust)
[![ライセンス](https://img.shields.io/crates/l/miniexcel.svg)](LICENSE)

**高速かつ省メモリな XLSX/CSV 処理。**

</div>

---

<div align="center">

[MiniExcel](https://github.com/mini-software/MiniExcel) プロジェクトファミリーの一員であり、.NET ライブラリを互換性の基準としています。

</div>

---

<div align="center">

**[Browser Lab を開く](https://mini-software.github.io/MiniExcel-Rust/)** と、XLSX をブラウザー内で確認・生成できます。データはブラウザー外へ送信されません。

</div>

---

## はじめに

MiniExcel for Rust は、有界メモリストリーミング、Serde、分析、RAG エクスポートに対応する XLSX/CSV reader/writer です。

## インストール

```bash
cargo add miniexcel
```

Rust 1.85.0 以降が必要です。

### .NET パッケージ

このリポジトリはプレビュー版 `MiniExcel.Rust` NuGet パッケージもビルドします。MiniExcel
v1 `1.46.0` に固定して依存し、その設定型と mapping 型を再利用しながら、
`MiniExcelRust` の呼び出しを Rust ネイティブライブラリで実行します。

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

`MiniExcel.Query` は元の managed 実装を、`MiniExcelRust.Query` は Rust backend を使用します。
ローカルパッケージは `./scripts/dotnet/Test-Package.ps1 -Rid win-x64` でビルドして検証できます。

## クイックスタート

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

## 主な機能

- 動的、型付き、構造化、Table、CSV の有界メモリクエリ。
- Path、bytes、borrowed reader/writer API。
- Serde 読み書き、日時 helper、exact-cell mapping。
- 複数 worksheet 作成、format option、visibility。
- Worksheet の原子的な追加/置換、rename、reorder、copy、visibility 変更。
- Template、条件/グループ block、marker ベースの cell merge。
- 明示的な上限を持つ streaming grouped analytics。
- LLM/RAG 向けソースアドレス付き JSONL・Markdown export。
- 任意の runtime-neutral async stream。ZIP/XML/filesystem 処理は blocking のまま。

## 重要なセマンティクス

- Path query は worksheet XML をストリーミングし、全行を保持しない。
- 既定 worksheet は最初の worksheet で、active tab ではない。
- 通常の読み取りは式の cache 値を返し、structured read は式テキストと format も返す。MiniExcel は式を計算しない。
- Save は新しい workbook を作成し、既定で既存 path を拒否する。Insert は検証後に `.xlsx` を原子的に変更する。
- 大きな shared-string table は indexed temporary file に spill できる。Bytes/WASM query はメモリに保持する。
- 未対応：`.xls`、`.xlsb`、`.ods`、macro、画像作成、式計算、汎用 style system。

[互換性マトリクス](docs/compatibility.md)、[分析/RAG contract](docs/analytics-and-rag.md)、[Insert migration guide](docs/insert-v1-migration.md) を参照してください。

## Rust と .NET の Benchmark

### 最新 NuGet 結果

最新の Windows x64 テストでは、100,000 行 x 10 列の workbook で `MiniExcel 1.46.0`、
.NET wrapper `MiniExcel.Rust 0.1.0-preview.1`、native `MiniExcel Rust 0.4.0` を比較しました。
runtime とシナリオごとに 5 個の新規 process を使用し、計測前にすべての値を検証しています。

| シナリオ | Runtime | 中央値 | 行/秒 | 最初の行 | Managed allocation | Peak working set |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Cold | MiniExcel | 2,598.50 ms | 38,484 | 1,051.97 ms | 2,541.93 MB | 50.93 MB |
| Cold | MiniExcel.Rust (.NET) | 1,899.42 ms | 52,648 | 450.85 ms | 289.36 MB | 49.64 MB |
| Cold | MiniExcel.Rust | 1,220.82 ms | 81,912 | 537.17 ms | n/a | 4.14 MB |
| Steady | MiniExcel | 5,997.76 ms | 50,019 | 616.40 ms | 7,625.15 MB | 52.80 MB |
| Steady | MiniExcel.Rust (.NET) | 4,202.10 ms | 71,393 | 442.59 ms | 868.05 MB | 48.79 MB |
| Steady | MiniExcel.Rust | 3,344.20 ms | 89,708 | 467.30 ms | n/a | 4.20 MB |

結果はマシンに依存します。代表的な workbook で `pwsh ./scripts/compare-nuget-v1-rust.ps1` を実行し、
[測定方法](docs/dotnet-v1-query-benchmark.md)を参照してください。

このリポジトリを [.NET MiniExcel](https://github.com/mini-software/MiniExcel) と隣接配置して実行します。

```powershell
pwsh ./scripts/compare-dotnet-v1-rust.ps1 -DotNetRepository D:\git\MiniExcel
```

レポートは `target/benchmarks/dotnet-v1-vs-rust.json` に出力されます。同一マシンの結果だけを比較し、[測定方法](docs/dotnet-v1-query-benchmark.md)を参照してください。
