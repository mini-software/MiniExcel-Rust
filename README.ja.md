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

同梱 example は自分のファイルですぐに実行できます。

```bash
cargo run -p miniexcel --example read -- book.xlsx
cargo run -p miniexcel --example write -- output.xlsx
cargo run -p miniexcel --example rag_export -- book.xlsx
```

## 一般的なワークフロー

すべての API は `miniexcel::Result` を返します。以下のコードは
`fn main() -> miniexcel::Result<()>` 内に配置できます。型付き example には
`cargo add serde --features derive`、Template example にはさらに
`cargo add serde_json` が必要です。

### Worksheet と範囲を選択する

`query()` は既定では headerless で、Excel の列文字を key として使用します。
選択範囲の先頭行が列名なら `HeaderMode::FirstRow` を使用します。Start cell と
end cell はどちらも範囲に含まれます。

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

Iterator は有界 worker を所有します。たとえば `.take(10)` で早期終了すると、
iterator の drop 時に残りの path query も停止します。

### Serde で型付き行へ Deserialize する

型付き query は既定で選択範囲の先頭行を header として扱い、1 行ずつ mapping
します。

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

Excel serial date/time を `chrono` 型へ厳密に変換する場合は
`miniexcel::serde_helpers` を使用してください。

### Serde 行から Workbook を作成する

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

Save API は新しい workbook を作成します。`with_overwrite_file(true)` を明示
しない限り、既存の出力 path は拒否されます。

### 既存 Workbook を原子的に変更する

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

Path mutation は workbook を lock、rewrite、validate した後に原子的に置換します。
同名 worksheet は既定で拒否されます。置換する場合は
`ExistingSheetPolicy::Replace` を明示してください。

### XLSX Template を埋める

Template workbook に `{{title}}`、`{{items.name}}`、`{{items.score}}` などの
placeholder を配置します。List はその template row を展開します。

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

### Source を追跡できる RAG Chunk を Export する

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

各 chunk は worksheet/range identity、A1 cell address、型付き cache value、
formula text、style ID、number format を保持します。Hidden sheet には明示的な
privacy opt-in が必要です。JSONL と streaming Markdown の詳細は
[RAG contract](docs/analytics-and-rag.md)を参照してください。

### リポジトリ内 CLI を使用する

CLI は workspace 内のローカル tool であり、独立した crate としては公開されません。

```bash
cargo run -p miniexcel-cli -- sheets book.xlsx
cargo run -p miniexcel-cli -- query book.xlsx --sheet Data --header --start-cell A1 --end-cell F100 --format jsonl
cargo run -p miniexcel-cli -- rag-export book.xlsx --header --chunk-rows 25 --format both --output-prefix ./out/book
```

### I/O 形式を選択する

| 入出力 | 主な API | メモリ動作 |
| --- | --- | --- |
| File path | `query*`、`query_as*`、`save_as*`、`insert*` | 有界 row pipeline。大きな shared strings は disk index を使用可能 |
| XLSX bytes | `query_bytes`、`save_as_bytes`、`visit_rag_chunks_from_bytes` | Workbook bytes はメモリ内に保持 |
| Borrowed stream | `visit_rows_from_reader`、`save_as_to_writer` | Stream の所有権は caller が保持 |
| Browser | `miniexcel-wasm` と [Browser Lab](https://mini-software.github.io/MiniExcel-Rust/) | ローカル WebAssembly。Upload bytes と完成した download は browser memory を使用 |

実行可能な追加プログラムは [`miniexcel/examples`](miniexcel/examples) にあります。
Workbook editing、Template、formula、formatting に依存する前に、
[互換性マトリクス](docs/compatibility.md)で正確な対応範囲を確認してください。

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
