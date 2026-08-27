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

このリポジトリを [.NET MiniExcel](https://github.com/mini-software/MiniExcel) と隣接配置して実行します。

```powershell
pwsh ./scripts/compare-dotnet-v1-rust.ps1 -DotNetRepository D:\git\MiniExcel
```

レポートは `target/benchmarks/dotnet-v1-vs-rust.json` に出力されます。同一マシンの結果だけを比較し、[測定方法](docs/dotnet-v1-query-benchmark.md)を参照してください。
