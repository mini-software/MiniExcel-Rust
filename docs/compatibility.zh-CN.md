# Rust XLSX 兼容性说明

[English](compatibility.md)

## 目标

Rust MVP 在统一的 `MiniExcel` facade 后实现最小但实用的 MiniExcel 风格 XLSX 读写接口。它使用聚焦的 OOXML pull parser 完成有界内存路径 query，内部使用 calamine 处理数据和 Serde 转换，并使用 rust_xlsxwriter 生成 workbook。

## 依赖基线

| 依赖 | 锁定 API 版本线 | 用途 | 许可证 | MSRV 说明 |
| --- | --- | --- | --- | --- |
| `atomicwrites` | 0.4 | Windows path insert 的安全跨平台原子替换 | MIT | 仅 Windows 依赖；已使用 Rust 1.85 检查 |
| `async-channel` / `event-listener` / `futures-*` | 2.5 / 5.4 / 0.3 | 可选 runtime-neutral async query/Insert 与 cancellation | MIT OR Apache-2.0 | 仅由 `async` feature 启用；已使用 Rust 1.85 检查 |
| `fs2` | 0.4 | Path insert 的跨平台 advisory lock | MIT OR Apache-2.0 | 已使用 Rust 1.85 检查 |
| `calamine` | 0.35 | XLSX 解析和 Serde row 反序列化 | MIT | 0.35 声明 Rust 1.83 |
| `clap` | 4.6 | 本地 CLI 参数解析 | MIT OR Apache-2.0 | 4.6 声明 Rust 1.85 |
| `rust_xlsxwriter` | 0.96 | 新 XLSX workbook 生成和 Serde 序列化 | MIT OR Apache-2.0 | 0.96 声明 Rust 1.83 |
| `serde` | 1.x | 类型化映射 | MIT OR Apache-2.0 | 由 workspace lockfile 解析 |
| `chrono` | 0.4 | 无时区 Excel date/time 值 | MIT OR Apache-2.0 | 由 workspace lockfile 解析 |
| `indexmap` | 2.x | 稳定的动态 column 顺序 | MIT OR Apache-2.0 | 由 workspace lockfile 解析 |
| `quick-xml` | 0.39 | 增量 OOXML 解析 | MIT | 已锁定并使用 Rust 1.85 检查 |
| `serde_json` | 1.x | Query plan、分析/RAG 输出、等价契约和 CLI JSON | MIT OR Apache-2.0 | 已使用 Rust 1.85 检查 |
| `sha2` | 0.10 | 为 RAG manifest 流式计算 SHA-256 源身份 | MIT OR Apache-2.0 | 已使用 Rust 1.85 检查 |
| `thiserror` | 2.x | 公共 error 组合 | MIT OR Apache-2.0 | 由 workspace lockfile 解析 |
| `uuid` | 1.x | Typed threaded-comment、reply、person 与 legacy-note identifier | MIT OR Apache-2.0 | 由 workspace lockfile 解析 |
| `zip` | 7.2 | 增量 worksheet entry 解压 | MIT | 已锁定并使用 Rust 1.85 检查 |

最新的 `calamine 0.36` 和 `rust_xlsxwriter 0.97` 需要 Rust 1.88。MVP 固定使用前一条 API 版本线，使声明的 Rust 1.85 MSRV 可实际执行，而不只是目标。

## API 映射

| MiniExcel V2 概念 | Rust MVP | 说明 |
| --- | --- | --- |
| OpenXML importer | `MiniExcel` | 具体 reader/parser 类型保持内部可见 |
| 动态 `Query` | `MiniExcel::query()` | 以有界缓冲流式输出拥有所有权的 `IndexMap<String, CellValue>` row |
| 类型化 `Query<T>` | `MiniExcel::query_as<T>()` | 流式处理 row，并逐行应用 Serde 映射 |
| 保留结构的 query | `MiniExcel::query_structured()` | 流式输出稀疏 row，包含从 1 开始的坐标、公式、style ID 和 number format |
| 分组/过滤分析 | `MiniExcel::analyze_with_options()` | 版本化 Rust 扩展；流式处理 row，只保留有界 group/evidence 状态 |
| RAG 证据导出 | `MiniExcel::export_rag()` | 版本化 Rust 扩展；流式输出可转为带地址 JSONL 的 chunk、增强 GFM Markdown 和源 manifest |
| `QueryRange` | `ReadOptions::with_start_cell()` / `with_end_cell()` | 动态和类型化读取使用包含端点的 A1 range |
| `GetSheetNames` | `MiniExcel::get_sheet_names()` | 保持 workbook 顺序 |
| `GetSheetInformations` | `MiniExcel::get_sheet_info()` | 包含 OOXML ID、顺序、名称、类型、visibility 和 active 状态 |
| `GetSheetDimensions` | `MiniExcel::get_sheet_dimensions()` | 按 workbook 顺序返回使用范围，index 从 1 开始 |
| `GetColumns` | `MiniExcel::get_columns()` | 返回选中的动态 key，或空 vector |
| `QueryTable` | `query_table()` / `query_table_as()` / byte 和 borrowed-reader variants | 大小写不敏感 table-name lookup、metadata header 与包含端点的 table bounds |
| 读取 comments 与 notes | `get_comments()` / bytes / borrowed-reader variants | Thread root、reply、person、resolved/timestamp 与 legacy note |
| `startCell` | `ReadOptions::with_start_cell()` | A1 起始坐标 |
| `IgnoreEmptyRows` | `ReadOptions::with_ignore_empty_rows()` | 为兼容 MiniExcel，默认值为 `false` |
| `FillMergedCells` | `ReadOptions::with_fill_merged_cells()` | 默认 `false`；适用于动态、类型化和 byte query |
| OpenXML exporter | `MiniExcel::save_as*()` | 具体 writer 类型保持内部可见；只创建新 workbook |
| 动态导出 | `save_as()` / `save_as_with_schema()` | map 序列化在内部实现 |
| 类型化导出 | `save_as_serialized<T>()` | 内部使用 Serde 映射 |
| 多工作表导出 | `save_as_sheets()` / `save_as_serialized_sheets()` | 保留输入工作表顺序并返回数据行数 |
| `InsertSheet` append/replace | `insert()` / `insert_with_schema()` / `insert_serialized()` / borrowed reader-to-writer variants | Path API 为原子操作；独立 borrowed stream 要求空 sink，并在无原子 commit 的情况下保持相同 package 行为 |
| Async Insert producer | `insert_with_schema_async*()` | 可选 `async` feature；bounded producer channel，XLSX 工作在专用 blocking thread |
| Async path query | `query_async*()` / `query_as_async*()` | 可选 `async` feature；bounded 动态/Serde stream、协作式 cancellation、blocking XLSX worker |
| 每表 visibility | `WriteOptions::with_sheet_visibility()` | visible、hidden、very hidden；第一个 visible sheet 为 active |
| `overwriteFile` | `WriteOptions::with_overwrite_file()` | 默认 `false`；已有路径需要显式允许覆盖 |
| `FreezeRowCount` / `FreezeColumnCount` | `WriteOptions::with_freeze_row_count()` / `with_freeze_column_count()` | 默认冻结一行、零列 |
| `AutoFilter` | `WriteOptions::with_auto_filter()` | 默认 `true`；覆盖完整写入范围 |
| `RightToLeft` | `WriteOptions::with_right_to_left()` | 默认 `false`；只改变 worksheet view |
| `EnableAutoWidth` / `MinWidth` / `MaxWidth` | `WriteOptions::with_auto_width()` / `with_min_width()` / `with_max_width()` | 固定 v1 风格 width；默认关闭、`8.42857143`、`200` |
| 每列 width/hidden | `WriteOptions::with_column_width()` / `with_column_hidden()` | 按最终 header name 映射；explicit width 作为 AutoWidth 起点 |
| `WrapCellContents` | `WriteOptions::with_wrap_cell_contents()` | 默认 `false`；只换行普通 body value |
| Body 水平/垂直对齐 | `WriteOptions::with_horizontal_alignment()` / `with_vertical_alignment()` | 默认 left/general、bottom；header 独立 |
| Header style | `HeaderStyle` / `WriteOptions::with_header_style()` | v1 蓝底白字细边框视觉默认值，可配置 wrap、RGB 和 alignment |
| `TableStyles.Default` / `None` | `TableStyle::Default` / `None` | Cell styling 模式；`None` 保留 number format 与 AutoFilter |
| 基础模板填充 | `save_as_template()` / `save_as_template_bytes()` | 标量占位符与单 row 数组展开；保留 package part |
| 调用方持有的 XLSX input | `visit_*_from_reader()` / metadata `*_from_reader()` | 借用 `Read + Seek`；同步 visitor 模型 |
| 调用方持有的 XLSX output | `save_as*_to_writer()` | 借用 `Write + Send`；动态、schema、类型化和多工作表 |

`MiniExcel` 是唯一公共行为入口。Reader、writer、parser 和具体迭代器类型均为 crate 内部实现。公共支持类型仅限 row/cell value、结构化 provenance row、option、error/result 和 Serde date/time helper。

## 兼容性默认值

- 使用 `HeaderMode::Auto` 的 `MiniExcel::query()` 以 column letter 作为 key，并将第一行视为数据。
- 使用 `HeaderMode::Auto` 的 `MiniExcel::query_as()` 将第一个选中 row 用作 header。
- `MiniExcel::query_structured()` 不会消费 header row，并且只输出 worksheet XML 中明确表示的 cell。
- 未指定名称时，选择 workbook 顺序中的第一个 worksheet。
- 默认保留所选起点和最后一个已使用 cell 之间的空 row。
- 除非启用 `fill_merged_cells`，合并范围只暴露物理存储的左上角值。Structured query 永不合成 merged cell。
- 类型化 header string 默认会 trim。动态 header 遵循 .NET 行为，按存储内容保留非空白文本。
- 空白动态 header 会被省略。重复动态 header 保留首次出现的 key 位置，后续 column 会覆盖 value。
- 已知 schema 中缺失的动态 cell 表示为 `CellValue::Empty`，而不是省略。
- Writer row count 不包含 header row。

## 类型映射

| XLSX 值 | 动态 Rust 值 |
| --- | --- |
| Empty | `CellValue::Empty` |
| Boolean | `CellValue::Bool` |
| `i64` 范围内的精确整数 | `CellValue::Int` |
| 其他 number | `CellValue::Float` |
| Shared/inline string | `CellValue::String` |
| Excel serial date/time | `CellValue::DateTime` |
| Excel duration | `CellValue::Duration` |
| ISO date/time | 可解析时为 `Date`、`Time` 或 `DateTime` |
| Cell error | `CellValue::Error` |
| 通过动态/类型化 query 读取的公式 | 仅缓存结果值 |
| 通过 structured query 读取的公式 | 原始公式文本和缓存结果值；不执行计算 |

类型化转换委托给 calamine 的 Serde deserializer。公共 `serde_helpers` module 提供严格的 chrono helper，将无效值转换为库中带上下文的 `Error::Deserialize` 路径。

类型化写入 chrono 值时，必须使用匹配的 MiniExcel helper（`serialize_date_to_excel`、`serialize_datetime_to_excel` 或 `serialize_time_to_excel`），并设置对应的 `WriteOptions::with_column_format()`。否则，标准 chrono Serde 行为会写入文本，而不是 Excel serial value。

## 内存与 I/O 模型

`MiniExcel::query()` 和 `query_as()` 使用专用的路径流式 backend。worker 拥有 ZIP archive，读取 workbook relationship、style 和 shared string，然后使用 quick-xml 处理 worksheet XML。有界 channel 最多保留 8 个已解析 row。丢弃公共迭代器会断开 channel 并 join worker，因此提前执行 `take` 或 `find` 会停止后续工作。

借用 reader 通过 callback 同步使用相同的两遍 parser。库不会关闭或消费 reader；ZIP discovery 每次调用都可独立 seek，调用结束后的 reader position 不保证。Callback 返回 `false` 会停止 row delivery，callback error 会原样传播。借用 writer 在调用后仍可使用，从当前位置开始写入，库不会执行 truncate。

路径 query 在 `xl/sharedStrings.xml` 未压缩大小至少为 5 MiB 时，默认自动使用带索引的临时文件。`ReadOptions` 可关闭 cache、调整 threshold 或选择一个已存在的 cache 目录。索引使用固定宽度 offset/length record，因此 lookup metadata 不会随 string 数量占用更多内存。正常完成、parser failure 和提前丢弃 iterator 都会通过 worker 持有的 RAII 清理删除文件。Byte/WASM query 保持纯内存模式，因为它们不具备原生临时文件系统契约。

`MiniExcel::query_structured()` 使用相同的有界 pipeline，并额外保留当前 row 与 channel 中明确 cell 的 metadata。sheet name 在每个 row 内共享，number-format string 按 style 共享。缺失 cell 不会扩展成 structured cell object。公式表达式按存储内容原样保留，但 shared formula 不会被展开，缓存值也可能过期。

分组分析消费动态 row stream，而不保留源数据行。内存还会为每个不同 group 保存一个聚合状态和有界源 row evidence list。`QueryPlan::max_groups` 会拒绝超出配置 limit 的 group。结果 limit 不会减少 group-state 内存。版本 1 不实现磁盘 spill、已排序输入聚合或高基数分组的常量内存处理。

路径 RAG 导出保留 parser 状态、重复 header 上下文和一个输出 chunk。manifest 通过单独的有界读取计算源文件 hash。Markdown 包含 stream 级 source/sheet provenance 以及 chunk 内 formula/style/number-format metadata，无需保留先前 chunk。Byte/WASM 工作流不会收集源数据行，但浏览器上传必然在 WebAssembly 内存中保留压缩后的 XLSX 字节；生成的 JSONL、Markdown 和 Blob 下载也会消耗与输出大小相当的内存。Browser Lab 在 Web Worker 中运行这些操作是为了保持响应，而不是声称其内存与路径模式等价。

backend 对所选 worksheet entry 执行两次顺序、有界内存扫描。第一次记录使用范围和紧凑 merged-cell 矩形。这是为了在合法文件省略 `<dimension>` 时保持 MiniExcel 兼容的稳定动态 schema、像 .NET reader 一样保留仅含 style 的 row element，并在不展开地址 map 的情况下支持按需 merged-cell 填充。第二次扫描输出 row，只保留当前活动 merge range 的锚点值。Worksheet XML 和先前 row 永远不会保留；内存主要由内存或磁盘索引的 shared string、style、merge metadata、parser buffer、当前 row 和有界 channel 构成。

内部 writer 组装包含一个或多个工作表的新 ZIP package。路径保存默认拒绝已有文件，也可显式替换。Path Insert API 通过验证后的 package rewrite 与同目录临时文件原子替换来追加或替换 worksheet；未修改的 ZIP entry 和现有 worksheet identity 会保留。独立 borrowed Insert API 接受 `Read + Seek` input 与空的 `Write + Seek` output，调用后两者保持 open，并在不提供 atomic commit、rollback 或写后验证的情况下保持相同 package 行为。可返回错误的显式 schema producer 只消费一次，经磁盘 spool 与 constant-memory worksheet writer 处理。生成的 donor worksheet XML、shared-string conversion、style-ID rebase 与 ZIP insertion 均使用临时文件 stream，因此 worksheet memory 与 row count 无关。Path Insert 还通过 advisory lock 与 commit 前 source fingerprint 防止并发更新丢失。模板填充会在复制的 package 中重写 worksheet XML；worksheet 样式和无关 ZIP part 会保留。数组展开会移动 row/cell 地址并更新 worksheet dimension。公式表达式会保留但不会重算；版本 1 不会在插行后调整公式引用、merge range、table、drawing 或 defined name。

## 测试来源

Rust integration test 复用仓库 `tests/data/xlsx` 下的现有文件，包括：

- 动态 header 和无 header 文件。
- 中间空 row 和 self-closing 空 row。
- 类型化 value 和 trim header 映射。
- 多个 worksheet。
- 没有显式 `r` attribute 的 cell。
- 已验证 Excel row number 的类型化转换失败。
- 严格流式 A1 起点、空 row 过滤、date、trim header 和提前出现的类型化 error。
- 动态、类型化和 byte query 的可选纵向/横向 merged-cell 填充。
- 强制 shared-string 磁盘 spill、索引 lookup、无效目录处理、纯内存 byte query 和提前 drop 清理。
- 借用动态/类型化/structured reader、重复 metadata 读取、callback 停止/error，以及借用动态/schema/类型化/多表 writer。
- structured formula text、缓存值、A1 地址、style ID、内置/自定义 number format、range 和提前丢弃迭代器。

Writer test 通过 `MiniExcel::save_as*()` 生成临时 workbook，并使用 `MiniExcel::query*()` 回读，覆盖动态和类型化 value、date、多工作表、visible/hidden/very-hidden 状态、active sheet 选择、行数、空 schema、默认/自定义/禁用冻结窗格、header/headerless/typed AutoFilter 范围、从右到左 view、有界固定 AutoWidth 输出、explicit/hidden column layout、普通 body 换行及 formatted-value 排除、body 对齐与换行/number format 组合、默认/自定义 header style、默认/最小 cell style 模式、显式 path 覆盖行为和 worksheet name 验证。模板测试覆盖标量与混合文本、原生 number/boolean、XML 转义、公式注入防护、缺失变量策略、空数组与非空数组、多工作表、样式保留、path 覆盖和 byte 工作流。WASM adapter 有原生 unit test，Browser Lab Playwright test 则覆盖生成 workbook 的渲染、query 控件、包含端点的结束 range，以及桌面/移动 viewport。

`TableStyle` 控制普通 cell format，并不是 OOXML table 抽象。两种模式都不会创建 `xl/tables` entry 或 worksheet `tableParts`。

Header 背景色只支持 RGB。MiniExcel v1 将默认色序列化为 ARGB `284472C4`；由于 backend 不保留 spreadsheet fill 的任意 alpha，Rust 输出视觉等价的不透明色 `FF4472C4`。

## .NET 等价契约

.NET 与 Rust 共享的行为由 `tests/data/contracts/xlsx-parity-v1.json` 定义。此文件是以下适配器唯一的预期数据来源：

- [`MiniExcel.OpenXml.Tests/Compatibility/RustParityContractTests.cs`](https://github.com/mini-software/MiniExcel/blob/master/tests/MiniExcel.OpenXml.Tests/Compatibility/RustParityContractTests.cs)
- `miniexcel/tests/parity_contract.rs`

两个适配器都通过公共 API 查询相同 XLSX fixture，规范化各语言的表示，再比较 sheet 顺序、row count、column 顺序、选中 value 和共同转换 error 上下文。规范化会将 null/empty cell、boolean、number、GUID、datetime、duration 和 string 映射为稳定的带 tag 文本。特别是，整数形式的 .NET `double` 与 Rust `CellValue::Int` 会被视为同一个 number，ISO date string 也会与 chrono date/time value 比较。

从仓库根目录运行两端：

```bash
cargo +1.85.0 test -p miniexcel --test parity_contract --locked
dotnet test ../MiniExcel/tests/MiniExcel.OpenXml.Tests/MiniExcel.OpenXml.Tests.csproj --framework net10.0 --filter "FullyQualifiedName~RustParityContractTests"
```

Rust workflow 会在 Linux 和 Windows 上运行 Rust 契约。其 .NET parity job 会 checkout MiniExcel 仓库，将当前 revision 的契约复制到该 checkout，并在 Linux 上运行 .NET adapter。只有在共享契约被有意识地更新且两个 adapter 都通过后，兼容性修改才算完成。

契约只覆盖当前公共交集：动态/类型化路径 query、包含端点的 range query、column name 发现、header 行为、sheet 选择/顺序、A1 起点、空 row/仅 style row、推断 cell reference、scalar/date/duration 映射、trim 后的类型化 header，以及转换 error 的 row/value 上下文。Structured provenance 是 Rust 研究扩展，不构成 .NET 等价声明。Async API 使用相同 fixture 测试，但尚未纳入共享契约；DataReader、template 和写入等价行为也仍不属于版本 1。

## .NET 覆盖边界

| .NET 接口 | Rust 状态 | 共享契约 |
| --- | --- | --- |
| 动态和类型化 XLSX query | 已实现 | 是 |
| 使用 A1 坐标的 `QueryRange` | 已实现 | 是 |
| `GetSheetNames` 和 `GetColumns` | 已实现 | 是 |
| `GetSheetInformations` ID/index/name/type/visibility/active | 已实现 | Rust 使用 .NET fixture 测试 |
| `GetSheetDimensions` | 已实现 | Rust 使用 .NET fixture 测试 |
| 命名 OpenXML `QueryTable` | 已实现 | Rust/.NET 使用 `TestQueryTable.xlsx` 的 focused test |
| Threaded comments 与 legacy notes | 已实现 | Rust/.NET 使用 `TestCommentsAndNotes.xlsx` 的 focused test |
| CSV 动态/类型化 query、save、append 与 columns | 已实现 | Rust 测试以及固定 .NET CSV fixture/基准测试 |
| 新 workbook `SaveAs`（含多工作表） | 已实现并完成 roundtrip 测试 | 尚未 |
| 基础 `SaveAsTemplate` 标量/列表填充 | 已实现并完成 roundtrip 测试 | 尚未 |
| 用于 WASM 的字节数组 query/write | 已实现 | Rust/browser 测试 |
| 版本化分组分析 | Rust 研究扩展 | 否 |
| 带地址 JSONL/Markdown/manifest RAG 导出 | Rust 研究扩展 | 否 |
| Async Insert producer | 已通过可选 feature 实现 | Rust cancellation test；不共享内部行为 |
| Async 动态/类型化 path query | 已通过可选 feature 实现 | Rust parity、cancellation、error 与 cleanup 测试 |
| DataReader 与更广泛的 stream ownership | 延后 | 否 |
| 向现有 `.xlsx` workbook 追加 worksheet | 已实现并原子提交 | Rust 测试；共享 parity contract 尚未扩展 |
| 严格 worksheet replacement | 已支持 plain target 与受支持的 target-owned closure | Rust 测试；删除 stale calcChain 并要求 full recalculation |
| 原子 worksheet rename | 已支持现有 `.xlsx` path | Rust package-preservation 测试；固定 .NET `AlterSheet` 基准 |
| 原子 worksheet visibility mutation | 已支持现有 `.xlsx` path | Rust invariant/rollback 测试；固定 .NET `AlterSheet` 基准 |
| 原子 worksheet reorder | 已支持现有 `.xlsx` path | Rust positional-reference/rollback 测试；固定 .NET `AlterSheet` 基准 |
| 旧式 `.xls`、`.xlsb` 与 `.ods` 格式 | 延后 | 否 |
| Worksheet copy-and-add | 延后 | 否 |
| 高级 template、picture 与 merge | 延后 | 否 |

此矩阵就是覆盖声明：Rust 目前尚未提供与当前 .NET package 完整的 API 等价性。

## 延后工作

SQL 文本解析、`HAVING`、`ORDER BY`、join、window、pivot、磁盘 spill 聚合、向量索引、模型调用、旧 Excel 格式、高级 template 指令与 sheet 克隆、image authoring、merged-cell API、公式计算/依赖展开、公式编写、通用 style、async export/template I/O、async borrowed reader，以及 borrowed XLSX lazy reader，都需要独立的设计与验收里程碑。CSV DataReader/DataTable adapter 已有意替换为 Rust iterator；当前不提供一步式 CSV/XLSX converter，调用方可组合 query 与 save API。支持工作流与有意保留的差异见 [Insert 迁移说明](insert-v1-migration.zh-CN.md)。