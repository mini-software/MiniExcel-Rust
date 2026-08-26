# MiniExcel .NET 功能缺口分析

[English](dotnet-feature-gaps.md)

## 比对范围

本文比对以下本地检出的公开可观察能力。它是一份特定时间点的实现待办清单，并不表示 Rust 应逐项照搬所有 .NET API。

| 项目 | 版本基线 |
| --- | --- |
| MiniExcel-Rust | 基于 `436f1bf` 的工作区（`0.3.0`） |
| MiniExcel .NET | `b9a76d7af62142e0e38545b6905b01a06e8d160e` |

比对依据包括同级目录 `../MiniExcel` 中的 .NET 公开 API、控制实现和聚焦测试，以及 Rust 的公开 `MiniExcel` 门面、选项、集成测试和[兼容性边界](compatibility.zh-CN.md)。

状态定义：

- **已实现**：有实际价值的可观察 XLSX 行为已由 Rust-native API 与跨项目 focused evidence 覆盖。
- **部分实现**：Rust 已覆盖核心场景，但尚未覆盖 .NET 的完整可观察行为。
- **未实现**：Rust 当前没有对应的公开能力。
- **设计不同**：Rust 提供了原生替代方案，但不构成 API 或行为等价。

## 已实现基线

Rust 已支持动态及 Serde 强类型 XLSX 路径查询、闭区间 A1 范围、工作表选择、表头与空行选项、工作表名称/信息/尺寸、有界内存行迭代、结构化单元格来源信息、面向浏览器的字节读取，以及新建单工作表或多工作表 XLSX。除非 .NET 的对应能力明显更广，下文不把这些能力列为缺失。

## 缺口矩阵

| 领域 | 状态 | .NET 已有而 Rust 尚未完整实现的能力 |
| --- | --- | --- |
| 命名表格 | 已实现 | 动态/类型化 path query、byte query 与 borrowed-reader visitor 使用 table metadata header 和 bounds，并按 table name 大小写不敏感匹配。 |
| DataReader 与 DataTable | 设计不同 | Rust 使用 iterator 与 borrowed visitor，不复制 .NET tabular interface。只有具体集成需要时才考虑 Rust-native Arrow/record-batch adapter；此项不阻塞 parity 完成。 |
| 调用方提供的流 | 部分实现 | 已实现借用的同步动态/类型化/structured visitor、metadata 读取、动态/schema/类型化/多表 writer，以及独立 reader-to-writer Insert，并保持 leave-open。借用 lazy iterator、borrowed async stream 和 template stream 仍不支持。 |
| 异步与取消 | 部分实现 | 可选、runtime-neutral 的动态/Serde path query、显式 schema dynamic 与推断 schema Serde path export、基础 template path output 与显式 schema Insert 已支持协作式 cancellation，并在适用位置提供原子发布。高级 template stream、borrowed async I/O 与 progress callback 仍不支持；ZIP 和 filesystem 工作仍在专用 blocking worker 上执行。 |
| 通用保存输入 | 部分实现 | 从普通对象/可枚举对象、字典、`DataTable`、`IDataReader` 和异步枚举导出，并报告进度。Rust 接受动态行或同类型 Serde 切片、bounded dynamic/Serde async stream，并返回每张工作表的行数。 |
| 多工作表导出 | 部分实现 | Rust 可按输入顺序创建 visible、hidden 和 very-hidden 工作表，但尚不能在一次调用中接受异构 Serde 行类型。 |
| 修改现有工作簿 | 已实现 | Rust 可原子 append、严格 replace、rename、修改 visibility、reorder，并执行 .NET 风格的 source-workbook copy-and-add，同时保留无关 package part 与 worksheet identity。Rename 保留 formula 文本；visibility 拒绝隐藏最后一张 visible sheet；reorder remap active/view/local-name index；copy-and-add 保留 source 并原子发布独立 destination。 |
| 模板 | 部分实现 | Rust 可使用 scalar、array、conditional block、validated multirow group 与 `$=` formula 填充 path/byte template；path output 也提供 cancellable async wrapper。Stream、nested/logical condition、grouped/conditional formula、参数化 sheet、formula-reference translation 与公式计算仍不支持。Stale calcChain metadata 会被删除并要求 full recalculation。 |
| 图片与合并处理 | 未实现 | 添加锚定图片，以及通过模板 API 合并相邻相同单元格。结构化读取不等于具备写入能力。 |
| CSV | 已实现 | 动态/Serde path、byte、borrowed query/save API；column discovery；推断/显式 schema append；delimiter/newline/encoding/BOM/null/quoting 配置，以及 `query-csv` CLI。DataReader/DataTable 由 Rust iterator 替代；未暴露 async/progress API 和一步式 CSV/XLSX converter。 |
| 批注与注释 | 已实现 | Path/bytes/borrowed API 返回 threaded root、reply、未解析 person ID、person/provider/user ID、resolved state、typed timestamp 与 legacy note。 |
| Fluent Mapping | 部分实现 | Rust-native `CellMap` 支持从 path、byte 和 borrowed reader 执行有序 exact-cell Serde 读取。Collection start cell/spacing、nested collection 与 mapped export/template API 仍不支持。Formula/format provenance 继续通过 structured read 提供，而不是 mapping metadata。 |
| 特性式字段映射 | 部分实现 | 仍缺列索引/名称特性、本地化表头、公式元数据、自定义动态格式器、字段映射，以及动态列排序/过滤。Serde 可覆盖重命名、别名、默认值、跳过、可选值和自定义序列化；`WriteOptions` 可按最终 header name 配置 width/hidden layout。 |
| 读取配置 | 部分实现 | 区域文化感知转换、缓冲/快速模式，以及部分 null/空字符串行为。合并单元格填充和 shared-string 磁盘 cache 已实现。 |
| 写入配置与样式 | 部分实现 | 仍缺 OOXML table、共享字符串与内联字符串选择，以及更广泛的单元格样式。Rust 已暴露默认/最小 cell style 模式、header output/style、AutoFilter、从右到左 view、冻结行列、有界 AutoWidth、body 换行/对齐和数字格式。 |
| 工作表元数据与流程 | 部分实现 | 仍缺 dynamic sheet alias、class-level sheet selection 和单 reader 遍历全部 sheet。Rust 已覆盖名称、顺序、尺寸、可见性、active state、table metadata 与 comments/notes。 |
| Provider/包模型 | 设计不同 | .NET 组合 OpenXML、CSV、模板和 Fluent Mapping provider。Rust 通过一个 crate 暴露 XLSX/CSV，并提供 CLI/WASM 适配器；包边界不属于 parity 要求。 |

## 证据索引

| 领域 | Rust 证据 | `../MiniExcel` 中的 .NET 证据 |
| --- | --- | --- |
| 公开读写边界 | `miniexcel/src/facade.rs`、`miniexcel/src/options.rs` | `src/MiniExcel.OpenXml/Api/OpenXmlImporter.cs`、`OpenXmlExporter.cs` |
| Async query | `MiniExcel::query_async*` 与 `query_as_async*`；Rust focused parity/cancellation/error/cleanup 测试 | `OpenXmlImporter.QueryAsync`；`MiniExcelOpenXmlImporterAsyncTests` |
| Async export | `MiniExcel::save_as_with_schema_async*` 与 `save_as_serialized_async*`；Rust 显式/推断 schema、rollback/cancellation/cleanup 测试 | `OpenXmlExporter.ExportAsync`；`SaveAsByAsyncEnumerable` 与 empty async-enumerable 测试 |
| Async template | `MiniExcel::save_as_template_async*`；Rust focused rollback/cancellation/cleanup 测试 | `OpenXmlTemplater.SaveAsByTemplateAsync`；scoped basic/cancellation 测试 |
| Template condition | Enumerable-cell `@if`/`@elseif`/`@else` block；Rust sync/async branch/error/style 测试 | `TestIEnumerableConditional` |
| Template group | `@group`/`@header`/`@endgroup` multirow block；Rust sync/async order/error/style 测试 | `GroupTemplateTest`；`TestIEnumerableGrouped` |
| Formula template | 带 final-row/range token 的 `$=` cell；Rust XML/style/calcChain 测试 | `TestIEnumerableWithFormulas`；`CalcChainTests`；async counterpart |
| 表格 | `MiniExcel::query_table*`；Rust focused test 使用完全相同的 `TestQueryTable.xlsx` fixture（SHA-256 `04F719BF9F9E99D9B437A8FB32F8111FD92580A1D29ACAD10B6ED128C0564501`） | `OpenXmlImporter.QueryTableAsync`；`tests/MiniExcel.OpenXml.Tests/Tables/` |
| Comments | `MiniExcel::get_comments*`；Rust focused test 使用 `TestCommentsAndNotes.xlsx`（SHA-256 `3A855CE896ED62DC27C91797432DD89EE081F07CD03AB05BF1B0CD745543A3FC`） | `OpenXmlImporter.RetrieveCommentsAsync`；`tests/MiniExcel.OpenXml.Tests/Comments/` |
| DataReader/DataTable | Rust iterator 与 borrowed visitor 是原生抽象；不计划逐字复制 .NET tabular adapter | `OpenXmlImporter.GetDataReader`、`GetAsyncDataReader`、`QueryAsDataTableAsync`；`tests/MiniExcel.OpenXml.Tests/DataReader/` |
| 多工作表与工作簿修改 | Writer 可创建多个工作表；现有 workbook 支持 append、严格 replacement、全部独立 `AlterSheet` metadata 操作，以及 source-to-destination `CopyAndAddSheet`，并提供原子 package preservation | `OpenXmlExporter.InsertSheetAsync`、`CopyAndAddSheetAsync`、`AlterSheetAsync`；exporter、`MultipleSheets/` 与 `AlterSheets/` 测试 |
| 模板/图片/合并 | 已实现基础模板填充；高级指令与 authoring 仍延期 | `src/MiniExcel.OpenXml/Api/OpenXmlTemplater.cs`；`tests/MiniExcel.OpenXml.Tests/Templates/` |
| CSV/转换 | `MiniExcel::query_csv*`、`save_csv*` 与 `append_csv*`；Rust 测试使用完全相同的 `TestHeader.csv`（`6C2FC27FCA2876F1ECCA17061B8EE23E133ECDB726F8E0B84167E58D86234432`）和 GB2312（`BA8A2505AB271D5575C58CC1FCBE5A5002CEB9E2F43CB95412246E25A50E8B5A`）fixture | `src/MiniExcel.Csv/Api/`；`src/MiniExcel/MiniExcelConverter.cs`；`tests/MiniExcel.Csv.Tests/` |
| 映射 | Serde row mapping 加显式 `CellMap` exact-cell object read | `src/MiniExcel.Core/Attributes/MiniExcelColumnAttribute.cs`；`src/MiniExcel.OpenXml.FluentMapping/`；scoped basic/complex-address 测试 |
| 批注 | `MiniExcel::get_comments*`；Rust focused test 使用 `TestCommentsAndNotes.xlsx`（SHA-256 `3A855CE896ED62DC27C91797432DD89EE081F07CD03AB05BF1B0CD745543A3FC`） | `OpenXmlImporter.RetrieveCommentsAsync`；`src/MiniExcel.OpenXml/Models/Comments.cs`；comment test |
| 配置/样式 | 较窄的 `ReadOptions` 与 `WriteOptions` | `MiniExcelBaseConfiguration`、`OpenXmlConfiguration`、`OpenXmlStyleOptions`；导出测试 |

.NET 中标为 `Async` 的 API 还会通过仓库的同步版本生成机制产生公开同步入口，因此这里比较的是能力，而不只是方法命名差异。

## 建议实现顺序

1. **Borrowed async stream 与 progress**：仅在 ownership、cancellation 与 blocking-I/O 语义清晰时增加 caller-owned async reader/writer integration 和 progress。
2. **高级模板与 collection mapping**：通过独立兼容里程碑增加 formula/merge-aware group、更丰富 conditional expression、参数化 sheet 和确定性 collection layout。
3. **选定 worksheet cloning**：不属于 .NET `CopyAndAddSheet`；仅在有具体 Rust 使用场景时，通过 relationship closure cloning contract 增加。

DataReader/DataTable 属于 .NET 生态抽象，明确不作为逐字 Rust parity 要求。只有出现具体集成需求时，才应设计 Rust-native record-batch 或 table adapter。

## 不计为缺口

- Rust 的分析与 RAG 导出属于 Rust 扩展，不是 .NET 等价声明。
- .NET 数字参数形式的 `QueryRange` 不单列，因为 Rust 的 A1 `CellReference` 边界可表达相同选择。
- 内部实现类和 .NET 专属依赖注入机制不计入，除非它们产生公开可观察行为。
- CSV DataReader/DataTable 和专用 CSV/XLSX converter 不计入，因为 Rust iterator 与组合 query/save 调用已提供原生替代。
- 任意选定 worksheet cloning 不计入，因为 .NET `CopyAndAddSheet` 是复制完整 source workbook 后从 row data 生成 sheet；Rust 已实现该可观察操作。
- 旧式二进制 Excel 格式不计入，因为本次比对未发现当前 .NET V2 有可构成兼容要求的公开 provider。

任一基线 revision 变化后都应重新执行比对。只有两端公开适配器和聚焦测试均覆盖某项行为后，才应把新的等价声明加入共享兼容契约。