# MiniExcel for Rust

[English](README.md)

[![Crates.io](https://img.shields.io/crates/v/miniexcel.svg)](https://crates.io/crates/miniexcel)
[![文档](https://docs.rs/miniexcel/badge.svg)](https://docs.rs/miniexcel)
[![许可证](https://img.shields.io/crates/l/miniexcel.svg)](LICENSE)

一个支持有界内存流式读取、Serde、结构化单元格、数据分析及 RAG 导出的 Rust XLSX/CSV 读写库。

**[打开 MiniExcel Browser Lab](https://mini-software.github.io/MiniExcel-Rust/)**，可以直接在浏览器本地检查或生成 XLSX，工作簿不会离开浏览器。

## 安装

```bash
cargo add miniexcel
```

MiniExcel 最低支持 Rust 1.85.0。

## Rust 与 .NET 压力测试

将 `MiniExcel-Rust` 与 [.NET MiniExcel 仓库](https://github.com/mini-software/MiniExcel) 放在同级目录，然后从 .NET 仓库运行共用压力测试脚本：

```powershell
pwsh ./benchmarks/compare-rust-dotnet.ps1
```

本测试比较动态流式 Query 性能：Rust 使用 `MiniExcel::query`，.NET 使用 `OpenXmlImporter.Query`，不包含 Save 性能。两种实现会流式读取同一份 100,000 行 XLSX 工作簿。脚本将校验读取行数一致，并报告多轮测试的耗时和峰值工作集。测试结果受运行环境影响，应以同一台机器产生的数据进行比较。

## 功能

- 以有界内存流式读取动态行和类型化行。
- 结构化读取单元格地址、公式和 number format。
- 支持工作表选择、A1 范围、表头和空行过滤。
- 通过 Serde 读写 Rust 类型。
- 支持动态/类型化 CSV 流式查询、保存、追加、编码与 dialect 选项。
- 按稳定列顺序动态创建工作簿。
- 向现有 XLSX 工作簿原子追加 worksheet。
- 在显式内存限制下执行流式筛选和分组分析。
- 为 LLM/RAG 输出带来源地址的 JSONL 和 Markdown。
- 支持字符串、数值、布尔值、错误、日期、时间、日期时间和时长。

## 公开 API

`MiniExcel` 是主要入口。日期/时间 Serde adapter 位于 `serde_helpers`。

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
```

## 简洁的流式 Query

最接近 `MiniExcel.Query` 的 Rust 写法是返回迭代器：

```rust
use miniexcel::MiniExcel;

for row in MiniExcel::query("book.xlsx")? {
    let row = row?;
    println!("{:?}", row["A"]);
}
```

worksheet XML 会被增量解压和解析，行数据通过有界 channel 交给迭代器，并随迭代逐行映射。因此可直接使用 `take`、`filter`、`find` 等操作，不必收集全部行；丢弃迭代器会停止其 worker。工作表、表头、起始单元格和空行选项可通过 `MiniExcel::query_with_options()` 设置。

类型化查询使用同一模式：

```rust
use miniexcel::MiniExcel;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Record {
    name: String,
}

for record in MiniExcel::query_as::<Record>("book.xlsx")? {
    println!("{}", record?.name);
}
```

`MiniExcel::query()` 和 `query_as()` 接收路径，因为迭代器存活期间由 worker 持有 ZIP archive。

### Async Query

启用可选 `async` feature 后，可以 runtime-neutral stream 消费动态或 Serde 类型化 path query：

```rust
use futures_util::StreamExt;
use miniexcel::{CancellationToken, MiniExcel, ReadOptions};

let cancellation = CancellationToken::new();
let mut rows = MiniExcel::query_async_with_options_and_cancellation(
    "book.xlsx",
    &ReadOptions::new(),
    cancellation.clone(),
)?;

while let Some(row) = rows.next().await {
    println!("{:?}", row?);
}
```

`query_async*()` 与 `query_as_async*()` 使用 bounded channel，将 blocking ZIP/XML 工作移出
async executor。它们不会把 filesystem access 变成 async I/O，也不依赖 Tokio。显式取消会
返回可由 `Error::is_cancelled()` 识别的错误；丢弃 stream 会请求取消，但不会阻塞 executor。
Parser 初始化或当前 row 可能会在后台清理完成前继续执行。

显式 schema async row stream 也可原子创建新 workbook：

```rust
let count = MiniExcel::save_as_with_schema_async_with_cancellation(
    "book.xlsx",
    &["Name".to_owned(), "Version".to_owned()],
    rows,
    &WriteOptions::new().with_sheet_name("Async"),
    cancellation,
).await?;
```

显式 schema 使空 stream 与 one-pass producer 的行为保持确定。Row 通过 bounded channel，
并在 blocking constant-memory writer 运行前落盘 spool。Producer error、cancellation、
drop future、validation failure 与 destination race 都会使已有 target 字节不变，或使缺失
target 继续不存在。`with_overwrite_file(true)` 启用原子 replacement。返回 count 不含 header。

## 借用 Reader 与 Writer

对调用方持有的 `Read + Seek` source，可使用 visitor API 流式处理，不会物化全部 row 或转移所有权：

```rust
MiniExcel::visit_rows_from_reader(&mut input, &options, |excel_row, row| {
    println!("{excel_row}: {:?}", row);
    Ok(true)
})?;
```

借用 reader 还支持类型化/structured visitor，以及 sheet name、information、dimension 和 column。这里刻意不提供借用 lazy iterator，因为路径 iterator 会把 reader 移入 worker thread。

动态、显式 schema、类型化和多工作表 workbook 可通过 `*_to_writer` API 写入借用的 `Write + Send` sink。库不会关闭 reader 或 writer。调用结束后的 reader position 不保证；writer 从当前位置开始写入且不截断既有内容，因此调用方应提供空或已截断的 sink。

现有 workbook 可通过独立 borrowed stream 的 `insert*_from_reader_to_writer` API 执行
append 或 replacement。这些 API 要求 `Read + Seek` source 与空的 `Write + Seek`
destination。调用后两者保持 open，但 output 不具备 atomicity，destination error 后也不会
rollback。Source 与 destination 不得指向同一个底层 stream。

> **内存边界：** 流式路径会在内存中保留工作簿元数据、样式、少量行 channel 和 parser buffer。默认情况下，至少 5 MiB 的 shared-string table 会 spill 到带索引的临时文件；丢弃 iterator 后自动删除。可通过 `with_shared_string_disk_cache()`、`with_shared_string_cache_size()` 和 `with_shared_string_cache_path()` 配置，目录必须预先存在。Byte/WASM query 始终将 shared string 保留在内存中。Worksheet XML 和先前 row 永远不会保留；峰值内存仍可能随单个超大 row 增长，但不会随 worksheet 总行数增长。

## 保留结构的流式 Query

当使用方需要源坐标、公式或 number format 时，应使用 structured stream：

```rust
use miniexcel::MiniExcel;

for row in MiniExcel::query_structured("book.xlsx")? {
    for cell in row?.cells() {
        println!(
            "{} value={:?} formula={:?} format={:?}",
            cell.address(),
            cell.value(),
            cell.formula(),
            cell.number_format()
        );
    }
}
```

structured row 只包含 worksheet XML 中明确表示的 cell。Row 和 column index 从 1 开始，`address()` 返回对应的 A1 reference。sheet name 在每个 row 中只存储一次，而不会在每个 cell 上重复。`HeaderMode` 不会为 structured read 消费第一行，因为源 row 会按存储内容原样返回。

公式文本与其缓存值会分别保留。MiniExcel 不计算公式、不展开 shared-formula 定义，也不保证文件生成方刷新过缓存值。style 已知时，会公开原始自定义和标准内置 number format。

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
```

路径分析不会保留源数据行；内存会随 shared strings、样式、parser buffer 和不同 group 的状态增长。`max_groups` 会把高基数分组转为确定性错误；当前版本不宣称常量内存，也不会 spill 到磁盘。

## RAG 证据导出

`MiniExcel::export_rag()` 会流式产生稀疏、带来源地址的 chunks。每个 cell 保留 A1、显式类型、公式缓存值、公式文本、style ID 和 number format；manifest 则记录工作簿 SHA-256、工作表可见性、选区、chunk 策略、输出计数、截断和公式缓存限制。

```rust
use miniexcel::{HeaderMode, MiniExcel, RagExportOptions, ReadOptions};

let options = ReadOptions::new().with_header_mode(HeaderMode::FirstRow);
let mut export = MiniExcel::export_rag(
    "book.xlsx",
    &options,
    &RagExportOptions::new().with_chunk_rows(25),
)?;
for chunk in export.by_ref() {
    println!("{}", chunk?.data_range());
}
println!("{}", export.manifest().source_sha256());
```

hidden 与 very-hidden 工作表默认拒绝导出，必须显式 opt-in。JSONL chunks 是规范证据格式，manifest 是规范 extraction/provenance 记录。完整语义见[分析与 RAG 契约](docs/analytics-and-rag.zh-CN.md)。

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
```

默认的 `HeaderMode::Auto` 表示：`query()` 默认没有表头，`query_as()` 默认使用第一行作为表头。

没有表头时，动态键使用真实 Excel 列名，例如 `A`、`B`、`AA`。为了兼容 MiniExcel，默认保留空行；可通过 `with_ignore_empty_rows(true)` 删除所有单元格都为空的行。

合并单元格默认只保留物理存储的左上角值。使用 `ReadOptions::with_fill_merged_cells(true)` 可在动态、类型化和 byte query 中将该值投影到整个合并范围。Structured query 仍保持稀疏，只暴露物理存储的 cell。

### 命名 Table Query

可按 OpenXML table metadata name 查询，并且不会读取声明 range 外的 cell：

```rust
let rows = MiniExcel::query_table("book.xlsx", "SalesTable", Some("Data"))?
    .collect::<miniexcel::Result<Vec<_>>>()?;
```

`query_table_as::<T>()` 提供 Serde mapping，`query_table_bytes()` 支持内存 XLSX，
`visit_table_rows*_from_reader()` 会保持 borrowed reader open。Table name 会按 table
`name`（不是 `displayName`）进行大小写不敏感匹配。未指定 sheet 时只搜索第一张 worksheet。
Column name 来自 table metadata；除非 `headerRowCount="0"`，否则会跳过物理 header row；
返回完整声明 range，包括 totals row。Path query 继续使用既有有界内存两遍 worksheet pipeline。

### Comments 与 Notes

可在不读取 worksheet row 的情况下读取 threaded comment 与 legacy note：

```rust
let comments = MiniExcel::get_comments("book.xlsx", Some("Data"))?;

for thread in comments.threaded_comments() {
    println!("{}: {}", thread.cell(), thread.text());
    for reply in thread.replies() {
        println!("  {}", reply.text());
    }
}
```

`get_comments_from_bytes()` 与 `get_comments_from_reader()` 为内存和 borrowed source 提供
相同 metadata。结果包含 typed UUID/cell reference、person、provider/user ID、resolved
state、local 或 offset timestamp、reply，以及 legacy note author/text。只有 author marker
为 `tc={thread-id}` 且 cell 同时匹配 threaded root 的 compatibility-shadow note 才会被
抑制；同 cell 的无关 note 仍保留。Comment metadata 会物化，但不会读取 worksheet row。

### CSV

CSV 使用独立的流式 provider，动态值保持为字符串：

```rust
use miniexcel::{CsvConfiguration, CsvEncoding, CsvReadOptions, HeaderMode, MiniExcel};

let options = CsvReadOptions::new()
    .with_header_mode(HeaderMode::FirstRow)
    .with_configuration(CsvConfiguration::new().with_encoding(CsvEncoding::Gbk));

for row in MiniExcel::query_csv_with_options("data.csv", &options)? {
    println!("{:?}", row?["Name"]);
}
```

动态与 Serde API 支持 path、byte 和 borrowed reader/writer。Save/append 可推断 schema，也可
显式指定 schema。配置覆盖单字节 delimiter、CRLF/LF/CR、UTF-8、UTF-16LE/BE、GBK、
Windows-1252、BOM 输出、empty-as-null 读取与 quoting。默认行为尽量匹配 MiniExcel：逗号、
CRLF、UTF-8 BOM、包含空格时加引号、空字段作为空字符串。Record 必须保持一致宽度。CLI
通过 `miniexcel query-csv` 暴露相同 reader。

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
```

支持 Serde 的 `rename`、`alias`、`default`、`skip` 和 `Option` 语义。暂不支持 MiniExcel 专用的列索引 Attribute。

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
```

动态 schema 按所有行中键第一次出现的顺序合并，缺失值写为空单元格。需要显式 schema 或仅写表头时，请使用 `MiniExcel::save_as_with_schema()`。

为兼容 MiniExcel v1，新 worksheet 默认冻结第一个物理 row。使用 `with_freeze_row_count()` 和 `with_freeze_column_count()` 配置物理行列数；两者都设为 `0` 可关闭冻结窗格。

AutoFilter 下拉菜单默认覆盖完整写入范围，包括只有 header 的导出。使用 `with_auto_filter(false)` 可关闭。禁用 header 时，Excel 会把第一个数据 row 当作筛选标题行。

使用 `WriteOptions::with_right_to_left(true)` 可让 worksheet 从右向左显示。它只改变 worksheet view，不改变 cell 坐标或值。

使用 `with_auto_width(true)` 可启用 MiniExcel v1 风格的固定列宽。它只测量数据 payload，不测量 header，并受 `with_min_width()` 与 `with_max_width()` 限制（默认 `8.42857143` 和 `200`）；输出固定 width，不使用 `bestFit`。类型化 row 启用该选项时会额外执行一次轻量 Serde 测宽。与 .NET v1 不同，Rust 不需要单独开启 fast mode。

使用 `with_column_width()` 和 `with_column_hidden()` 可按最终 dynamic/Serde header name 配置每列的固定布局。Explicit width 是 AutoWidth 的起始最小值；hidden 状态不会删除数据，隐藏列仍可查询。

使用 `with_wrap_cell_contents(true)` 可让普通 body value 自动换行。Header、date、time、duration 及设置了 explicit number format 的字段保持不换行，与 MiniExcel v1 的 style 边界一致。

使用 `with_horizontal_alignment()` 和 `with_vertical_alignment()` 配置 body cell 对齐。水平方向支持 left/general、center、right；垂直方向支持 bottom、center、top。Alignment 可与换行和 number format 组合，但不影响 header。

Header 使用 MiniExcel v1 的默认视觉样式：蓝色背景（`#4472C4`）、白字、细边框、不换行、left/general 水平对齐和 bottom 垂直对齐。可通过 `HeaderStyle` 与 `with_header_style()` 自定义换行、RGB 背景色和对齐。Rust 输出不透明 RGB（`FFRRGGBB`），不保留 v1 的 alpha byte。

`TableStyle::Default` 是默认 cell-style 模式，会应用细边框及 header/body 选项。`TableStyle::None` 会移除 header 和 body 的视觉样式，但保留 date/time/custom number format 与 AutoFilter。此选项不会创建 Excel table 或 `xl/tables` package part。

使用 `MiniExcel::save_as_sheets()` 可按输入顺序创建多个工作表；返回值是每张工作表的数据行数：

```rust
let counts = MiniExcel::save_as_sheets(
    "report.xlsx",
    [("Current", current.as_slice()), ("Archive", archive.as_slice())],
    &WriteOptions::new(),
)?;
```

使用 `with_sheet_visibility(name, SheetVisibility::...)` 可按最终 sheet name 配置 visible、hidden 或 very hidden，名称匹配不区分大小写。第一个 visible sheet 自动成为 active；未知名称或全部隐藏的 workbook 会在创建输出前报错。隐藏状态只用于 UI 组织，不是数据保护，隐藏 worksheet 仍可查询。

## 追加或替换 Worksheet

使用 `MiniExcel::insert()` 原子追加一张 visible worksheet。路径不存在时会新建 workbook，并保持相同的数据 row count 语义：

```rust
use miniexcel::{CellValue, DynamicRow, InsertOptions, MiniExcel};

let mut row = DynamicRow::new();
row.insert("Name".to_owned(), CellValue::String("Archived".to_owned()));

let count = MiniExcel::insert(
    "book.xlsx",
    &[row],
    &InsertOptions::new().with_sheet_name("Archive"),
)?;
assert_eq!(count, 1);
```

可原位 replace 现有 worksheet，同时保留其 workbook identity：

```rust
use miniexcel::ExistingSheetPolicy;

let mut replacement_row = DynamicRow::new();
replacement_row.insert("Name".to_owned(), CellValue::String("Replaced".to_owned()));

let count = MiniExcel::insert(
    "book.xlsx",
    &[replacement_row],
    &InsertOptions::new()
        .with_sheet_name("Archive")
        .with_existing_sheet_policy(ExistingSheetPolicy::Replace),
)?;
```

`insert_with_schema()` 接受可返回错误、只消费一次的动态 iterator。源 row 与生成的 donor worksheet XML 都会落盘 spool；row generation、shared-string conversion、style-ID rebase 和 ZIP output 均为流式处理，不会保留完整 worksheet XML。`insert_serialized()` 接受 Serde struct。现有无关 ZIP entry、worksheet identity、formula 和 cached value 均会保留；只有重写 package 完成验证并同步后，才原子替换现有 workbook。

对于两个独立的 borrowed stream，可使用 `insert_from_reader_to_writer()`、
`insert_with_schema_from_reader_to_writer()` 或
`insert_serialized_from_reader_to_writer()`。Source 必须实现 `Read + Seek`，destination
必须实现 `Write + Seek`，调用后两者都保持 open。Destination 必须为空：MiniExcel 不会
truncate，也不会在错误后 rollback，因此 destination 写入失败可能留下部分 XLSX package。
两个 handle 不得指向同一个底层 stream。这些 stream API 保持与 path Insert 相同的 package
行为，但不提供 path API 的 atomic commit 或写后验证保证。

默认的 `ExistingSheetPolicy::Reject` 会不区分大小写地拒绝重复 worksheet name。使用 `ExistingSheetPolicy::Replace` 可原位替换 worksheet，并保留其 workbook 顺序、ID、relationship/path、visibility 与 active state。默认 `TargetRelationshipPolicy::Reject` 只接受没有 worksheet relationship 的 plain target。`RemoveSupported` 可删除 target-owned table、drawing 及其独占 image、comment、VML drawing 和 external hyperlink；pivot、external link、未知 relationship 与 shared/global part 会被拒绝或保守保留。Insert 写入 XLSX package，拒绝 macro-enabled `.xlsm` path，并拒绝 `WriteOptions::with_overwrite_file(true)`，因为 workbook replacement 由 Insert policy 控制。

Insert preflight 会限制 package entry count、单个和累计 control XML、XML attribute size、
XML depth 与 relationship count；同时拒绝不安全或别名 part path、内部 relationship cycle、
重复语义 relationship target 与 Strict OOXML package。Path Insert 会持有跨进程 advisory
lock，并在 commit 前校验 source SHA-256 fingerprint；并发 writer 或外部 source 变化会返回
确定性 conflict，不会静默覆盖更新后的内容。

可以在不改变 package identity 或位置的情况下重命名现有 worksheet：

```rust
MiniExcel::rename_sheet("book.xlsx", "Sheet1", "Archive")?;
```

Source 按大小写不敏感匹配，也支持只改变大小写。Path 更新具备原子性，并与 Insert 共用
lock、fingerprint、package validation、permission preservation 和 raw ZIP copy pipeline。
Duplicate/非法 target name 会在 commit 前被拒绝。Worksheet relationship、ID、顺序、
visibility、active state、formula 和 defined name 均保留。与 .NET `AlterSheet` 一样，引用旧
sheet name 的 formula/defined-name 文本不会自动重写；调用方必须另行更新这些引用。

Visibility 可通过同一条原子 metadata pipeline 修改：

```rust
use miniexcel::SheetVisibility;

MiniExcel::set_sheet_visibility("book.xlsx", "Archive", SheetVisibility::VeryHidden)?;
```

即使隐藏 active worksheet，也会保留 active-tab index。与 .NET `AlterSheet` 不同，Rust 会
拒绝隐藏最后一张 visible worksheet，保证 commit 后的 workbook 至少保留一张 visible sheet。
重复设置当前状态是字节级 no-op。

Worksheet 也可按 0-based index 移动：

```rust
MiniExcel::reorder_sheet("book.xlsx", "Archive", 0)?;
```

负数 index 会 clamp 到第一位，过大的 index 会 clamp 到最后一位，与 .NET `AlterSheet`
一致。Rust 还会 remap `activeTab`、`firstSheet` 和 defined-name `localSheetId`，使这些引用
仍归属于原 worksheet。Relationship、formula 文本、visibility 和 worksheet ID 保持不变；
移动到当前有效 index 是字节级 no-op。

与 .NET `CopyAndAddSheet` 一样，Rust 可将完整 source workbook 复制到独立 destination，
并在复制后的 package 中生成或 replace 一张 worksheet：

```rust
let count = MiniExcel::copy_and_add_sheet(
    "source.xlsx",
    "destination.xlsx",
    &rows,
    &InsertOptions::new().with_sheet_name("Added"),
)?;
```

提供 dynamic、显式 schema one-pass 和 Serde 变体。Source 永不修改；source alias 会被拒绝，
destination 只有在 package validation 后才原子发布。默认拒绝已有 destination，设置
`with_overwrite_file(true)` 后才允许替换。`ExistingSheetPolicy` 与
`TargetRelationshipPolicy` 保持 Insert 语义。不同于 .NET 的 shallow package regeneration，
Rust 会 raw-copy 无关 part，并保留 workbook identity、relationship、defined name、formula、
table、drawing、comment、pivot 与 external link。该 API 复制整个 workbook 并添加数据，
并不是克隆任意选定 worksheet。

启用可选 `async` feature 后，可通过 runtime-neutral
`Stream<Item = miniexcel::Result<DynamicRow>>` 为现有 workbook 的 Insert 提供 row：

```rust
use miniexcel::{CancellationToken, InsertOptions, MiniExcel};

let cancellation = CancellationToken::new();
let count = MiniExcel::insert_with_schema_async_with_cancellation(
    "book.xlsx",
    &["Name".to_owned(), "Version".to_owned()],
    rows,
    &InsertOptions::new().with_sheet_name("Async"),
    cancellation,
).await?;
```

Async API 使用 bounded channel 提供 row backpressure，并把 blocking XLSX 工作隔离到专用
thread；ZIP、XML 和 filesystem I/O 并不是 async。显式 cancellation 会等待 worker cleanup
后返回；drop future 会请求 cooperative cancellation，cleanup 在后台完成。Commit 前由
cancellation 获胜时原 workbook 保持不变；atomic replacement 开始后不能撤销。默认不启用
任何 async runtime，也不依赖 Tokio。

追加 formula-free worksheet 时会保留已有 calculation chain 与 workbook calculation property。Replacement 会完整删除 stale `calcChain` part、relationship 和 content-type override，并设置 `fullCalcOnLoad` 与 `forceFullCalc`，让 Excel 下次打开时重算。MiniExcel 不执行或改写公式；未修改 worksheet 中的 formula 与 cached value 保持原始字节。

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
```

列格式的键是经过 Serde 重命名后的最终字段/表头名称。类型化写入支持结构体及结构体集合；Map 和 `flatten` 应改用动态 API。

## 模板写入

`MiniExcel::save_as_template()` 会填充现有 XLSX package 中的占位符，同时保留 worksheet 样式及其他 workbook part：

```rust
use miniexcel::{MiniExcel, TemplateOptions};
use serde::Serialize;

#[derive(Serialize)]
struct Report<'a> {
    title: &'a str,
    items: Vec<Item<'a>>,
}

#[derive(Serialize)]
struct Item<'a> {
    name: &'a str,
    score: u32,
}

MiniExcel::save_as_template(
    "report.xlsx",
    "template.xlsx",
    &Report {
        title: "Quarterly Report",
        items: vec![Item { name: "Ada", score: 10 }],
    },
    &TemplateOptions::new(),
)?;
```

标量占位符使用 `{{title}}`。包含 `{{items.name}}` 的 row 会按数组 item 数量重复。单独的 number、boolean 和 null 占位符会写成原生 cell value；混合文本写成 inline string。缺失变量默认留空，也可用 `with_ignore_missing_variables(false)` 拒绝。路径输出默认拒绝已有文件，设置 `with_overwrite_file(true)` 后才覆盖。内存模板可使用 `save_as_template_bytes()`。

Enumerable row 中的 cell 可通过多行 conditional block 为每个 item 选择一行内容：

```text
@if(name == Jack)
{{items.name}}
@elseif(score >= 10)
Top {{items.name}}
@else
{{items.department}}
@endif
```

直接 item field 支持 string `==`/`!=`、number 比较和 boolean `==`/`!=`。每个 branch marker
和正文各占一行；每个 item 仍输出一行。Malformed block 与缺失 field 会返回 template error。
Nested block、逻辑表达式和 conditional formula branch 尚不支持。

启用可选 `async` feature 后，`save_as_template_async()` 与
`save_as_template_async_with_cancellation()` 会在 worker thread 执行 blocking template
ZIP/XML 工作，并原子发布通过验证的 path output。Pre-cancellation、render error、drop future
以及 commit 前 cancellation 会保留已有 destination 的原始字节，或让缺失 destination 继续
不存在。该 API 不会把 ZIP/filesystem 操作变成 async，并保留当前模板内存渲染模型。

版本 1 尚不支持 `@group`、参数化 sheet 克隆、`$=` 公式模板或公式重算。

## 重要语义

- 未指定工作表时选择工作簿顺序中的第一张表，而不是 active tab。
- 能精确表示为 `i64` 的 XLSX 数值返回 `CellValue::Int`，其他数值返回 `Float`。
- Excel 序列日期不总能区分纯日期、纯时间和日期时间，因此动态读取统一为 `CellValue::DateTime`；ISO 值会尽量保留更具体的类型。
- 公式只读取缓存值，不返回公式表达式。
- `MiniExcel::query()` 和 `query_as()` 会从路径严格流式解析 worksheet XML。
- 分组分析保留与不同 group 数量成比例的状态，并在 `max_groups` 停止。
- RAG 导出不会重新计算公式，hidden sheet 未显式允许时会拒绝处理。
- 同步流式 query 每个活动 query 使用一个 worker thread。可选 async query、显式 schema export 与 Insert API 通过 bounded channel 包装 blocking XLSX worker；ZIP/XML/filesystem 工作并不是 async I/O。
- Save 会创建新工作簿，并默认拒绝已有目标路径。`MiniExcel::insert*()` 会向现有 `.xlsx` path 原子 append 或严格 replace worksheet；路径不存在时则创建 workbook。`copy_and_add_sheet*()` 创建经过验证、源自 source 的 destination。`rename_sheet()`、`set_sheet_visibility()` 和 `reorder_sheet()` 会原子修改现有 workbook 的 sheet metadata。

## 暂不支持

目前不支持 `.xls`、`.xlsb`、`.ods`、高级模板指令、宏、图片写入、合并单元格操作、公式写入、通用样式系统，以及克隆任意选定 worksheet。

当前支持范围请查看[兼容性矩阵](docs/compatibility.zh-CN.md)；有意保留的差异见 [MiniExcel v1 Insert 迁移说明](docs/insert-v1-migration.zh-CN.md)。