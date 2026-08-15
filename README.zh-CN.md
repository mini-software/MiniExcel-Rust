# MiniExcel Rust XLSX MVP

该目录包含 MiniExcel 基础 XLSX 读写流程的实验性 Rust 实现。目前它属于研究分支，不会替代现有 .NET 包。核心 crate 已可生成 crates.io 发布包，但尚未正式发布。

[English](README.md)

**[打开 MiniExcel Browser Lab](https://mini-software.github.io/MiniExcel-Rust/)**，可以直接在浏览器本地检查或生成 XLSX；上传的工作簿不会离开浏览器。

## 当前能力

- 从路径读取 `.xlsx`。
- 通过 `MiniExcel::query()` 和 `MiniExcel::query_as()` 以有界内存流式读取 worksheet。
- 通过 `MiniExcel::query_structured()` 流式保留稀疏坐标、公式和 number format。
- 使用版本化条件/分组/聚合 plan，并通过最大分组数显式限制内存。
- 为 LLM/RAG 输出带 A1 地址的 JSONL chunks 和 SHA-256 manifest。
- 枚举工作表的索引、类型和可见性元数据，并按名称选择工作表。
- 按表头和 A1 起点语义列出选中列名。
- 使用稳定列顺序的动态行，可选首行表头。
- 通过 Serde 将行反序列化为 Rust 结构体。
- 支持包含结束点的 A1 起止范围、表头修剪和可选空行过滤。
- 从动态行或 Serde 结构体创建新的 `.xlsx` 工作簿。
- 读取时可选择工作表，写入输出到文件路径。
- 支持字符串、布尔值、整数、浮点数、空单元格、Excel 错误、日期、时间、日期时间和时长。
- 提供基于 Web Worker 的 Browser Lab，在本地完成行预览、分组分析和 RAG 导出。

项目使用 Rust 2024，最低支持 Rust 1.85.0。

## 构建

在仓库根目录运行：

```bash
cargo +1.85.0 check --workspace --all-targets --locked
cargo +1.85.0 test --workspace --all-targets --locked
```

仓库会提交 workspace 的 `Cargo.lock`，确保本地研究与 CI 使用同一依赖图。

## 打包

[crates.io](https://crates.io/) 相当于 Rust 生态的 NuGet。目前 `miniexcel` 名称仍可用；只有核心 library 配置为可发布，CLI 和 WebAssembly adapter 仍是 workspace 内部包。

使用下面命令生成并验证与 crates.io 实际接收内容一致的 archive：

```bash
cargo +1.85.0 package --manifest-path miniexcel/Cargo.toml --locked
```

生成文件位于 `target/package/miniexcel-0.1.0.crate`。正式发布前需要 crates.io 账号、已验证邮箱和 API token，并应先检查 archive 内容：

```bash
cargo login
cargo +1.85.0 publish --manifest-path miniexcel/Cargo.toml --locked
```

## 本地 CLI

在仓库根目录运行 CLI：

```bash
cargo +1.85.0 run -p miniexcel-cli -- --help
```

如果当前目录已经是 `miniexcel-cli`，Cargo 会自动找到该 package 和上级 workspace，直接运行：

```bash
cargo +1.85.0 run -- --help
```

`--manifest-path` 始终相对于当前目录解析。在 `miniexcel-cli` 中如需显式指定 workspace manifest，可使用 `--manifest-path ../Cargo.toml`。

列出工作表并检查行数据：

```bash
cargo +1.85.0 run -p miniexcel-cli -- sheets tests/data/xlsx/TestMultiSheet.xlsx

cargo +1.85.0 run -p miniexcel-cli -- query tests/data/xlsx/TestDynamicQueryBasic.xlsx --header --limit 5
```

在 `miniexcel-cli` 目录下，对应命令为：

```bash
cargo +1.85.0 run -- sheets ../tests/data/xlsx/TestMultiSheet.xlsx

cargo +1.85.0 run -- query ../tests/data/xlsx/TestDynamicQueryBasic.xlsx --header --limit 5
```

`query` 支持 `--sheet`、`--header`、`--start-cell`、`--end-cell`、`--ignore-empty-rows` 和 `--format table|json|jsonl`。默认最多显示 20 行；使用 `--limit 0 --format jsonl` 可持续流式输出全部行，JSON 和 table 格式则会先收集选中的行再渲染。

执行版本化分析 plan，或导出 RAG 证据文件：

```bash
cargo +1.85.0 run -p miniexcel-cli -- analyze book.xlsx --header --plan plan.json --format json

cargo +1.85.0 run -p miniexcel-cli -- rag-export book.xlsx --header --chunk-rows 25 --output-prefix ./out/book
```

JSON 契约、操作符及输出保证见[流式分析与 RAG 导出](docs/analytics-and-rag.md)。

## 浏览器 WebAssembly

[Browser Lab](https://mini-software.github.io/MiniExcel-Rust/) 使用 Web Worker 和可复用的 `miniexcel-wasm` workbook session，在浏览器本地执行有界行预览、分组 plan 及 JSONL/manifest 下载。上传的工作簿不会离开设备。在 `web-demo` 运行：

```bash
npm ci
npm run build
npm run test:e2e
```

构建需要 `wasm32-unknown-unknown` target 和 `wasm-bindgen-cli 0.2.127`。Rust workflow 会验证 WASM 构建及 Playwright 桌面/移动端行为。

创建并回读工作簿，或同时运行 Rust/.NET 等价契约：

```bash
cargo +1.85.0 run -p miniexcel-cli -- write-demo ./tmp/miniexcel-demo.xlsx

cargo +1.85.0 run -p miniexcel-cli -- parity --repo-root ../MiniExcel
```

在 `miniexcel-cli` 目录下可简化为：

```bash
cargo +1.85.0 run -- write-demo ./tmp/miniexcel-demo.xlsx

cargo +1.85.0 run -- parity --repo-root ../../MiniExcel
```

完成一次构建后，可直接使用 `target/debug/miniexcel`（Windows 为 `.exe`）。

## .NET 等价验证

.NET 与 Rust 共同消费版本化行为契约 `tests/data/contracts/xlsx-parity-v1.json`，对相同 XLSX fixture、动态/类型化 query 和规范化预期值进行比较。

```bash
cargo +1.85.0 test -p miniexcel --test parity_contract --locked
dotnet test ../MiniExcel/tests/MiniExcel.OpenXml.Tests/MiniExcel.OpenXml.Tests.csproj --framework net10.0 --filter "FullyQualifiedName~RustParityContractTests"
```

只有两条命令都通过，相关行为才视为等价。规范化规则和 v1 明确范围请查看[兼容性研究记录](docs/compatibility.md#net-parity-contract)。

## 公开 API

`MiniExcel` 是主要公开行为入口。reader、writer 和 ZIP/XML parser 类型保留在 crate 内部。crate 根会导出行/配置契约，以及版本化 analytics 和 RAG 支持类型。日期/时间 Serde adapter 位于 `serde_helpers`。

路径和内存 XLSX 数据都可以读取 worksheet 元数据：

```rust
use miniexcel::{MiniExcel, SheetVisibility};

for sheet in MiniExcel::get_sheet_info("book.xlsx")? {
    println!(
        "{} (id={}): {:?}, active={}",
        sheet.name(),
        sheet.id(),
        sheet.visibility(),
        sheet.is_active()
    );
    if sheet.visibility() == SheetVisibility::Hidden {
        println!("{} is hidden", sheet.name());
    }
}
# Ok::<(), miniexcel::Error>(())
```

## 简洁的流式 Query

最接近 `MiniExcel.Query` 的 Rust 写法是返回迭代器：

```rust
use miniexcel::MiniExcel;

for row in MiniExcel::query("book.xlsx")? {
    let row = row?;
    println!("{:?}", row["A"]);
}
# Ok::<(), miniexcel::Error>(())
```

worksheet XML 会被增量解压和解析，行数据通过有界 channel 交给迭代器，并随迭代逐行映射。因此可直接使用 `take`、`filter`、`find` 等操作，不必收集全部行；丢弃迭代器会停止其 worker。工作表、表头、起始单元格和空行选项可通过 `MiniExcel::query_with_options()` 设置。

类型化查询使用同一模式：

```rust
# use serde::Deserialize;
use miniexcel::MiniExcel;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Record {
    name: String,
}

for record in MiniExcel::query_as::<Record>("book.xlsx")? {
    println!("{}", record?.name);
}
# Ok::<(), miniexcel::Error>(())
```

`MiniExcel::query()` 和 `query_as()` 接收路径，因为迭代器存活期间由 worker 持有 ZIP archive。具体 iterator 类型刻意隐藏在 crate 内部。

> **内存边界：** 流式路径会在内存中保留工作簿元数据、样式、shared-string table、少量行 channel 和 parser buffer，但不会保留完整 worksheet XML 或所有行。为了在 `<dimension>` 缺失/过期时仍提供稳定的全局列 schema，并保留 XML 中明确声明的仅样式空行，它会先做一次有界内存元数据扫描，再进行流式输出。峰值内存仍可能随 shared-string table 或单个超大行增长，但不会随 worksheet 总行数增长。

## 流式分组分析

analytics 会在源行流过时执行严格、可序列化的条件与聚合：

```rust
use miniexcel::{AggregateOp, AggregateSpec, HeaderMode, MiniExcel, QueryPlan, ReadOptions};

let options = ReadOptions::new().with_header_mode(HeaderMode::FirstRow);
let plan = QueryPlan::new([
    AggregateSpec::count_all("rows"),
    AggregateSpec::column(AggregateOp::Sum, "Amount", "totalAmount"),
])
.with_group_by(["Category", "Region"])
.with_max_groups(10_000);

let result = MiniExcel::analyze_with_options("book.xlsx", &options, &plan)?;
# Ok::<(), miniexcel::Error>(())
```

路径分析不会保留源数据行；内存会随 shared strings、样式、parser buffer 和不同 group 的状态增长。`max_groups` 会把高基数分组转为确定性错误；当前版本不宣称常量内存，也不会 spill 到磁盘。

## RAG 证据导出

`MiniExcel::export_rag()` 会流式产生稀疏、带来源地址的 chunks。每个 cell 保留 A1、显式类型、公式缓存值、公式文本、style ID 和 number format；manifest 则记录工作簿 SHA-256、工作表可见性、选区、chunk 策略、输出计数、截断和公式缓存限制。

hidden 与 very-hidden 工作表默认拒绝导出，必须显式 opt-in。JSONL chunks 是规范证据格式，manifest 是规范 extraction/provenance 记录。完整语义见[分析与 RAG 契约](docs/analytics-and-rag.md)。

## 动态读取

```rust
use miniexcel::{HeaderMode, MiniExcel, ReadOptions};

let options = ReadOptions::new()
    .with_sheet_name("Data")
    .with_start_cell("B2".parse()?)
    .with_end_cell("E20".parse()?)
    .with_header_mode(HeaderMode::FirstRow);

for row in MiniExcel::query_with_options("book.xlsx", &options)? {
    println!("{:?}", row?["Name"]);
}
# Ok::<(), miniexcel::Error>(())
```

默认的 `HeaderMode::Auto` 表示：`query()` 默认没有表头，`query_as()` 默认使用第一行作为表头。

没有表头时，动态键使用真实 Excel 列名，例如 `A`、`B`、`AA`。为了兼容 MiniExcel，默认保留空行；可通过 `with_ignore_empty_rows(true)` 删除所有单元格都为空的行。

## 类型化读取

```rust
use chrono::NaiveDate;
use miniexcel::MiniExcel;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Release {
    name: String,
    version: u32,
    #[serde(deserialize_with = "miniexcel::serde_helpers::deserialize_date")]
    released_on: NaiveDate,
}

let rows = MiniExcel::query_as::<Release>("book.xlsx")?
    .collect::<miniexcel::Result<Vec<_>>>()?;
# Ok::<(), miniexcel::Error>(())
```

支持 Serde 的 `rename`、`alias`、`default`、`skip` 和 `Option` 语义。首期不移植 MiniExcel 专用的列索引 Attribute。

## 动态写入

```rust
use miniexcel::{CellValue, DynamicRow, MiniExcel, WriteOptions};

let mut row = DynamicRow::new();
row.insert("Name".to_owned(), CellValue::String("MiniExcel".to_owned()));
row.insert("Version".to_owned(), CellValue::Int(2));

MiniExcel::save_as_with_options(
    "book.xlsx",
    &[row],
    &WriteOptions::new().with_sheet_name("Data"),
)?;
# Ok::<(), miniexcel::Error>(())
```

动态 schema 按所有行中键第一次出现的顺序合并，缺失值写为空单元格。需要显式 schema 或仅写表头时，请使用 `MiniExcel::save_as_with_schema()`。

## 类型化写入

```rust
use chrono::NaiveDate;
use miniexcel::{MiniExcel, WriteOptions};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct Release {
    name: String,
    #[serde(serialize_with = "miniexcel::serde_helpers::serialize_date_to_excel")]
    released_on: NaiveDate,
}

let values = [Release {
    name: "MiniExcel Rust".to_owned(),
    released_on: NaiveDate::from_ymd_opt(2026, 8, 13).unwrap(),
}];
let options = WriteOptions::new()
    .with_sheet_name("Releases")
    .with_column_format("ReleasedOn", "yyyy-mm-dd");

MiniExcel::save_as_serialized_with_options("releases.xlsx", &values, &options)?;
# Ok::<(), miniexcel::Error>(())
```

列格式的键是经过 Serde 重命名后的最终字段/表头名称。类型化写入支持结构体及结构体集合；Map 和 `flatten` 应改用动态 API。

## 重要语义

- 未指定工作表时选择工作簿顺序中的第一张表，而不是 active tab。
- 能精确表示为 `i64` 的 XLSX 数值返回 `CellValue::Int`，其他数值返回 `Float`。
- Excel 序列日期不总能区分纯日期、纯时间和日期时间，因此动态读取统一为 `CellValue::DateTime`；ISO 值会尽量保留更具体的类型。
- 公式只读取缓存值，不返回公式表达式。
- `MiniExcel::query()` 和 `query_as()` 会从路径严格流式解析 worksheet XML。
- 分组分析保留与不同 group 数量成比例的状态，并在 `max_groups` 停止。
- RAG 导出不会重新计算公式，hidden sheet 未显式允许时会拒绝处理。
- 流式查询是同步接口，每个活动 query 使用一个 worker thread；首期不包含 async I/O。
- 写入只创建新工作簿并覆盖目标路径，不能修改已有工作簿。

## 首期不包含

CSV、`.xls`、`.xlsb`、`.ods`、模板、宏、图片、合并单元格操作、公式写入、通用样式系统和修改已有工作簿均延后实现。

依赖选择和行为对照请查看[兼容性研究记录](docs/compatibility.md)。